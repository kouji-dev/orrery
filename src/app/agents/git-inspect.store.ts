import { inject, Injectable, signal, WritableSignal } from "@angular/core";
import { BRIDGE, Commands } from "../data-source/bridge";
import {
  BlameIntern,
  BlameLine,
  CommitFile,
  FileHistoryEntry,
  FileDiff,
  hydrateBlame,
  Loadable,
  RangeFiles,
} from "../models";

const IDLE_ARR = <T>(): Loadable<T[]> => ({ status: "idle", data: [] as T[] });
const IDLE_NULL = <T>(): Loadable<T | null> => ({ status: "idle", data: null });

/** Why 4: entries reload lazily by design — eviction is free (A0.6). Four
 *  covers the agents a user actively flips between; beyond that the keyed
 *  maps only grow for the process lifetime. */
const MAX_AGENTS = 4;

/**
 * Per-agent git inspection data: commit files, file diffs, range diffs, blame,
 * and file history — as keyed Loadable maps. Mirrors the AgentWorkStore pattern:
 * keyed maps, generation guards to supersede in-flight requests, and a dispose
 * method that drops all entries whose key starts with `id + '/'`.
 */
@Injectable({ providedIn: "root" })
export class GitInspectStore {
  private bridge = inject(BRIDGE);

  // ---- keyed Loadable maps ----
  private readonly commitFilesMap = signal<Record<string, Loadable<CommitFile[]>>>({});
  private readonly commitFileDiffMap = signal<Record<string, Loadable<FileDiff | null>>>({});
  private readonly rangeFilesMap = signal<Record<string, Loadable<RangeFiles | null>>>({});
  private readonly rangeFileDiffMap = signal<Record<string, Loadable<FileDiff | null>>>({});
  private readonly blameMap = signal<Record<string, Loadable<BlameLine[]>>>({});
  private readonly fileHistoryMap = signal<Record<string, Loadable<FileHistoryEntry[]>>>({});

  // ---- generation guards (supersede in-flight) ----
  private commitFilesGen: Record<string, number> = {};
  private commitFileDiffGen: Record<string, number> = {};
  private rangeFilesGen: Record<string, number> = {};
  private rangeFileDiffGen: Record<string, number> = {};
  private blameGen: Record<string, number> = {};
  private fileHistoryGen: Record<string, number> = {};

  // ---- LRU agent eviction (A0.6) ----
  /** Agent ids in touch order, most recent LAST. Loading data for a 5th agent
   *  disposes the least-recently-touched one's entries (they reload lazily). */
  private touched: string[] = [];

  private touch(id: string): void {
    const i = this.touched.indexOf(id);
    if (i >= 0) this.touched.splice(i, 1);
    this.touched.push(id);
    while (this.touched.length > MAX_AGENTS) {
      this.dispose(this.touched[0]); // dispose() also drops it from `touched`
    }
  }

  // =========================================================================
  // commit files (AgentCommitDiff)
  // =========================================================================

  commitFilesFor(id: string, sha: string): Loadable<CommitFile[]> {
    return this.commitFilesMap()[`${id}/${sha}`] ?? IDLE_ARR<CommitFile>();
  }

  loadCommitFiles(id: string, sha: string): void {
    this.touch(id);
    const key = `${id}/${sha}`;
    const gen = (this.commitFilesGen[key] ?? 0) + 1;
    this.commitFilesGen[key] = gen;
    const prev = this.commitFilesFor(id, sha);
    this.patch(this.commitFilesMap, key, { status: "loading", data: prev.data });
    void this.bridge
      .invoke<CommitFile[]>(Commands.AgentCommitDiff, { id, sha })
      .then((files) => {
        if (this.commitFilesGen[key] !== gen) return;
        this.patch(this.commitFilesMap, key, { status: "ready", data: files });
      })
      .catch(() => {
        if (this.commitFilesGen[key] !== gen) return;
        this.patch(this.commitFilesMap, key, { status: "error", data: prev.data });
      });
  }

