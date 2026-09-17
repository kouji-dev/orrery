import { Injectable, computed, signal } from "@angular/core";

/**
 * Unsaved-edit buffers for the writable editor (B1.1). The store — not the
 * Monaco instance — is the source of truth for buffer text: the editor cap
 * (`editor-cap.ts`) demotes the oldest live editor to plain text and disposes
 * its model, so an unsaved buffer must live where that cannot destroy it.
 *
 * Echo-tolerance contract with the FS watcher: `saved()` moves `baseText` to
 * the just-written text, so the `agent://changed` burst a save triggers reads
 * disk == baseText and treats it as a no-op instead of an external change.
 */
export interface EditBuffer {
  /** Last content known to be on disk (at open, reload, or successful save). */
  baseText: string;
  /** Current buffer content (== editor content while an editor is live). */
  text: string;
  dirty: boolean;
}

const k = (agentId: string, path: string): string => `${agentId}:${path}`;

@Injectable({ providedIn: "root" })
export class EditsStore {
  private readonly state = signal<Record<string, EditBuffer>>({});

  /** Set of dirty buffer keys — drives tab dots / close guards reactively. */
  readonly dirtyKeys = computed<ReadonlySet<string>>(() => {
    const set = new Set<string>();
    for (const [key, b] of Object.entries(this.state())) if (b.dirty) set.add(key);
    return set;
  });

  get(agentId: string, path: string): EditBuffer | undefined {
    return this.state()[k(agentId, path)];
  }

  isDirty(agentId: string, path: string): boolean {
    return this.dirtyKeys().has(k(agentId, path));
  }

  dirtyPaths(agentId: string): string[] {
    const prefix = `${agentId}:`;
    return [...this.dirtyKeys()]
      .filter((key) => key.startsWith(prefix))
      .map((key) => key.slice(prefix.length));
  }

  /**
   * Sync a buffer with freshly read disk content. A clean (or absent) buffer
   * adopts the disk text; a dirty buffer is left untouched — the caller
   * compares `diskText` against `baseText` to decide whether to show the
   * external-change conflict banner. Returns the resulting buffer.
   */
  open(agentId: string, path: string, diskText: string): EditBuffer {
    const key = k(agentId, path);
    const cur = this.state()[key];
    if (cur?.dirty) return cur;
    // Idempotent when nothing changed — callers run inside effects, and an
    // unconditional write here makes such an effect retrigger itself forever.
    if (cur && cur.baseText === diskText && cur.text === diskText) return cur;
    const next: EditBuffer = { baseText: diskText, text: diskText, dirty: false };
    this.state.update((m) => ({ ...m, [key]: next }));
    return next;
  }

  /** Monotonic tick, bumped on every real keystroke-path change — the autosave
   *  debounce keys off this (dirtyKeys alone doesn't move while typing). */
  readonly editTick = signal(0);

  /** Editor keystroke path: update the buffer text, derive dirtiness. */
  update(agentId: string, path: string, text: string): void {
    const key = k(agentId, path);
    let changed = false;
    this.state.update((m) => {
      const cur = m[key];
      if (!cur) return m;
      if (cur.text === text) return m;
      changed = true;
      return { ...m, [key]: { ...cur, text, dirty: text !== cur.baseText } };
    });
    if (changed) this.editTick.update((n) => n + 1);
  }

  /** A save was written: the buffer text is now the disk text (echo-tolerance). */
  saved(agentId: string, path: string): void {
    const key = k(agentId, path);
    this.state.update((m) => {
      const cur = m[key];
      if (!cur) return m;
      return { ...m, [key]: { baseText: cur.text, text: cur.text, dirty: false } };
    });
  }

  /** Drop unsaved edits and adopt `diskText` (Reload / Discard actions). */
  discard(agentId: string, path: string, diskText: string): void {
    const key = k(agentId, path);
    this.state.update((m) => ({
      ...m,
      [key]: { baseText: diskText, text: diskText, dirty: false },
    }));
  }

  /** Buffer lifecycle ends with the tab (after any close-confirm has run). */
  close(agentId: string, path: string): void {
    const key = k(agentId, path);
    this.state.update((m) => {
      if (!(key in m)) return m;
      const { [key]: _, ...rest } = m;
      return rest;
    });
  }

  /** Drop every buffer of an agent (agent removed / worktree gone). */
  closeAgent(agentId: string): void {
    const prefix = `${agentId}:`;
    this.state.update((m) => {
      const rest = Object.fromEntries(
        Object.entries(m).filter(([key]) => !key.startsWith(prefix)),
      );
      return Object.keys(rest).length === Object.keys(m).length ? m : rest;
    });
  }
}
