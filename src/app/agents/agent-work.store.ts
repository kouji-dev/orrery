import { inject, Injectable, signal, WritableSignal } from "@angular/core";
import { AgentChangeTotals, BRIDGE, Commands } from "../data-source/bridge";
import { AgentFile, Commit, FileNode, Loadable } from "../models";

/** Commits page size for the lazy feed (+ "Load more"). */
export const COMMITS_PAGE = 10;

const IDLE: Loadable<never[]> = { status: "idle", data: [] };
const IDLE_COMMITS = { ...IDLE, hasMore: false };

/** Structural equality via JSON — rows are plain backend DTOs (stable key
 *  order from serde), and even the file TREE stringifies in well under a
 *  millisecond at sidebar scale. */
function sameJson(a: unknown, b: unknown): boolean {
  return JSON.stringify(a) === JSON.stringify(b);
}

/** Why 4: entries reload lazily by design — eviction is free (A0.6). Four
 *  covers the agents a user actively flips between; beyond that the keyed
 *  maps only grow for the process lifetime. */
const MAX_AGENTS = 4;

export type CommitsEntry = Loadable<Commit[]> & { hasMore: boolean };

/** Tree-root key: a plain agent id, or `proj:<projectId>` for a repo's main
 *  worktree (the sidebar files section's root chip can point at either). */
export function projectRootKey(projectId: string): string {
  return `proj:${projectId}`;
}
function rootOf(key: string): { cmdTree: string; cmdDir: string; id: string } {
  return key.startsWith("proj:")
    ? { cmdTree: Commands.ProjectTree, cmdDir: Commands.ProjectDir, id: key.slice(5) }
    : { cmdTree: Commands.AgentTree, cmdDir: Commands.AgentDir, id: key };
}

/**
 * Per-agent worktree data (git status / branch commits / file tree) as keyed
 * Loadable maps, SEPARATE from the Agent records so one agent's reload never
 * re-renders the others' consumers (entry reference identity is preserved for
 * untouched ids). `idle` means "never requested" — unknown, not empty.
 * Changes arrive as backend watcher pushes (applyScan); tree/commits stay lazy — on first agent open.
 */
@Injectable({ providedIn: "root" })
export class AgentWorkStore {
  private bridge = inject(BRIDGE);

  private readonly changesMap = signal<Record<string, Loadable<AgentFile[]>>>({});
  private readonly commitsMap = signal<Record<string, CommitsEntry>>({});
  private readonly treesMap = signal<Record<string, Loadable<FileNode[]>>>({});
  /** Monotonic per-agent scan counter — bumps on every landed scan, EVEN when
   *  the file list was structurally identical (content may still differ, e.g.
   *  an edit that keeps ± counts). The diff view keys its silent refetch on
   *  this instead of file-object identity, so a no-op scan re-renders nothing. */
  private readonly scanSeqMap = signal<Record<string, number>>({});
  /** Sidebar counters: 3 numbers per agent, SEPARATE from the LRU'd file lists
   *  above — every agent row shows counters, so eviction must never touch them. */
  private readonly totalsMap = signal<Record<string, { add: number; del: number; files: number }>>({});
  private totalsInit = false;

  // generation guards: a newer load supersedes an in-flight older one
  private changesGen: Record<string, number> = {};
  private commitsGen: Record<string, number> = {};
  private treesGen: Record<string, number> = {};
  // last pushed HEAD oid per agent — commits refresh only when it moves
  private lastHead: Record<string, string | null> = {};

  // ---- LRU agent eviction (A0.6) ----
  /** Agent ids in touch order, most recent LAST. Data landing for a 5th agent
   *  disposes the least-recently-touched one's entries (they reload lazily —
   *  a watcher push or reopen repopulates them). */
  private touched: string[] = [];

  private touch(id: string): void {
    const i = this.touched.indexOf(id);
    if (i >= 0) this.touched.splice(i, 1);
    this.touched.push(id);
    while (this.touched.length > MAX_AGENTS) {
      this.dispose(this.touched[0]); // dispose() also drops it from `touched`
    }
  }

  changesFor(id: string): Loadable<AgentFile[]> {
    return this.changesMap()[id] ?? IDLE;
  }
  /** See scanSeqMap — the diff view's refetch trigger. */
  scanSeqFor(id: string): number {
    return this.scanSeqMap()[id] ?? 0;
  }
  /** Sidebar counters — null until the init pass (or a full scan) supplied them. */
  totalsFor(id: string): { add: number; del: number; files: number } | null {
    return this.totalsMap()[id] ?? null;
  }