  // =========================================================================
  // commit file diff (AgentCommitFileDiff)
  // =========================================================================

  commitFileDiffFor(id: string, sha: string, path: string): Loadable<FileDiff | null> {
    return this.commitFileDiffMap()[`${id}/${sha}/${path}`] ?? IDLE_NULL<FileDiff>();
  }

  loadCommitFileDiff(id: string, sha: string, path: string): void {
    this.touch(id);
    const key = `${id}/${sha}/${path}`;
    const gen = (this.commitFileDiffGen[key] ?? 0) + 1;
    this.commitFileDiffGen[key] = gen;
    const prev = this.commitFileDiffFor(id, sha, path);
    this.patch(this.commitFileDiffMap, key, { status: "loading", data: prev.data });
    void this.bridge
      .invoke<FileDiff | null>(Commands.AgentCommitFileDiff, { id, sha, path })
      .then((diff) => {
        if (this.commitFileDiffGen[key] !== gen) return;
        this.patch(this.commitFileDiffMap, key, { status: "ready", data: diff });
      })
      .catch(() => {
        if (this.commitFileDiffGen[key] !== gen) return;
        this.patch(this.commitFileDiffMap, key, { status: "error", data: prev.data });
      });
  }

  // =========================================================================
  // range files (AgentRangeFiles)
  // =========================================================================

  rangeFilesFor(id: string, shas: string[]): Loadable<RangeFiles | null> {
    return this.rangeFilesMap()[`${id}/${shas.join(",")}`] ?? IDLE_NULL<RangeFiles>();
  }

  loadRangeFiles(id: string, shas: string[]): void {
    this.touch(id);
    const key = `${id}/${shas.join(",")}`;
    const gen = (this.rangeFilesGen[key] ?? 0) + 1;
    this.rangeFilesGen[key] = gen;
    const prev = this.rangeFilesFor(id, shas);
    this.patch(this.rangeFilesMap, key, { status: "loading", data: prev.data });
    void this.bridge
      .invoke<RangeFiles | null>(Commands.AgentRangeFiles, { id, shas })
      .then((result) => {
        if (this.rangeFilesGen[key] !== gen) return;
        this.patch(this.rangeFilesMap, key, { status: "ready", data: result });
      })
      .catch(() => {
        if (this.rangeFilesGen[key] !== gen) return;
        this.patch(this.rangeFilesMap, key, { status: "error", data: prev.data });
      });
  }

  // =========================================================================
  // range file diff (AgentRangeFileDiff)
  // =========================================================================

  rangeFileDiffFor(id: string, from: string, to: string, path: string): Loadable<FileDiff | null> {
    return this.rangeFileDiffMap()[`${id}/${from}/${to}/${path}`] ?? IDLE_NULL<FileDiff>();
  }

  loadRangeFileDiff(id: string, from: string, to: string, path: string): void {
    this.touch(id);
    const key = `${id}/${from}/${to}/${path}`;
    const gen = (this.rangeFileDiffGen[key] ?? 0) + 1;
    this.rangeFileDiffGen[key] = gen;
    const prev = this.rangeFileDiffFor(id, from, to, path);
    this.patch(this.rangeFileDiffMap, key, { status: "loading", data: prev.data });
    void this.bridge
      .invoke<FileDiff | null>(Commands.AgentRangeFileDiff, { id, from, to, path })
      .then((diff) => {
        if (this.rangeFileDiffGen[key] !== gen) return;
        this.patch(this.rangeFileDiffMap, key, { status: "ready", data: diff });
      })
      .catch(() => {
        if (this.rangeFileDiffGen[key] !== gen) return;
        this.patch(this.rangeFileDiffMap, key, { status: "error", data: prev.data });
      });
  }

  // =========================================================================
  // blame (AgentBlame)
  // =========================================================================

