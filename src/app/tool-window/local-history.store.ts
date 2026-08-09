import { inject, Injectable, signal } from "@angular/core";

import { BRIDGE, Commands, HistorySnapshot } from "../data-source/bridge";
import { FileDiff } from "../models";
import { UiStore } from "../ui/ui.store";

/**
 * Local-history panel state (B4.4): the scoped agent's snapshot timeline plus
 * the restore / diff operations. Snapshots are captured backend-side (watcher
 * bursts + restore guards) — this store only reads and restores.
 */
@Injectable({ providedIn: "root" })
export class LocalHistoryStore {
  private bridge = inject(BRIDGE);
  private ui = inject(UiStore);

  readonly snapshots = signal<HistorySnapshot[]>([]);
  readonly busy = signal(false);
  readonly loadedFor = signal<string | null>(null);

  async load(agentId: string): Promise<void> {
    this.busy.set(true);
    try {
      const list = await this.bridge.invoke<HistorySnapshot[]>(Commands.HistoryList, {
        id: agentId,
      });
      this.snapshots.set(list);
      this.loadedFor.set(agentId);
    } catch {
      this.snapshots.set([]);
      this.loadedFor.set(null);
    } finally {
      this.busy.set(false);
    }
  }

  /** Snapshot vs current content of one file — rendered by app-code-diff. */
  fileDiff(agentId: string, snapId: string, path: string): Promise<FileDiff> {
    return this.bridge.invoke<FileDiff>(Commands.HistoryFile, {
      id: agentId,
      snap: snapId,
      path,
    });
  }

  /** Restore files (undefined = the whole snapshot); resolves restored paths. */
  async restore(agentId: string, snapId: string, paths?: string[]): Promise<boolean> {
    if (this.busy()) return false;
    this.busy.set(true);
    try {
      const restored = await this.bridge.invoke<string[]>(Commands.HistoryRestore, {
        id: agentId,
        snap: snapId,
        paths: paths ?? null,
      });
      this.ui.flash(`Restored ${restored.length} file${restored.length === 1 ? "" : "s"}`);
      return true;
    } catch (e) {
      this.ui.flash(e instanceof Error ? e.message : String(e));
      return false;
    } finally {
      this.busy.set(false);
      void this.load(agentId);
    }
  }
}
