// The SDK against a fake host: no harness, no model, no network.
//
// The host half is tested in Rust (`orrery-host-rpc`); this is the guest half
// on its own, which is what an extension author is actually running.

import assert from "node:assert/strict";
import { PassThrough } from "node:stream";
import test from "node:test";

import { Decoder, encode } from "../src/framing.mjs";
import { defineExtension, outcome, serve } from "../src/index.mjs";
import { STRIPPED, resolve } from "../src/loader.mjs";

/** A host on a pair of pipes: sends requests, collects replies. */
function host(definition) {
  const toGuest = new PassThrough();
  const fromGuest = new PassThrough();
  const decoder = new Decoder();
  const pending = new Map();
  const requests = [];
  let handleGuestRequest = async () => ({});

  fromGuest.on("data", (chunk) => {
    for (const message of decoder.push(chunk)) {
      if (message.method !== undefined) {
        requests.push(message);
        if (message.id !== undefined) {
          Promise.resolve(handleGuestRequest(message.method, message.params)).then(
            (result) => toGuest.write(encode({ jsonrpc: "2.0", id: message.id, result })),
            (error) =>
              toGuest.write(
                encode({
                  jsonrpc: "2.0",
                  id: message.id,
                  error: { code: error.code ?? -32001, message: error.message, data: error.data },
                }),
              ),
          );
        }
        continue;
      }
      const waiting = pending.get(message.id);
      pending.delete(message.id);
      waiting?.(message);
    }
  });

  const served = serve(definition, { input: toGuest, output: fromGuest });
  let nextId = 1;

  return {
    served,
    requests,
    onGuestRequest(fn) {
      handleGuestRequest = fn;
    },
    ask(method, params) {
      const id = nextId++;
      const answer = new Promise((resolve) => pending.set(id, resolve));
      toGuest.write(encode({ jsonrpc: "2.0", id, method, params }));
      return answer;
    },
    tell(method, params) {
      toGuest.write(encode({ jsonrpc: "2.0", method, params }));
    },
    close() {
      toGuest.end();
    },
  };
}

const BUDGET = { wall_clock_ms: 30_000, output_bytes: 1 << 20 };

test("ext/load reports what the extension contributes", async () => {
  const ext = defineExtension({
    serve: false,
    tools: {
      say: {
        description: "Say it back",
        input: { type: "object", properties: { text: { type: "string" } } },
        requires: ["read"],
        async run({ text }) {
          return { said: text };
        },
      },
    },
  });
  const h = host(ext);

  const reply = await h.ask("ext/load", {});
  assert.equal(reply.result.tools.length, 1);
  assert.deepEqual(reply.result.tools[0].name, "say");
  assert.deepEqual(reply.result.tools[0].requires, ["read"]);
  assert.equal(reply.result.tools[0].input_schema.type, "object");
  h.close();
  await h.served;
});

test("a tool's value comes back as an ok outcome", async () => {
  const ext = defineExtension({
    serve: false,
    tools: { say: { async run({ text }) { return { said: text }; } } },
  });
  const h = host(ext);

  const reply = await h.ask("tool/call", {
    call: "c1",
    tool: "say",
    input: { text: "hello" },
    budget: BUDGET,
  });
  assert.equal(reply.result.outcome.t, "ok");
  assert.deepEqual(reply.result.outcome.value, { said: "hello" });
  h.close();
  await h.served;
});

test("a surface from ctx.ui is the outcome's surface", async () => {
  const ext = defineExtension({
    serve: false,
    tools: {
      rows: {
        async run(_input, ctx) {
          return ctx.ui.table({ columns: ["module"], rows: [["core"]] });
        },
      },
    },
  });
  const h = host(ext);

  const reply = await h.ask("tool/call", {
    call: "c1",
    tool: "rows",
    input: {},
    budget: BUDGET,
  });
  assert.equal(reply.result.outcome.t, "ok");
  assert.equal(reply.result.outcome.surface.kind.t, "table");
  assert.deepEqual(reply.result.outcome.surface.kind.rows, [[{ text: "core" }]]);
  h.close();
  await h.served;
});

test("ctx.proc.run travels up the same connection as a broker request", async () => {
  const ext = defineExtension({
    serve: false,
    tools: {
      build: {
        async run(_input, ctx) {
          const out = await ctx.proc.run("java", ["-version"], { timeoutMs: 1000 });
          return { stdout: out.stdout };
        },
      },
    },
  });
  const h = host(ext);
  h.onGuestRequest(async (method, params) => {
    assert.equal(method, "broker/spawn");
    assert.equal(params.program, "java");
    assert.deepEqual(params.args, ["-version"]);
    return { status: 0, stdout: "17.0.1", stderr: "", truncated: false };
  });

  const reply = await h.ask("tool/call", {
    call: "c1",
    tool: "build",
    input: {},
    budget: BUDGET,
  });
  assert.deepEqual(reply.result.outcome.value, { stdout: "17.0.1" });
  h.close();
  await h.served;
});