  blameFor(id: string, path: string, rev?: string): Loadable<BlameLine[]> {
    return this.blameMap()[`${id}/${path}/${rev ?? ""}`] ?? IDLE_ARR<BlameLine>();
  }

  loadBlame(id: string, path: string, rev?: string): void {
    this.touch(id);
    // Key includes rev: annotate blames the OLD side (parent/from) and the NEW
    // side (commit/to) of the same file — different revs, must not collide.
    const key = `${id}/${path}/${rev ?? ""}`;
    const gen = (this.blameGen[key] ?? 0) + 1;
    this.blameGen[key] = gen;
    const prev = this.blameFor(id, path, rev);
    this.patch(this.blameMap, key, { status: "loading", data: prev.data });
    const payload: Record<string, unknown> = { id, path };
    if (rev !== undefined) payload["rev"] = rev;
    void this.bridge
      .invoke<BlameIntern>(Commands.AgentBlame, payload)
      .then((interned) => {
        if (this.blameGen[key] !== gen) return;
        // hydrate once at the boundary: rows share the interned commit strings
        this.patch(this.blameMap, key, { status: "ready", data: hydrateBlame(interned) });
      })
      .catch(() => {
        if (this.blameGen[key] !== gen) return;
        this.patch(this.blameMap, key, { status: "error", data: prev.data });
      });
  }

  // =========================================================================
  // file history (AgentFileHistory)
  // =========================================================================

  fileHistoryFor(id: string, path: string): Loadable<FileHistoryEntry[]> {
    return this.fileHistoryMap()[`${id}/${path}`] ?? IDLE_ARR<FileHistoryEntry>();
  }

  loadFileHistory(id: string, path: string): void {
    this.touch(id);
    const key = `${id}/${path}`;
    const gen = (this.fileHistoryGen[key] ?? 0) + 1;
    this.fileHistoryGen[key] = gen;
    const prev = this.fileHistoryFor(id, path);
    this.patch(this.fileHistoryMap, key, { status: "loading", data: prev.data });
    void this.bridge
      .invoke<FileHistoryEntry[]>(Commands.AgentFileHistory, { id, path })
      .then((entries) => {
        if (this.fileHistoryGen[key] !== gen) return;
        this.patch(this.fileHistoryMap, key, { status: "ready", data: entries });
      })
      .catch(() => {
        if (this.fileHistoryGen[key] !== gen) return;
        this.patch(this.fileHistoryMap, key, { status: "error", data: prev.data });
      });
  }

  // =========================================================================
  // dispose — drop all entries whose key starts with `id + '/'`
  // =========================================================================

  dispose(id: string): void {
    const t = this.touched.indexOf(id);
    if (t >= 0) this.touched.splice(t, 1);
    const prefix = `${id}/`;
    const dropKeys = (map: WritableSignal<Record<string, unknown>>) => {
      map.update((m) => {
        const toRemove = Object.keys(m).filter((k) => k.startsWith(prefix));
        if (!toRemove.length) return m;
        const next = { ...m };
        for (const k of toRemove) delete next[k];
        return next;
      });
    };
    for (const map of [
      this.commitFilesMap,
      this.commitFileDiffMap,
      this.rangeFilesMap,
      this.rangeFileDiffMap,
      this.blameMap,
      this.fileHistoryMap,
    ] as WritableSignal<Record<string, unknown>>[]) {
      dropKeys(map);
    }
    // clean up generation guards
    for (const gens of [
      this.commitFilesGen,
      this.commitFileDiffGen,
      this.rangeFilesGen,
      this.rangeFileDiffGen,
      this.blameGen,
      this.fileHistoryGen,
    ]) {
      for (const k of Object.keys(gens).filter((k) => k.startsWith(prefix))) {
        delete gens[k];
      }
    }
  }

  /** Single-key update — untouched keys keep their entry references. */
  private patch<T>(map: WritableSignal<Record<string, T>>, key: string, entry: T): void {
    map.update((m) => ({ ...m, [key]: entry }));
  }
}
