import { Injector, runInInjectionContext } from "@angular/core";
import { beforeEach, describe, expect, it } from "vitest";
import { AgentWorkStore, COMMITS_PAGE } from "./agent-work.store";
import { BRIDGE, Bridge, Commands } from "../data-source/bridge";
import { Commit } from "../models";

function commit(sha: string): Commit {
  return { sha, msg: sha, agent: "a", when: "1m", files: 1 } as unknown as Commit;
}

// Bridge stub queueing manual resolvers per invoke, so each test drives async
// completion order explicitly. Mirrors cost.store.spec.ts (no Angular test env).
describe("AgentWorkStore", () => {
  let invokes: Array<{ cmd: string; payload?: Record<string, unknown> }>;
  let resolvers: Array<(v: unknown) => void>;
  let store: AgentWorkStore;

  beforeEach(() => {
    invokes = [];
    resolvers = [];
    const bridge = {
      invoke: (cmd: string, payload?: Record<string, unknown>) => {
        invokes.push({ cmd, payload });
        return new Promise((res) => resolvers.push(res));
      },
      on: () => Promise.resolve(() => {}),
      pickDirectory: () => Promise.resolve(null),
    } as unknown as Bridge;
    const injector = Injector.create({ providers: [{ provide: BRIDGE, useValue: bridge }] });
    store = runInInjectionContext(injector, () => new AgentWorkStore());
  });

  it("changes: idle -> loading -> ready, and untouched ids keep reference identity", async () => {
    expect(store.changesFor("a").status).toBe("idle");
    store.loadChanges("b"); // unrelated entry exists first
    resolvers.shift()!([]);
    await Promise.resolve();
    const bBefore = store.changesFor("b");

    store.loadChanges("a");
    expect(store.changesFor("a").status).toBe("loading");
    resolvers.shift()!([{ path: "x", add: 1, del: 0, state: "M" }]);
    await Promise.resolve();
    expect(store.changesFor("a").status).toBe("ready");
    expect(store.changesFor("a").data.length).toBe(1);
    expect(store.changesFor("b")).toBe(bBefore); // identity preserved
  });

  it("ensureCommits loads page one once; loadMoreCommits appends with offset", async () => {
    store.ensureCommits("a");
    store.ensureCommits("a"); // second ensure is a no-op
    expect(invokes.filter((i) => i.cmd === Commands.AgentCommits).length).toBe(1);
    expect(invokes[0].payload).toEqual({ id: "a", limit: COMMITS_PAGE, offset: 0 });
    resolvers.shift()!(Array.from({ length: COMMITS_PAGE }, (_, i) => commit(`s${i}`)));
    await Promise.resolve();
    expect(store.commitsFor("a").hasMore).toBe(true);

    store.loadMoreCommits("a");
    expect(invokes[1].payload).toEqual({ id: "a", limit: COMMITS_PAGE, offset: COMMITS_PAGE });
    resolvers.shift()!([commit("tail")]);
    await Promise.resolve();
    expect(store.commitsFor("a").data.length).toBe(COMMITS_PAGE + 1);
    expect(store.commitsFor("a").hasMore).toBe(false); // short page = end
  });

  it("refreshCommits keeps previous rows visible while reloading (SWR)", async () => {
    store.ensureCommits("a");
    resolvers.shift()!([commit("s1")]);
    await Promise.resolve();
    store.refreshCommits("a");
    expect(store.commitsFor("a").status).toBe("loading");
    expect(store.commitsFor("a").data.length).toBe(1); // stale rows kept
    resolvers.shift()!([commit("s2"), commit("s1")]);
    await Promise.resolve();
    expect(store.commitsFor("a").data.map((c) => c.sha)).toEqual(["s2", "s1"]);
  });

  it("superseded loads are discarded (generation guard)", async () => {
    store.loadChanges("a");
    const first = resolvers.shift()!;
    store.loadChanges("a"); // supersedes
    resolvers.shift()!([{ path: "new", add: 1, del: 0, state: "A" }]);
    await Promise.resolve();
    first([{ path: "old", add: 9, del: 9, state: "M" }]); // stale resolve arrives late
    await Promise.resolve();
    expect(store.changesFor("a").data[0].path).toBe("new");
  });

  it("onWorktreeChanged reloads changes always, tree only when previously loaded", async () => {
    store.onWorktreeChanged("a");
    expect(invokes.map((i) => i.cmd)).toEqual([Commands.AgentChanges]); // no tree: idle
    store.ensureTree("a");
    resolvers[1]!([]);
    await Promise.resolve();
    store.onWorktreeChanged("a");
    expect(invokes.map((i) => i.cmd)).toEqual([
      Commands.AgentChanges,
      Commands.AgentTree,
      Commands.AgentChanges,
      Commands.AgentTree,
    ]);
  });
});