test("a denial is an outcome, not a crash", async () => {
  const ext = defineExtension({
    serve: false,
    tools: {
      build: {
        async run(_input, ctx) {
          await ctx.proc.run("java", []);
          return { unreachable: true };
        },
      },
    },
  });
  const h = host(ext);
  h.onGuestRequest(async () => {
    throw Object.assign(new Error("no `spawn` grant covers `java`"), {
      code: -32001,
      data: { denied: true, rule: "11111111-1111-1111-1111-111111111111" },
    });
  });

  const reply = await h.ask("tool/call", {
    call: "c1",
    tool: "build",
    input: {},
    budget: BUDGET,
  });
  assert.equal(reply.result.outcome.t, "denied");
  assert.match(reply.result.outcome.reason, /spawn/);
  h.close();
  await h.served;
});

test("$/cancel reaches one call and leaves the other alone", async () => {
  let release;
  const held = new Promise((r) => {
    release = r;
  });
  const ext = defineExtension({
    serve: false,
    tools: {
      wait: {
        async run(_input, ctx) {
          await new Promise((resolve, reject) => {
            ctx.signal.addEventListener("abort", () => reject(new DOMException("x", "AbortError")));
            held.then(resolve);
          });
          return { finished: true };
        },
      },
    },
  });
  const h = host(ext);

  const doomed = h.ask("tool/call", { call: "c1", tool: "wait", input: {}, budget: BUDGET });
  const spared = h.ask("tool/call", { call: "c2", tool: "wait", input: {}, budget: BUDGET });
  await new Promise((r) => setTimeout(r, 10));
  h.tell("$/cancel", { call: "c1" });

  const first = await doomed;
  assert.equal(first.result.outcome.t, "cancelled");

  release();
  const second = await spared;
  assert.equal(second.result.outcome.t, "ok", "the other call is untouched");
  h.close();
  await h.served;
});

test("a tool that throws fails, and says what threw", async () => {
  const ext = defineExtension({
    serve: false,
    tools: { boom: { async run() { throw new Error("it broke"); } } },
  });
  const h = host(ext);
  const reply = await h.ask("tool/call", { call: "c1", tool: "boom", input: {}, budget: BUDGET });
  assert.equal(reply.result.outcome.t, "failed");
  assert.match(reply.result.outcome.message, /it broke/);
  h.close();
  await h.served;
});

test("an unknown method is an error, not a silent success", async () => {
  const ext = defineExtension({ serve: false, tools: { say: { async run() { return {}; } } } });
  const h = host(ext);
  const reply = await h.ask("ext/nonsense", {});
  assert.ok(reply.error, "the guest refuses a method it does not implement");
  h.close();
  await h.served;
});

test("defineExtension refuses a tool with no run", () => {
  assert.throws(
    () => defineExtension({ serve: false, tools: { broken: { description: "no run" } } }),
    /has no `run`/,
  );
});

test("the loader hook refuses fs and names what to use instead", async () => {
  for (const specifier of STRIPPED.keys()) {
    await assert.rejects(
      () => resolve(specifier, {}, async () => ({ url: "file:///anything" })),
      (e) => e.code === "ERR_MODULE_NOT_FOUND" && /ctx\./.test(e.message),
      `\`${specifier}\` is stripped`,
    );
  }
  // And it is a *convenience*: anything else still resolves.
  const passed = await resolve("node:path", {}, async () => ({ url: "file:///path" }));
  assert.equal(passed.url, "file:///path");
});

test("outcome builders produce the shapes the host deserialises", () => {
  assert.deepEqual(outcome.failed("x", "y"), { t: "failed", code: "x", message: "y" });
  assert.equal(outcome.cancelled().reason, "user");
  assert.equal(outcome.truncated(undefined, 10, 4).bytes_emitted, 10);
});

test("framing survives a split read", () => {
  const bytes = Buffer.concat([
    encode({ jsonrpc: "2.0", id: 1, method: "a" }),
    encode({ jsonrpc: "2.0", id: 2, method: "b" }),
  ]);
  const decoder = new Decoder();
  const first = decoder.push(bytes.subarray(0, 20));
  assert.equal(first.length, 0, "half a header is not a message");
  const rest = decoder.push(bytes.subarray(20));
  assert.deepEqual(rest.map((m) => m.method), ["a", "b"]);
});
