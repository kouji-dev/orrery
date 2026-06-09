import { inject, Injectable, signal, WritableSignal } from "@angular/core";
import { BRIDGE, Commands } from "../data-source/bridge";
import { AgentFile, Commit, FileNode, Loadable } from "../models";

/** Commits page size for the lazy feed (+ "Load more"). */
export const COMMITS_PAGE = 10;

const IDLE: Loadable<never[]> = { status: "idle", data: [] };
const IDLE_COMMITS = { ...IDLE, hasMore: false };

export type CommitsEntry = Loadable<Commit[]> & { hasMore: boolean };

/**
 * Per-agent worktree data (git status / branch commits / file tree) as keyed
 * Loadable maps, SEPARATE from the Agent records so one agent's reload never
 * re-renders the others' consumers (entry reference identity is preserved for
 * untouched ids). `idle` means "never requested" — unknown, not empty. Lazy:
 * changes load eagerly per agent at startup; tree/commits on first agent open.
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

  /** Watcher event: this agent's worktree changed. Changes always reload
   *  (eager data feeding the always-visible badges); tree only if loaded. */
  onWorktreeChanged(id: string): void {
    this.loadChanges(id);
    if (this.treeFor(id).status !== "idle") this.loadTree(id);
  }

  /** Drop all of an agent's entries (on removal). */
  dispose(id: string): void {
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
  }

  /** Single-key update — untouched ids keep their entry references. */
  private patch<T>(map: WritableSignal<Record<string, T>>, id: string, entry: T): void {
    map.update((m) => ({ ...m, [id]: entry }));
  }
}
