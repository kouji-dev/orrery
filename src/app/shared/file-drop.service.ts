import { inject, Injectable, NgZone } from "@angular/core";
import { AgentsStore } from "../stores/agents.store";
import { UiStore } from "../ui/ui.store";

// Bracketed paste so the path lands as literal text (no keybinding interpretation)
// in the Claude prompt, mirroring AgentReviewService.
const PASTE_START = "\x1b[200~";
const PASTE_END = "\x1b[201~";

function inTauri(): boolean {
  return typeof window !== "undefined" && "__TAURI_INTERNALS__" in window;
}

/** Quote a path only when it contains whitespace, so Claude reads it as one token. */
function quotePath(p: string): string {
  return /\s/.test(p) ? `"${p}"` : p;
}

/**
 * OS file/image drag-and-drop → the focused agent's terminal. Dropping files
 * anywhere in the window inserts their absolute path(s) into the agent's Claude
 * prompt (as a bracketed paste, NO trailing newline — the user still edits/sends).
 * Images work identically: Claude reads the path.
 *
 * Requires `dragDropEnabled: true` in tauri.conf.json so Tauri emits the drag-drop
 * event WITH absolute paths (the browser File API never exposes real paths).
 */
@Injectable({ providedIn: "root" })
export class FileDropService {
  private agents = inject(AgentsStore);
  private ui = inject(UiStore);
  private zone = inject(NgZone);
  private started = false;

  /** Register the OS drag-drop listener once. No-op outside Tauri. */
  async init(): Promise<void> {
    if (this.started || !inTauri()) return;
    this.started = true;
    const { getCurrentWindow } = await import("@tauri-apps/api/window");
    await getCurrentWindow().onDragDropEvent((event) => {
      if (event.payload.type !== "drop") return;
      const paths = event.payload.paths ?? [];
      if (!paths.length) return;
      const target = this.ui.scopeAgentId();
      if (!target) {
        this.zone.run(() => this.ui.flash("Focus an agent to drop files into it"));
        return;
      }
      const text = paths.map(quotePath).join(" ") + " ";
      // Show the terminal so the pasted path is visible, then write it to the PTY.
      this.zone.run(() => this.ui.openAgent(target, "terminal"));
      void this.agents.input(target, PASTE_START + text + PASTE_END).catch(() => {});
    });
  }
}
