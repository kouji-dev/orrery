import { effect, inject, Injectable, signal } from "@angular/core";

import { BRIDGE, Commands } from "../data-source/bridge";
import { SettingsStore } from "../settings/settings.store";
import { EditsStore } from "../stores/edits.store";
import { UiStore } from "../ui/ui.store";
import { fileName } from "../utils";

/** Why 2s: long enough that mid-word saves are rare, short enough that an agent
 *  (or a crash) sees recent work — the roadmap's B1.2 "after N seconds idle". */
export const AUTOSAVE_DELAY_MS = 2000;

/**
 * Writes a dirty EditsStore buffer through to the agent's worktree (B1.1,
 * `file_write`). One save path for Ctrl+S, the close-confirm dialog, and any
 * future save-all: write → mark the buffer saved (which is what makes the
 * watcher's follow-up rescan read as an echo, not an external change).
 */
@Injectable({ providedIn: "root" })
export class FileSaveService {
  private readonly bridge = inject(BRIDGE);
  private readonly edits = inject(EditsStore);
  private readonly ui = inject(UiStore);
  private readonly settings = inject(SettingsStore);

  /** Keys ("agentId:path") with a write in flight — guards double Ctrl+S. */
  readonly saving = signal<ReadonlySet<string>>(new Set());

  private autosaveTimer: ReturnType<typeof setTimeout> | null = null;

  constructor() {
    // B1.2 autosave (opt-in setting): every keystroke re-arms a trailing
    // debounce; on fire, quietly write every dirty buffer (Save All, no flash).
    effect(() => {
      const tick = this.edits.editTick();
      const on = this.settings.settings().autosave;
      if (this.autosaveTimer !== null) {
        clearTimeout(this.autosaveTimer);
        this.autosaveTimer = null;
      }
      if (!on || tick === 0 || this.edits.dirtyKeys().size === 0) return;
      this.autosaveTimer = setTimeout(() => {
        this.autosaveTimer = null;
        void this.saveAll(false);
      }, AUTOSAVE_DELAY_MS);
    });
  }

  /** Save if dirty; resolves true when the buffer is clean on disk afterwards.
   *  `announce: false` suppresses the success flash (bulk paths flash once). */
  async save(agentId: string, path: string, announce = true): Promise<boolean> {
    const buf = this.edits.get(agentId, path);
    if (!buf) return true;
    if (!buf.dirty) return true;
    const key = `${agentId}:${path}`;
    if (this.saving().has(key)) return false;
    this.setSaving(key, true);
    try {
      await this.bridge.invoke(Commands.FileWrite, { id: agentId, path, content: buf.text });
      this.edits.saved(agentId, path);
      if (announce) this.ui.flash(`Saved ${fileName(path)}`);
      return true;
    } catch (e) {
      this.ui.flash(`Save failed: ${e instanceof Error ? e.message : e}`);
      return false;
    } finally {
      this.setSaving(key, false);
    }
  }

  /** Ctrl+S semantics (JetBrains Save All): write EVERY dirty buffer, not just
   *  the focused file. Clean workspace = silent no-op. `announce: false` is the
   *  autosave path — no success flashes at all (a toast every 2s would be noise). */
  async saveAll(announce = true): Promise<boolean> {
    const keys = [...this.edits.dirtyKeys()];
    if (keys.length === 0) return true;
    const files = keys.map((key) => {
      const i = key.indexOf(":");
      return { agentId: key.slice(0, i), path: key.slice(i + 1) };
    });
    if (files.length === 1) return this.save(files[0].agentId, files[0].path, announce);
    const results = await Promise.all(files.map((f) => this.save(f.agentId, f.path, false)));
    const failed = results.filter((r) => !r).length;
    // failures already flashed individually; summarize success here
    if (!failed && announce) this.ui.flash(`Saved ${results.length} files`);
    return failed === 0;
  }

  private setSaving(key: string, on: boolean): void {
    this.saving.update((s) => {
      const next = new Set(s);
      if (on) next.add(key);
      else next.delete(key);
      return next;
    });
  }
}