  /** One initialization pass: full change totals for EVERY agent. After this,
   *  only full-detail watcher scans (running/visible agents) update the map —
   *  idle projects push nothing further. Safe to call more than once. */
  initTotals(): void {
    if (this.totalsInit) return;
    this.totalsInit = true;
    void this.bridge
      .invoke<AgentChangeTotals[]>(Commands.AgentChangeTotals)
      .then((all) => {
        this.totalsMap.update((m) => {
          const next = { ...m };
          // scans may have landed while the init call ran — they win
          for (const t of all) next[t.id] ??= { add: t.add, del: t.del, files: t.files };
          return next;
        });
      })
      .catch(() => {
        this.totalsInit = false; // backend unavailable — allow a retry
      });
  }
  commitsFor(id: string): CommitsEntry {
    return this.commitsMap()[id] ?? IDLE_COMMITS;
  }
  treeFor(id: string): Loadable<FileNode[]> {
    return this.treesMap()[id] ?? IDLE;
  }

  // ---- changes (eager per agent; reloaded on watcher events) ----
  loadChanges(id: string): void {
    this.touch(id);
    const gen = (this.changesGen[id] ?? 0) + 1;
    this.changesGen[id] = gen;
    const prev = this.changesFor(id);
    this.patch(this.changesMap, id, { status: "loading", data: prev.data });
    void this.bridge
      .invoke<AgentFile[]>(Commands.AgentChanges, { id })
      .then((files) => {
        if (this.changesGen[id] !== gen) return;
        this.adoptReady(this.changesMap, id, files);
        this.bumpScan(id);
      })
      .catch(() => {
        if (this.changesGen[id] !== gen) return;
        this.patch(this.changesMap, id, { status: "error", data: prev.data });
      });
  }

  // ---- commits (lazy; paged; stale-while-revalidate) ----
  ensureCommits(id: string): void {
    if (this.commitsFor(id).status !== "idle") return;
    this.loadCommitsPage(id, COMMITS_PAGE, 0, []);
  }
  loadMoreCommits(id: string): void {
    const cur = this.commitsFor(id);
    if (cur.status === "loading" || !cur.hasMore) return;
    this.loadCommitsPage(id, COMMITS_PAGE, cur.data.length, cur.data);
  }
  /** Reload from the top (after a commit action) keeping current rows visible. */
  refreshCommits(id: string): void {
    const cur = this.commitsFor(id);
    if (cur.status === "idle") return; // never opened — stay lazy
    this.loadCommitsPage(id, Math.max(COMMITS_PAGE, cur.data.length), 0, cur.data);
  }
  private loadCommitsPage(id: string, limit: number, offset: number, keep: Commit[]): void {
    this.touch(id);
    const gen = (this.commitsGen[id] ?? 0) + 1;
    this.commitsGen[id] = gen;
    this.patch(this.commitsMap, id, {
      status: "loading",
      data: keep,
      hasMore: this.commitsFor(id).hasMore,
    });
    void this.bridge
      .invoke<Commit[]>(Commands.AgentCommits, { id, limit, offset })
      .then((page) => {
        if (this.commitsGen[id] !== gen) return;
        const data = offset === 0 ? page : [...keep, ...page];
        this.patch(this.commitsMap, id, {
          status: "ready",
          data,
          hasMore: page.length === limit,
        });
      })
      .catch(() => {
        if (this.commitsGen[id] !== gen) return;
        this.patch(this.commitsMap, id, { status: "error", data: keep, hasMore: false });
      });
  }

  // ---- file tree (lazy; reloaded on watcher events once loaded) ----
  // Keyed by ROOT key: a plain agent id, or `proj:<id>` (see projectRootKey) —
  // the same Loadable map / LRU / generation guards serve both root kinds.
  ensureTree(id: string): void {
    if (this.treeFor(id).status !== "idle") return;
    this.loadTree(id);
  }
  /** Forced reload (file-tree refresh button; watcher path once loaded). */
  loadTree(id: string): void {
    this.touch(id);
    const gen = (this.treesGen[id] ?? 0) + 1;
    this.treesGen[id] = gen;
    const prev = this.treeFor(id);
    const root = rootOf(id);
    this.patch(this.treesMap, id, { status: "loading", data: prev.data });
    void this.bridge
      .invoke<FileNode[]>(root.cmdTree, { id: root.id })
      .then((nodes) => {
        if (this.treesGen[id] !== gen) return;
        this.adoptReady(this.treesMap, id, nodes);
      })
      .catch(() => {
        if (this.treesGen[id] !== gen) return;
        this.patch(this.treesMap, id, { status: "error", data: prev.data });
      });
  }
  /** Lazily expand one unloaded dir: splice its children into the loaded tree. */
  expandDir(id: string, path: string): void {
    const root = rootOf(id);
    void this.bridge.invoke<FileNode[]>(root.cmdDir, { id: root.id, path }).then((kids) => {
      const cur = this.treeFor(id);
      if (cur.status === "idle") return;
      const splice = (list: FileNode[]): FileNode[] =>
        list.map((n) => {
          if (n.path === path) return { ...n, children: kids };
          if (n.children) return { ...n, children: splice(n.children) };
          return n;
        });
      this.patch(this.treesMap, id, { ...cur, data: splice(cur.data) });
    });
  }

