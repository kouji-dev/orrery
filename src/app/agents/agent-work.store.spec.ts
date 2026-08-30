import { Injector, runInInjectionContext } from "@angular/core";
import { beforeEach, describe, expect, it } from "vitest";
import { AgentWorkStore, COMMITS_PAGE } from "./agent-work.store";
import { BRIDGE, Bridge, Commands } from "../data-source/bridge";
import { AgentFile, Commit } from "../models";

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

  // ----- sidebar counter totals (separate map, never LRU-evicted) -----

  const file = (path: string, add: number, del: number): AgentFile =>
    ({ path, add, del, state: "M" }) as AgentFile;

  it("initTotals populates counters for every agent in one call; scans win over a late init", async () => {
    store.initTotals();
    store.initTotals(); // guarded — one invoke
    expect(invokes.filter((i) => i.cmd === Commands.AgentChangeTotals).length).toBe(1);
    // a full scan lands while the init call is in flight
    store.applyScan("a", [file("x", 7, 2)], "h1", true);
    resolvers.shift()!([
      { id: "a", add: 1, del: 1, files: 1 }, // stale vs the scan
      { id: "b", add: 4, del: 0, files: 2 },
    ]);
    await Promise.resolve();
    expect(store.totalsFor("a")).toEqual({ add: 7, del: 2, files: 1 }); // scan won
    expect(store.totalsFor("b")).toEqual({ add: 4, del: 0, files: 2 });
  });

  it("a counts-only scan updates the file count but never zeroes line totals", () => {
    store.applyScan("a", [file("x", 5, 3)], "h1", true);
    // A2.2 background scan: same file plus a new one, all add/del = 0
    store.applyScan("a", [file("x", 0, 0), file("y", 0, 0)], "h1", false);
    expect(store.totalsFor("a")).toEqual({ add: 5, del: 3, files: 2 });
  });

  it("LRU eviction keeps totals; dropTotals (agent removal) removes them", () => {
    for (const id of ["a1", "a2", "a3", "a4", "a5"]) store.applyScan(id, [file("x", 1, 1)], "h", true);
    // a1 was evicted from the heavy maps…
    expect(store.changesFor("a1").status).toBe("idle");
    // …but its sidebar counters survive
    expect(store.totalsFor("a1")).toEqual({ add: 1, del: 1, files: 1 });
    store.dropTotals("a1");
    expect(store.totalsFor("a1")).toBeNull();
  });

  it("applyScan stores ready changes with zero bridge calls", () => {
    store.applyScan("a", [{ path: "x", add: 1, del: 0, state: "M" as const }], "h1");
    expect(store.changesFor("a").status).toBe("ready");
    expect(store.changesFor("a").data[0].path).toBe("x");
    expect(invokes.length).toBe(0);
  });

  it("applyScan reloads tree only when previously loaded", async () => {
    store.applyScan("a", [], "h1");
    expect(invokes.length).toBe(0); // tree idle → no pull
    store.ensureTree("a");
    resolvers.shift()!([]);
    await Promise.resolve();
    store.applyScan("a", [], "h1");
    expect(invokes.map((i) => i.cmd)).toEqual([Commands.AgentTree, Commands.AgentTree]);
  });

  it("applyScan refreshes commits only on a HEAD move, and only when loaded", async () => {
    store.applyScan("a", [], "h1"); // commits idle → nothing
    store.ensureCommits("a");
    resolvers.shift()!([commit("s1")]);
    await Promise.resolve();
    store.applyScan("a", [], "h1"); // same head → no refresh
    expect(invokes.filter((i) => i.cmd === Commands.AgentCommits).length).toBe(1);
    store.applyScan("a", [], "h2"); // head moved → refresh
    expect(invokes.filter((i) => i.cmd === Commands.AgentCommits).length).toBe(2);
  });

  it("a late pull resolve cannot stomp fresher pushed data", async () => {
    store.loadChanges("a");
    const stale = resolvers.shift()!;
    store.applyScan("a", [{ path: "pushed", add: 1, del: 0, state: "A" as const }], "h1");
    stale([{ path: "stale", add: 9, del: 9, state: "M" as const }]);
    await Promise.resolve();
    expect(store.changesFor("a").data[0].path).toBe("pushed");
  });

  // ---- A0.6 LRU eviction: keep the last 4 agents touched ----

  it("evicts the least-recently-touched agent beyond the 4-agent cap", () => {
    const file: AgentFile = { path: "x", add: 1, del: 0, state: "M" };
    for (const id of ["a1", "a2", "a3", "a4"]) store.applyScan(id, [file], "h");
    expect(store.changesFor("a1").status).toBe("ready");

    store.applyScan("a5", [file], "h"); // 5th agent → a1 evicted
    expect(store.changesFor("a1").status).toBe("idle");
    for (const id of ["a2", "a3", "a4", "a5"]) {
      expect(store.changesFor(id).status).toBe("ready");
    }
  });

  it("touching an old agent protects it from eviction (true LRU, not FIFO)", () => {
    const file: AgentFile = { path: "x", add: 1, del: 0, state: "M" };
    for (const id of ["a1", "a2", "a3", "a4"]) store.applyScan(id, [file], "h");
    store.applyScan("a1", [file], "h"); // a1 becomes most recent
    store.applyScan("a5", [file], "h"); // now a2 is the oldest → evicted
    expect(store.changesFor("a1").status).toBe("ready");
    expect(store.changesFor("a2").status).toBe("idle");
  });

  // ---- anti-flash contract: identical scans re-render nothing ----

  it("an identical re-scan keeps the changes ENTRY reference (no signal write)", () => {
    store.applyScan("a", [file("x", 1, 2)], "h1", true);
    const before = store.changesFor("a");
    store.applyScan("a", [file("x", 1, 2)], "h1", true);
    expect(store.changesFor("a")).toBe(before);
  });

  it("every landed scan bumps scanSeq, identical or not — the diff refetch trigger", () => {
    expect(store.scanSeqFor("a")).toBe(0);
    store.applyScan("a", [file("x", 1, 2)], "h1", true);
    store.applyScan("a", [file("x", 1, 2)], "h1", true);
    expect(store.scanSeqFor("a")).toBe(2);
  });

  it("a real change replaces the entry as before", () => {
    store.applyScan("a", [file("x", 1, 2)], "h1", true);
    const before = store.changesFor("a");
    store.applyScan("a", [file("x", 3, 2)], "h1", true);
    expect(store.changesFor("a")).not.toBe(before);
    expect(store.changesFor("a").data[0].add).toBe(3);
  });

  it("a counts-only scan carries previous per-file ± forward (no zeroed chips)", () => {
    store.applyScan("a", [file("x", 5, 3)], "h1", true);
    const before = store.changesFor("a");
    // unchanged file set arriving counts-only adopts as a full no-op…
    store.applyScan("a", [file("x", 0, 0)], "h1", false);
    expect(store.changesFor("a")).toBe(before);
    // …and a NEW file still lands, with the old file's counts intact
    store.applyScan("a", [file("x", 0, 0), file("y", 0, 0)], "h1", false);
    const rows = store.changesFor("a").data;
    expect(rows.find((f) => f.path === "x")).toMatchObject({ add: 5, del: 3 });
    expect(rows.find((f) => f.path === "y")).toMatchObject({ add: 0, del: 0 });
  });

  it("an identical tree reload keeps the DATA reference (rows stay identical)", async () => {
    store.ensureTree("a");
    resolvers.shift()!([{ path: "src", name: "src", isDir: true }]);
    await Promise.resolve();
    const before = store.treeFor("a").data;
    store.loadTree("a"); // watcher path: forced reload (status does cycle)
    resolvers.shift()!([{ path: "src", name: "src", isDir: true }]);
    await Promise.resolve();
    expect(store.treeFor("a").data).toBe(before);
  });

  it("evicted entries reload lazily — eviction is free by design", async () => {
    const file: AgentFile = { path: "x", add: 1, del: 0, state: "M" };
    for (const id of ["a1", "a2", "a3", "a4", "a5"]) store.applyScan(id, [file], "h");
    expect(store.changesFor("a1").status).toBe("idle"); // evicted
    store.loadChanges("a1"); // reveal → normal lazy pull
    resolvers.shift()!([file]);
    await Promise.resolve();
    expect(store.changesFor("a1").status).toBe("ready");
  });
});
