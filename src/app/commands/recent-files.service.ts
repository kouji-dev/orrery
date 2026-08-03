import { effect, inject, Injectable, signal } from "@angular/core";
import { UiStore } from "../ui/ui.store";
import { PaneNode } from "../workspace/pane-model";

export interface RecentFileEntry {
  agentId: string;
  path: string;
  at: number;
}

const KEY = "orrery.recentFiles";
const CAP = 50;

function load(): RecentFileEntry[] {
  try {
    const raw = JSON.parse(localStorage.getItem(KEY) || "null");
    return Array.isArray(raw) ? raw.filter((e) => e && e.agentId && e.path) : [];
  } catch {
    return [];
  }
}

/**
 * Recent-files history (roadmap B2.3, Ctrl+E). Records every file the user
 * views in a workspace pane. Tracking is PASSIVE: an effect watches the pane
 * trees' `activeFile` leaves and records transitions, so every open path
 * (file tree, search result, palette) is captured without touching the
 * shared UiStore / pane-node code. Persisted across sessions (localStorage).
 */
@Injectable({ providedIn: "root" })
export class RecentFilesService {
  private ui = inject(UiStore);
  readonly entries = signal<RecentFileEntry[]>(load());

  /** Last observed active file per pane leaf id — transition detector. */
  private lastActive = new Map<string, string>();

  constructor() {
    effect(() => {
      const roots = this.ui.paneRoots();
      const seen = new Set<string>();
      for (const root of Object.values(roots)) this.walk(root, seen);
      // drop tracking for leaves that no longer exist
      for (const k of [...this.lastActive.keys()]) {
        if (!seen.has(k)) this.lastActive.delete(k);
      }
    });
  }

  private walk(node: PaneNode, seen: Set<string>): void {
    if (node.type === "leaf") {
      seen.add(node.id);
      const file = node.view === "file" ? (node.activeFile ?? null) : null;
      if (file && node.agentId && this.lastActive.get(node.id) !== file) {
        this.record(node.agentId, file);
      }
      this.lastActive.set(node.id, file ?? "");
      return;
    }
    this.walk(node.a, seen);
    this.walk(node.b, seen);
  }

  /** Move (agentId, path) to the top of the list. */
  record(agentId: string, path: string): void {
    this.entries.update((prev) => {
      const rest = prev.filter((e) => !(e.agentId === agentId && e.path === path));
      const next = [{ agentId, path, at: Date.now() }, ...rest].slice(0, CAP);
      try {
        localStorage.setItem(KEY, JSON.stringify(next));
      } catch {
        /* storage unavailable — history is session-only then */
      }
      return next;
    });
  }

  /** Drop entries for a removed agent (worktree deleted). */
  clearAgent(agentId: string): void {
    this.entries.update((prev) => prev.filter((e) => e.agentId !== agentId));
  }
}
