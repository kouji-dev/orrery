import { inject, Injectable, signal } from "@angular/core";
import { ConflictFile, Loadable } from "../models";
import { AgentsStore } from "../stores/agents.store";

/** One agent's in-progress merge session as held by the frontend. `files`
 *  keeps EVERY conflicted file of the session — including ones already
 *  resolved+staged (flagged `resolved: true`) — so the progress meter has a
 *  stable denominator even though resolved files leave the backend's conflict
 *  index. */
export interface ConflictSession {
  /** "ours" label — the agent's branch (HEAD of its worktree). */
  ours: string;
  /** "theirs" label — the branch that was merged in. */
  theirs: string;
  files: ConflictFile[];
}

const IDLE: Loadable<ConflictSession | null> = { status: "idle", data: null };

/**
 * Per-agent conflict-session state (A3.6 → B4.2): opened by a native merge
 * that hit conflicts, resolved file-by-file via `conflict_resolve`, finished
 * with merge_continue / merge_abort. Keyed Loadable map like AgentWorkStore so
 * one agent's session never re-renders another's consumers.
 */
@Injectable({ providedIn: "root" })
export class ConflictStore {
  private agents = inject(AgentsStore);

  private readonly sessions = signal<Record<string, Loadable<ConflictSession | null>>>({});
  private gens: Record<string, number> = {};

  sessionFor(id: string): Loadable<ConflictSession | null> {
    return this.sessions()[id] ?? IDLE;
  }

  /** Adopt the session returned by a conflicting native merge. */
  open(id: string, ours: string, theirs: string, files: ConflictFile[]): void {
    this.gens[id] = (this.gens[id] ?? 0) + 1;
    this.patch(id, { status: "ready", data: { ours, theirs, files } });
  }

  /** Re-read the backend's conflict list (e.g. after an app reload found a
   *  session in progress via `session_state`). Files already resolved before
   *  the reload are gone from the index, so the denominator restarts at the
   *  remaining set — acceptable for the recovery path. */
  load(id: string, ours: string, theirs: string): void {
    const gen = (this.gens[id] = (this.gens[id] ?? 0) + 1);
    const prev = this.sessionFor(id);
    this.patch(id, { status: "loading", data: prev.data });
    void this.agents
      .conflicts(id)
      .then((files) => {
        if (this.gens[id] !== gen) return;
        this.patch(id, { status: "ready", data: { ours, theirs, files } });
      })
      .catch(() => {
        if (this.gens[id] !== gen) return;
        this.patch(id, { status: "error", data: prev.data });
      });
  }

  /** Write `content` as the resolution of `path`, stage it, and flip the
   *  file's `resolved` flag locally (the backend listing forgets it). */
  async resolve(id: string, path: string, content: string): Promise<void> {
    await this.agents.conflictResolve(id, path, content);
    const cur = this.sessionFor(id);
    if (!cur.data) return;
    this.patch(id, {
      status: "ready",
      data: {
        ...cur.data,
        files: cur.data.files.map((f) => (f.path === path ? { ...f, resolved: true } : f)),
      },
    });
  }

  /** Commit the fully-resolved merge; clears the session on success. */
  async commit(id: string, message?: string): Promise<string> {
    const sha = await this.agents.mergeContinue(id, message);
    this.clear(id);
    return sha;
  }

  /** Abort the merge; clears the session on success. */
  async abort(id: string): Promise<void> {
    await this.agents.mergeAbort(id);
    this.clear(id);
  }

  clear(id: string): void {
    this.gens[id] = (this.gens[id] ?? 0) + 1;
    this.patch(id, IDLE);
  }

  /** Drop an agent's entry entirely (on removal). */
  dispose(id: string): void {
    this.sessions.update((m) => {
      if (!(id in m)) return m;
      const { [id]: _drop, ...rest } = m;
      return rest;
    });
    delete this.gens[id];
  }

  private patch(id: string, entry: Loadable<ConflictSession | null>): void {
    this.sessions.update((m) => ({ ...m, [id]: entry }));
  }
}
