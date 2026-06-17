import { Injector, runInInjectionContext } from "@angular/core";
import { describe, it, expect, vi, beforeEach } from "vitest";
import { GitInspectStore } from "./git-inspect.store";
import { BRIDGE } from "../data-source/bridge";
import type { CommitFile } from "../models";

// ---------------------------------------------------------------------------
// Mock BRIDGE — resolves/rejects are controlled per test via `resolveFn`.
// ---------------------------------------------------------------------------
let resolveFn: ((v: unknown) => void) | null = null;
let rejectFn: ((e: unknown) => void) | null = null;

const mockBridge = {
  invoke: vi.fn(() =>
    new Promise((res, rej) => {
      resolveFn = res;
      rejectFn = rej;
    }),
  ),
  on: vi.fn(),
  pickDirectory: vi.fn(),
  pickFile: vi.fn(),
};

function flush() {
  // Flush microtask queue so the Promise chain settles.
  return new Promise<void>((r) => setTimeout(r, 0));
}

// ---------------------------------------------------------------------------

describe("GitInspectStore", () => {
  let store: GitInspectStore;

  beforeEach(() => {
    vi.clearAllMocks();
    resolveFn = null;
    rejectFn = null;

    const injector = Injector.create({ providers: [{ provide: BRIDGE, useValue: mockBridge }] });
    store = runInInjectionContext(injector, () => new GitInspectStore());
  });

  // -------------------------------------------------------------------------
  // idle → loading → ready
  // -------------------------------------------------------------------------
  it("commitFilesFor starts idle", () => {
    expect(store.commitFilesFor("a1", "abc123").status).toBe("idle");
    expect(store.commitFilesFor("a1", "abc123").data).toEqual([]);
  });

  it("loadCommitFiles transitions idle → loading → ready", async () => {
    const files: CommitFile[] = [{ path: "src/foo.ts", state: "M", add: 3, del: 1 }];

    store.loadCommitFiles("a1", "abc123");
    expect(store.commitFilesFor("a1", "abc123").status).toBe("loading");

    resolveFn!(files);
    await flush();

    const entry = store.commitFilesFor("a1", "abc123");
    expect(entry.status).toBe("ready");
    expect(entry.data).toEqual(files);
  });

  it("loadCommitFiles transitions to error on rejection", async () => {
    store.loadCommitFiles("a1", "abc123");
    expect(store.commitFilesFor("a1", "abc123").status).toBe("loading");

    rejectFn!(new Error("network failure"));
    await flush();

    expect(store.commitFilesFor("a1", "abc123").status).toBe("error");
  });

  // -------------------------------------------------------------------------
  // generation guard — supersede
  // -------------------------------------------------------------------------
  it("generation guard: a superseded in-flight resolve is ignored", async () => {
    const files1: CommitFile[] = [{ path: "a.ts", state: "A", add: 1, del: 0 }];
    const files2: CommitFile[] = [{ path: "b.ts", state: "M", add: 2, del: 1 }];

    // First call starts
    store.loadCommitFiles("a1", "abc123");
    const firstResolve = resolveFn!;

    // Second call supersedes (new gen)
    store.loadCommitFiles("a1", "abc123");
    const secondResolve = resolveFn!;

    // Resolve the second (newer) one first
    secondResolve(files2);
    await flush();
    expect(store.commitFilesFor("a1", "abc123").data).toEqual(files2);

    // Now resolve the stale first — should be ignored (gen guard)
    firstResolve(files1);
    await flush();
    expect(store.commitFilesFor("a1", "abc123").data).toEqual(files2);
  });

  // -------------------------------------------------------------------------
  // dispose
  // -------------------------------------------------------------------------
  it("dispose drops all entries whose key starts with id + '/'", async () => {
    const files: CommitFile[] = [{ path: "x.ts", state: "A", add: 5, del: 0 }];

    store.loadCommitFiles("agent-1", "sha111");
    resolveFn!(files);
    await flush();
    expect(store.commitFilesFor("agent-1", "sha111").status).toBe("ready");

    store.loadFileHistory("agent-1", "src/x.ts");
    resolveFn!([]); // fileHistory returns empty array
    await flush();
    expect(store.fileHistoryFor("agent-1", "src/x.ts").status).toBe("ready");

    // A second agent's data must survive the dispose
    store.loadCommitFiles("agent-2", "sha222");
    resolveFn!(files);
    await flush();

    // Dispose agent-1 only
    store.dispose("agent-1");

    expect(store.commitFilesFor("agent-1", "sha111").status).toBe("idle");
    expect(store.fileHistoryFor("agent-1", "src/x.ts").status).toBe("idle");
    // agent-2 entry is unaffected
    expect(store.commitFilesFor("agent-2", "sha222").status).toBe("ready");
  });

  // -------------------------------------------------------------------------
  // blameFor / loadBlame (with optional rev)
  // -------------------------------------------------------------------------
  it("loadBlame passes rev to bridge when provided", () => {
    store.loadBlame("a1", "src/foo.ts", "HEAD~3");
    expect(mockBridge.invoke).toHaveBeenCalledWith(
      "agent_blame",
      expect.objectContaining({ id: "a1", path: "src/foo.ts", rev: "HEAD~3" }),
    );
  });

  it("loadBlame omits rev when not provided", () => {
    store.loadBlame("a1", "src/foo.ts");
    const call = mockBridge.invoke.mock.calls[0][1] as Record<string, unknown>;
    expect(call["rev"]).toBeUndefined();
  });
});
