import { inject, Injectable, signal } from "@angular/core";

import { EditsStore } from "../stores/edits.store";
import { UiStore } from "../ui/ui.store";
import { FileSaveService } from "./file-save.service";
import { treeAgentIds } from "./pane-model";
import { ScrollStateService } from "./scroll-state.service";

export interface PendingTabClose {
  tabId: string;
  /** Dirty buffers living in this tab's pane tree ("would be lost" set). */
  files: { agentId: string; path: string }[];
}

/**
 * Closing a WORKSPACE tab tears down its whole pane tree — file tabs and their
 * close-confirm never get a say. Every user-facing closeTab path routes here:
 * clean tabs close immediately, dirty ones raise a Save all / Discard / Cancel
 * dialog (rendered by the top bar, which owns the tab strip).
 */
@Injectable({ providedIn: "root" })
export class TabCloseGuardService {
  private readonly ui = inject(UiStore);
  private readonly edits = inject(EditsStore);
  private readonly saver = inject(FileSaveService);
  private readonly scroll = inject(ScrollStateService);

  readonly pending = signal<PendingTabClose | null>(null);

  /** Close `tabId`, or raise the unsaved-changes dialog when buffers are dirty. */
  requestClose(tabId: string): void {
    const root = this.ui.paneRoots()[tabId];
    const files = root
      ? treeAgentIds(root).flatMap((agentId) =>
          this.edits.dirtyPaths(agentId).map((path) => ({ agentId, path })),
        )
      : [];
    if (files.length === 0) {
      this.evictScroll(tabId);
      this.ui.closeTab(tabId);
      return;
    }
    this.pending.set({ tabId, files });
  }

  async saveAndClose(): Promise<void> {
    const p = this.pending();
    if (!p) return;
    const results = await Promise.all(p.files.map((f) => this.saver.save(f.agentId, f.path, false)));
    if (results.some((ok) => !ok)) {
      // save() flashed the failure — keep the tab (and the dialog closed) so
      // the user can deal with the failing file
      this.pending.set(null);
      return;
    }
    this.ui.flash(`Saved ${results.length === 1 ? "1 file" : `${results.length} files`}`);
    this.finish(p);
  }

  discardAndClose(): void {
    const p = this.pending();
    if (!p) return;
    for (const f of p.files) this.edits.close(f.agentId, f.path);
    this.finish(p);
  }

  cancel(): void {
    this.pending.set(null);
  }

  private finish(p: PendingTabClose): void {
    this.pending.set(null);
    this.evictScroll(p.tabId);
    this.ui.closeTab(p.tabId);
  }

  /** Closing the workspace tab closes its file tabs — drop their positions.
   *  Agents also present in another tab's pane tree keep theirs. */
  private evictScroll(tabId: string): void {
    const root = this.ui.paneRoots()[tabId];
    if (!root) return;
    const elsewhere = new Set(
      Object.entries(this.ui.paneRoots())
        .filter(([id]) => id !== tabId)
        .flatMap(([, r]) => treeAgentIds(r)),
    );
    for (const agentId of treeAgentIds(root)) {
      if (!elsewhere.has(agentId)) this.scroll.clearAgent(agentId);
    }
  }
}