  /** Backend watcher push: adopt the scanned changes; commits refresh only when
   *  HEAD actually moved (and the feed was ever opened); tree reloads only when
   *  previously loaded. Replaces the old ping → pull (`onWorktreeChanged`).
   *  `countsFull=false` = an A2.2 counts-only scan whose add/del are all 0 —
   *  it updates the file COUNT but must not zero the sidebar's line totals. */
  applyScan(id: string, changes: AgentFile[], head: string | null, countsFull = true): void {
    this.touch(id);
    // supersede any in-flight pull so its late resolve can't stomp fresher push data
    this.changesGen[id] = (this.changesGen[id] ?? 0) + 1;
    // counts-only scans zero add/del — showing that would blank every ± chip
    // in the diff list until the next full push, so carry the previous counts
    // forward per path (an unchanged file SET then adopts as a full no-op).
    let adopted = changes;
    if (!countsFull) {
      const byPath = new Map(this.changesFor(id).data.map((f) => [f.path, f]));
      adopted = changes.map((f) => {
        const p = byPath.get(f.path);
        return p && f.add === 0 && f.del === 0 ? { ...f, add: p.add, del: p.del } : f;
      });
    }
    this.adoptReady(this.changesMap, id, adopted);
    this.bumpScan(id);
    const cur = this.totalsMap()[id];
    const next = countsFull
      ? {
          add: changes.reduce((s, f) => s + f.add, 0),
          del: changes.reduce((s, f) => s + f.del, 0),
          files: changes.length,
        }
      : { add: cur?.add ?? 0, del: cur?.del ?? 0, files: changes.length };
    if (!cur || cur.add !== next.add || cur.del !== next.del || cur.files !== next.files) {
      this.patch(this.totalsMap, id, next);
    }
    const moved = id in this.lastHead && this.lastHead[id] !== head;
    this.lastHead[id] = head;
    if (moved) this.refreshCommits(id);
    if (this.treeFor(id).status !== "idle") this.loadTree(id);
  }

  /** Agent REMOVED (not merely LRU-evicted): its sidebar counters go too. */
  dropTotals(id: string): void {
    this.totalsMap.update((m) => {
      if (!(id in m)) return m;
      const { [id]: _drop, ...rest } = m;
      return rest;
    });
  }

  /** Drop an agent's heavy entries (on removal or LRU eviction). Totals stay —
   *  the sidebar shows counters for EVERY agent, eviction must not blank them. */
  dispose(id: string): void {
    const t = this.touched.indexOf(id);
    if (t >= 0) this.touched.splice(t, 1);
    for (const map of [this.changesMap, this.commitsMap, this.treesMap]) {
      (map as WritableSignal<Record<string, unknown>>).update((m) => {
        if (!(id in m)) return m;
        const { [id]: _drop, ...rest } = m;
        return rest;
      });
    }
    delete this.changesGen[id];
    delete this.commitsGen[id];
    delete this.treesGen[id];
    delete this.lastHead[id];
  }

  /** Single-key update — untouched ids keep their entry references. */
  private patch<T>(map: WritableSignal<Record<string, T>>, id: string, entry: T): void {
    map.update((m) => ({ ...m, [id]: entry }));
  }

  /** Anti-flash adopt: when the fresh rows structurally equal the current
   *  entry's, keep the ENTRY reference untouched (no signal write at all) —
   *  watcher re-scans that changed nothing then re-render nothing. Only a
   *  real difference (or a status transition) replaces the entry. */
  private adoptReady<T>(
    map: WritableSignal<Record<string, Loadable<T[]>>>,
    id: string,
    data: T[],
  ): void {
    const prev = map()[id];
    if (prev && prev.status === "ready" && sameJson(prev.data, data)) return;
    const keep = prev && sameJson(prev.data, data) ? prev.data : data;
    this.patch(map, id, { status: "ready", data: keep });
  }

  private bumpScan(id: string): void {
    this.patch(this.scanSeqMap, id, (this.scanSeqMap()[id] ?? 0) + 1);
  }
}
