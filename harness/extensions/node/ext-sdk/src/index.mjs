// @orrery/ext — write an Orrery extension in Node.
//
// ```js
// import { defineExtension } from "@orrery/ext";
//
// export default defineExtension({
//   tools: {
//     impacted: {
//       description: "Modules impacted by the current diff",
//       input: { type: "object", properties: { since: { type: "string" } } },
//       async run({ since = "HEAD~1" }, ctx) {
//         const out = await ctx.proc.run("java", ["-jar", "bg.jar", "--since", since]);
//         return ctx.ui.table({ columns: ["module", "reason"], rows: parse(out.stdout) });
//       },
//     },
//   },
// });
// ```
//
// **Read `README.md` before trusting the loader hook.** Stripping `fs` and
// `child_process` is an SDK convenience, not a boundary the host enforces.

import { METHOD, makeCtx, outcome } from "./ctx.mjs";
import { BrokerDenied, Cancelled, Peer } from "./rpc.mjs";

export { BrokerDenied, Cancelled, outcome };

/**
 * Declare an extension, and serve it over stdio.
 *
 * The host starts `node index.mjs` and speaks JSON-RPC to it, so there is
 * nothing else for a guest's entry point to do — calling this *is* running.
 * `serve: false` returns the definition without starting, which is how a test
 * calls a tool directly.
 */
export function defineExtension(definition) {
  const checked = validate(definition);
  if (definition.serve !== false && !process.env.ORRERY_EXT_NO_SERVE) {
    // On the next tick, so a module that defines and then exports is finished
    // before the first `ext/load` arrives.
    queueMicrotask(() => {
      serve(checked).catch((e) => {
        process.stderr.write(`${e?.stack ?? e}\n`);
        process.exit(1);
      });
    });
  }
  return checked;
}

/** Serve a definition on a pair of streams. Returns when the host goes away. */
export async function serve(definition, { input = process.stdin, output = process.stdout } = {}) {
  const inFlight = new Map();

  const peer = new Peer(input, output, {
    async request(method, params) {
      switch (method) {
        case METHOD.load:
          return {
            tools: Object.entries(definition.tools).map(([name, tool]) => ({
              name,
              description: tool.description ?? "",
              input_schema: tool.input ?? { type: "object" },
              atomic: tool.atomic === true,
              requires: tool.requires ?? [],
              ceiling: tool.ceiling ?? null,
            })),
            problems: definition.problems ?? [],
          };

        case METHOD.call: {
          const tool = definition.tools[params.tool];
          if (!tool) {
            return { outcome: outcome.failed("no-such-tool", `no tool \`${params.tool}\``) };
          }
          const controller = new AbortController();
          inFlight.set(params.call, controller);
          try {
            return { outcome: await run(peer, tool, params, controller.signal) };
          } finally {
            inFlight.delete(params.call);
          }
        }

        case METHOD.shutdown:
          // Answer first: an unanswered shutdown looks like a guest that hung.
          queueMicrotask(() => process.exit(0));
          return null;

        default:
          throw new Error(`no such method: ${method}`);
      }
    },

    notify(method, params) {
      if (method !== METHOD.cancel) return;
      // One call, not the connection.
      const controller = inFlight.get(params?.call ?? params?.id);
      if (controller) controller.abort();
      else for (const c of inFlight.values()) c.abort();
    },
  });

  await peer.listen();
}

/** Run one tool and turn whatever it did into an `Outcome`. */
async function run(peer, tool, params, signal) {
  const ctx = makeCtx(peer, {
    call: params.call,
    tool: params.tool,
    budget: params.budget,
    signal,
  });
  try {
    const answer = await tool.run(params.input ?? {}, ctx);
    if (answer && typeof answer === "object" && typeof answer.t === "string") {
      return answer; // the tool built its own outcome
    }
    if (answer && typeof answer === "object" && answer.kind) {
      return outcome.ok(null, answer); // a surface from `ctx.ui`
    }
    return outcome.ok(answer ?? null, ctx.ui.last ?? undefined);
  } catch (e) {
    if (signal.aborted || e?.name === "AbortError" || e instanceof Cancelled) {
      return outcome.cancelled();
    }
    // A denial is a value everywhere else in the system; it must not arrive at
    // the model dressed up as a crash.
    if (e instanceof BrokerDenied) {
      return e.rule
        ? outcome.denied(e.rule, e.message)
        : outcome.failed("denied", e.message);
    }
    return outcome.failed("tool-threw", e?.stack ?? e);
  }
}

/** Refuse a definition that would fail confusingly at load. */
function validate(definition) {
  if (!definition || typeof definition !== "object") {
    throw new TypeError("defineExtension takes an object");
  }
  if (!definition.tools || typeof definition.tools !== "object") {
    throw new TypeError("an extension declares `tools`");
  }
  for (const [name, tool] of Object.entries(definition.tools)) {
    if (typeof tool?.run !== "function") {
      throw new TypeError(`tool \`${name}\` has no \`run\``);
    }
  }
  return definition;
}
