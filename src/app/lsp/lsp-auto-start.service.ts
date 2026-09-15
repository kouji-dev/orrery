import { effect, inject, Injectable, untracked } from "@angular/core";
import { BRIDGE, Commands } from "../data-source/bridge";
import { ExtensionsStore } from "../extensions/extensions.store";
import { AgentsStore } from "../stores/agents.store";

/**
 * Auto-start of language servers (user, 2026-09-15): "out of those installed
 * we should run them automatically when an agent runs in a project" — not
 * only once a file of the language is opened in the editor.
 *
 * One effect over the agent list and the installed server packs: every
 * project with a RUNNING agent is pinned on the backend (`lsp_pin_project`),
 * which detects the languages the project root uses, starts the installed +
 * enabled server for each and keeps them out of the idle reaper; when the
 * last running agent of a project stops, the project is unpinned and its
 * servers fall back to the idle rule (an open file still keeps them). A
 * project is re-pinned when the set of enabled server packs changes, so a
 * pack installed mid-session starts for the projects already at work. No
 * enabled server pack at all → no IPC ever.
 *
 * Started once from the shell (like LspDocSyncService) — `start()` only forces
 * construction; the work is the constructor effect.
 */
@Injectable({ providedIn: "root" })
export class LspAutoStartService {
  private readonly bridge = inject(BRIDGE);
  private readonly agents = inject(AgentsStore);
  private readonly extensions = inject(ExtensionsStore);

  /** Project id → the pack signature it was pinned under. */
  private readonly pinned = new Map<string, string>();

  constructor() {
    effect(() => {
      const running = this.agents.all().filter((a) => a.status === "running");
      const packs = this.extensions
        .servers()
        .filter((p) => p.installed && p.enabled)
        .map((p) => p.id)
        .sort()
        .join(",");
      untracked(() => this.reconcile(new Set(running.map((a) => a.projectId)), packs));
    });
  }

  start(): void {
    // construction is the work
  }

  /** Projects with a running agent are pinned; everything else unpinned. */
  private reconcile(wanted: Set<string>, packs: string): void {
    for (const [id] of [...this.pinned]) {
      if (wanted.has(id)) continue;
      this.pinned.delete(id);
      void this.bridge.invoke(Commands.LspPinProject, { id, pinned: false }).catch(() => {});
    }
    if (!packs) {
      // nothing could start: release what an earlier pack set pinned, and do
      // not poke the backend on every agent tick
      for (const [id] of [...this.pinned]) {
        this.pinned.delete(id);
        void this.bridge.invoke(Commands.LspPinProject, { id, pinned: false }).catch(() => {});
      }
      return;
    }
    for (const id of wanted) {
      if (this.pinned.get(id) === packs) continue;
      this.pinned.set(id, packs);
      void this.bridge.invoke(Commands.LspPinProject, { id, pinned: true }).catch(() => this.pinned.delete(id));
    }
  }
}
