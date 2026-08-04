import { inject, Injectable, signal, WritableSignal } from "@angular/core";
import { BRIDGE, Commands } from "../data-source/bridge";
import { AgentFile, Commit, FileNode, Loadable } from "../models";

/** Commits page size for the lazy feed (+ "Load more"). */
export const COMMITS_PAGE = 10;

const IDLE: Loadable<never[]> = { status: "idle", data: [] };
const IDLE_COMMITS = { ...IDLE, hasMore: false };

/** Why 4: entries reload lazily by design — eviction is free (A0.6). Four
 *  covers the agents a user actively flips between; beyond that the keyed
 *  maps only grow for the process lifetime. */
const MAX_AGENTS = 4;

export type CommitsEntry = Loadable<Commit[]> & { hasMore: boolean };

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
        this.patch(this.changesMap, id, { status: "ready", data: files });
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
    this.patch(this.treesMap, id, { status: "loading", data: prev.data });
    void this.bridge
      .invoke<FileNode[]>(Commands.AgentTree, { id })
      .then((nodes) => {
        if (this.treesGen[id] !== gen) return;
        this.patch(this.treesMap, id, { status: "ready", data: nodes });
      })
      .catch(() => {
        if (this.treesGen[id] !== gen) return;
        this.patch(this.treesMap, id, { status: "error", data: prev.data });
      });
  }
  /** Lazily expand one unloaded dir: splice its children into the loaded tree. */
  expandDir(id: string, path: string): void {
    void this.bridge.invoke<FileNode[]>(Commands.AgentDir, { id, path }).then((kids) => {
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
   *  previously loaded. Replaces the old ping → pull (`onWorktreeChanged`). */
  applyScan(id: string, changes: AgentFile[], head: string | null): void {
    this.touch(id);
    // supersede any in-flight pull so its late resolve can't stomp fresher push data
    this.changesGen[id] = (this.changesGen[id] ?? 0) + 1;
    this.patch(this.changesMap, id, { status: "ready", data: changes });
    const moved = id in this.lastHead && this.lastHead[id] !== head;
    this.lastHead[id] = head;
    if (moved) this.refreshCommits(id);
    if (this.treeFor(id).status !== "idle") this.loadTree(id);
  }

  /** Drop all of an agent's entries (on removal or LRU eviction). */
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
}
