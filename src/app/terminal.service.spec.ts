import { Injector, runInInjectionContext } from "@angular/core";
import { describe, expect, it } from "vitest";
import { AgentsStore } from "./stores/agents.store";
import { TerminalService } from "./terminal.service";
import { flushTerminalQueue } from "./terminal-output-scheduler";

// TerminalService only calls AgentsStore from xterm event handlers (input/resize),
// none of which fire during a write/tail test — a bare stub is enough. We build it
// via a plain Injector + runInInjectionContext so no TestBed/zone bootstrap is
// needed (this repo's vitest setup has no Angular test environment).
const stubStore = {
  input: () => Promise.resolve(),
  resize: () => Promise.resolve(),
  focus: () => Promise.resolve(),
} as unknown as AgentsStore;

function makeService(store: AgentsStore = stubStore): TerminalService {
  const injector = Injector.create({
    providers: [{ provide: AgentsStore, useValue: store }],
  });
  return runInInjectionContext(injector, () => new TerminalService());
}

/** Service whose AgentsStore records every agent_focus invoke. */
function makeFocusSpy(): { svc: TerminalService; calls: Array<string | null> } {
  const calls: Array<string | null> = [];
  const store = {
    ...stubStore,
    focus: (id: string | null) => {
      calls.push(id);
      return Promise.resolve();
    },
  } as unknown as AgentsStore;
  return { svc: makeService(store), calls };
}

// `write` routes through the shared scheduler; an unattached terminal counts as
// hidden, so drain its queue first, then chain an empty write whose callback
// fires once xterm has parsed everything before it.
function flush(svc: TerminalService, id: string): Promise<void> {
  flushTerminalQueue(id);
  return new Promise((r) => {
    // @ts-expect-error reach the underlying term to chain a flush callback
    svc["handle"](id).term.write("", () => r());
  });
}

describe("TerminalService.tail", () => {
  it("returns [] when no terminal exists yet", () => {
    expect(makeService().tail("nope", 3)).toEqual([]);
  });

  it("reads the final rendered text, not stale frames", async () => {
    const svc = makeService();
    // a \r overwrite (progress bar) then a clear-line redraw of the same row
    svc.write("a", "10%\r50%\r100% done\r\n");
    svc.write("a", "downloading\x1b[2K\rdownloaded\r\n");
    await flush(svc, "a");
    // tail sees only the final state of each row — no "10%"/"50%"/"downloading"
    expect(svc.tail("a", 3)).toEqual(["100% done", "downloaded"]);
  });

  it("takes the last n non-empty rows", async () => {
    const svc = makeService();
    svc.write("b", "one\r\ntwo\r\n\r\nthree\r\nfour\r\n");
    await flush(svc, "b");
    expect(svc.tail("b", 2)).toEqual(["three", "four"]);
  });
});

// The backend mux drains the FOCUSED agent every frame and holds everyone
// else to a slow cadence, so TerminalService must tell it which terminal the
// user is in — exactly once per CHANGE (the xterm focus event re-fires on
// every click into an already-focused terminal; flapping invokes would just
// burn IPC). setFocused is the single funnel the xterm focus listener,
// exit() and dispose() all feed.
describe("TerminalService focus → agent_focus", () => {
  it("invokes once per change, not once per focus event", () => {
    const { svc, calls } = makeFocusSpy();
    // @ts-expect-error reach the private funnel the xterm focus listener calls
    svc["setFocused"]("a");
    // @ts-expect-error same terminal re-focused — must NOT re-invoke
    svc["setFocused"]("a");
    expect(calls).toEqual(["a"]);
    // @ts-expect-error switching terminals is a change — one more invoke
    svc["setFocused"]("b");
    expect(calls).toEqual(["a", "b"]);
  });

  it("clears focus when the focused agent's process exits", () => {
    const { svc, calls } = makeFocusSpy();
    svc.write("a", "x"); // materialize the terminal
    // @ts-expect-error private funnel
    svc["setFocused"]("a");
    svc.exit("a");
    expect(calls).toEqual(["a", null]);
    // an exit for an agent that is NOT focused must not touch it
    svc.write("b", "x");
    svc.exit("b");
    expect(calls).toEqual(["a", null]);
  });

  it("retries sending focus after a failed invoke — dedup does not suppress the retry", async () => {
    let rejectNext = false;
    const calls: Array<string | null> = [];
    const store = {
      ...stubStore,
      focus: (id: string | null) => {
        calls.push(id);
        if (rejectNext) {
          rejectNext = false;
          return Promise.reject(new Error("ipc error"));
        }
        return Promise.resolve();
      },
    } as unknown as AgentsStore;
    const svc = makeService(store);

    // First call succeeds — sentFocusId is now "a".
    // @ts-expect-error private
    svc["setFocused"]("a");
    await Promise.resolve();
    expect(calls).toEqual(["a"]);

    // Simulates a different focus then back — sentFocusId temporarily "b" then
    // we call with "a" again which should fail.
    rejectNext = true;
    // @ts-expect-error private
    svc["setFocused"]("b"); // sentFocusId → "b", focus("b") rejects → rolls back to "a"
    // let the rejection handler run
    await Promise.resolve();
    await Promise.resolve();

    // sentFocusId should have been rolled back to "a" (prev before the failed call)
    // @ts-expect-error read the private field
    expect(svc["sentFocusId"]).toBe("a");

    // Now "b" can be retried (dedup won't block it, since sentFocusId ≠ "b")
    // @ts-expect-error
    svc["setFocused"]("b");
    expect(calls).toEqual(["a", "b", "b"]);
  });

  it("clears focus when the focused terminal is disposed — and only then", () => {
    const { svc, calls } = makeFocusSpy();
    svc.write("a", "x");
    svc.write("b", "x");
    // @ts-expect-error private funnel
    svc["setFocused"]("a");
    svc.dispose("b"); // some OTHER terminal going away must not release focus
    expect(calls).toEqual(["a"]);
    svc.dispose("a");
    expect(calls).toEqual(["a", null]);
  });
});
