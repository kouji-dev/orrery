import { Injector, runInInjectionContext } from "@angular/core";
import { describe, expect, it } from "vitest";
import { AgentsStore } from "./stores/agents.store";
import { TerminalService } from "./terminal.service";
import { flushTerminalQueue } from "./terminal-output-scheduler";

// TerminalService only calls AgentsStore from xterm event handlers (input/resize)
// and the A1.2 recovery path (snapshot) — a bare stub is enough. We build it
// via a plain Injector + runInInjectionContext so no TestBed/zone bootstrap is
// needed (this repo's vitest setup has no Angular test environment).
const stubStore = {
  input: () => Promise.resolve(),
  resize: () => Promise.resolve(),
  snapshot: () => Promise.resolve({ text: "", endSeq: 0 }),
} as unknown as AgentsStore;

function makeService(store: AgentsStore = stubStore): TerminalService {
  const injector = Injector.create({
    providers: [{ provide: AgentsStore, useValue: store }],
  });
  return runInInjectionContext(injector, () => new TerminalService());
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

// Since A0.2 the backend has no single-focus fast path (interest subscription
// supersedes agent_focus) — focus tracking is LOCAL bookkeeping only, used as
// the drop-target fallback. It must track gains, and clear on the focused
// terminal's exit/dispose — and only then.
describe("TerminalService typing-focus sentinel (local, post-A0.2)", () => {
  it("tracks the last focused agent", () => {
    const svc = makeService();
    // @ts-expect-error private funnel the xterm focus listener calls
    svc["setFocused"]("a");
    expect(svc.focusedAgentId()).toBe("a");
    // @ts-expect-error private
    svc["setFocused"]("b");
    expect(svc.focusedAgentId()).toBe("b");
  });

  it("clears when the focused agent's process exits — not another's", () => {
    const svc = makeService();
    svc.write("a", "x"); // materialize the terminals
    svc.write("b", "x");
    // @ts-expect-error private funnel
    svc["setFocused"]("a");
    svc.exit("b"); // some OTHER terminal exiting must not release focus
    expect(svc.focusedAgentId()).toBe("a");
    svc.exit("a");
    expect(svc.focusedAgentId()).toBeNull();
  });

  it("clears when the focused terminal is disposed — and only then", () => {
    const svc = makeService();
    svc.write("a", "x");
    svc.write("b", "x");
    // @ts-expect-error private funnel
    svc["setFocused"]("a");
    svc.dispose("b");
    expect(svc.focusedAgentId()).toBe("a");
    svc.dispose("a");
    expect(svc.focusedAgentId()).toBeNull();
  });
});

// A stopped/finished process is torn down the moment it ends: the pane goes
// visibly empty right there (the backend destroys the matching scrollback ring
// on the same event), and every session-scoped bit of state goes with it.
describe("TerminalService.exit teardown", () => {
  it("empties the pane when the process ends", async () => {
    const svc = makeService();
    svc.write("a", "dead-run\r\n", 5);
    await flush(svc, "a");
    expect(svc.tail("a", 10)).toEqual(["dead-run"]);

    svc.exit("a");
    await flush(svc, "a");
    expect(svc.tail("a", 10)).toEqual([]); // now, not on the next open
  });

  it("drops the stale mark and snapshot dedup so a relaunch streams clean", async () => {
    const store = {
      ...stubStore,
      snapshot: () => Promise.resolve({ text: "ring\r\n", endSeq: 10 }),
    } as unknown as AgentsStore;
    const svc = makeService(store);
    svc.write("a", "x", 1);
    // @ts-expect-error private — leaves minSeq at the snapshot's endSeq (10)
    await svc["recover"]("a");
    svc.markStale("a");

    svc.exit("a");

    // @ts-expect-error read the private stale set
    expect(svc["stale"].has("a")).toBe(false);
    // @ts-expect-error read the private dedup map
    expect(svc["minSeq"].has("a")).toBe(false);
    // the relaunch's stream restarts at seq 1 — a surviving minSeq would eat it
    svc.write("a", "new-run\r\n", 1);
    await flush(svc, "a");
    expect(svc.tail("a", 10)).toEqual(["new-run"]);
  });
});

// A1.2 recovery: replaying a backend snapshot must (1) replace the buffer
// content, (2) drop live chunks that are already inside the snapshot
// (seq <= endSeq), and (3) let newer chunks through — including ones that
// arrive WHILE the recovery is in flight (they are parked, then flushed).
describe("TerminalService snapshot recovery (A1.2)", () => {
  function withSnapshot(snap: { text: string; endSeq: number }) {
    let resolve!: (s: { text: string; endSeq: number }) => void;
    const gate = new Promise<{ text: string; endSeq: number }>((r) => (resolve = r));
    const store = {
      ...stubStore,
      snapshot: () => gate,
    } as unknown as AgentsStore;
    return { svc: makeService(store), release: () => resolve(snap) };
  }

  it("replays the snapshot and dedups live chunks by seq", async () => {
    const { svc, release } = withSnapshot({ text: "from-ring\r\n", endSeq: 10 });
    svc.write("a", "pre-recovery\r\n", 10); // materialize the terminal
    await flush(svc, "a");
    // @ts-expect-error drive the private recovery entry point directly
    const done = svc["recover"]("a") as Promise<void>;
    release();
    await done;
    await flush(svc, "a");
    // stale live chunk — already inside the snapshot → dropped
    svc.write("a", "dupe-from-snapshot\r\n", 9);
    // newer live chunk — resumes the stream
    svc.write("a", "fresh\r\n", 11);
    await flush(svc, "a");
    const tail = svc.tail("a", 10);
    expect(tail).toContain("from-ring");
    expect(tail).toContain("fresh");
    expect(tail).not.toContain("dupe-from-snapshot");
    expect(tail).not.toContain("pre-recovery"); // cleared by the replay
  });

  it("parks chunks that arrive mid-recovery and flushes them seq-deduped", async () => {
    const { svc, release } = withSnapshot({ text: "ring\r\n", endSeq: 5 });
    svc.write("a", "x", 1);
    await flush(svc, "a");
    // @ts-expect-error private
    const done = svc["recover"]("a") as Promise<void>;
    // These land while the snapshot invoke is still pending:
    svc.write("a", "inside-snapshot\r\n", 4); // dupe — must be dropped
    svc.write("a", "after-snapshot\r\n", 6); // new — must survive
    release();
    await done;
    await flush(svc, "a");
    const tail = svc.tail("a", 10);
    expect(tail).toContain("ring");
    expect(tail).toContain("after-snapshot");
    expect(tail).not.toContain("inside-snapshot");
  });

  // The user-visible rule: re-opening an agent shows NO historical PTY dump.
  // The backend withholds the scrollback ring of a session whose PTY is gone
  // (dead/detached run) — recovery then gets an empty snapshot, which must
  // neither dump history nor wipe what a live terminal already holds.
  it("re-opening an agent with no live session replays no history", async () => {
    const { svc, release } = withSnapshot({ text: "", endSeq: 0 });
    // the attach() path for a fresh handle (webview reload / re-opened tab)
    // @ts-expect-error drive the private recovery entry point directly
    const done = svc["recover"]("a") as Promise<void>;
    release();
    await done;
    await flush(svc, "a");
    expect(svc.tail("a", 10)).toEqual([]); // opens empty, not a stale dump
  });

  it("keeps the current run's output when the snapshot is empty", async () => {
    const { svc, release } = withSnapshot({ text: "", endSeq: 0 });
    svc.write("a", "current-run\r\n", 12);
    await flush(svc, "a");
    // @ts-expect-error private
    const done = svc["recover"]("a") as Promise<void>;
    release();
    await done;
    await flush(svc, "a");
    svc.write("a", "still-streaming\r\n", 20); // live stream stays intact
    await flush(svc, "a");
    expect(svc.tail("a", 10)).toEqual(["current-run", "still-streaming"]);
  });

  it("marks the terminal stale again when the snapshot invoke fails", async () => {
    const store = {
      ...stubStore,
      snapshot: () => Promise.reject(new Error("backend gone")),
    } as unknown as AgentsStore;
    const svc = makeService(store);
    svc.write("a", "x", 1);
    svc.markStale("a");
    // @ts-expect-error private
    await svc["recover"]("a");
    // @ts-expect-error read the private stale set — retry on next attach
    expect(svc["stale"].has("a")).toBe(true);
  });
});
