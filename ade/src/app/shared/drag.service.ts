import { Injectable, signal } from "@angular/core";

/**
 * What's currently being dragged across the shell. Set on dragstart (a sidebar
 * agent row, a rail popover row, a top-bar tab, or a file-tree row) and read on
 * drop (a pane, another tab, or the window-level file-drop router). A tab drag
 * also carries its first agent id so it can be dropped into a split pane like a
 * plain agent; a file drag (B1.5) carries the worktree-relative path so the
 * drop inserts the absolute path into a prompt / terminal.
 */
export interface DragPayload {
  kind: "agent" | "tab" | "file";
  agentId: string | null;
  tabId?: string;
  /** Worktree-relative path — set only for kind "file". */
  relPath?: string;
}

@Injectable({ providedIn: "root" })
export class DragService {
  readonly payload = signal<DragPayload | null>(null);

  start(p: DragPayload) {
    this.payload.set(p);
  }
  end() {
    this.payload.set(null);
  }
}
