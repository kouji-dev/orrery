import { Injectable, computed, inject } from "@angular/core";
import { Agent } from "../../models";
import { AgentRuntimeService } from "../../agents/agent-runtime.service";
import { projectRootKey } from "../../agents/agent-work.store";
import { ProjectActionsService } from "../../projects/project-actions.service";
import { UiStore } from "../../ui/ui.store";

/**
 * Owns the files section's ROOT: which worktree the tree is rooted at, and who
 * is allowed to move it. Lives outside the component because the sidebar's
 * project rows re-root too (see `selectProject`) — the resolution must not be
 * private to the panel that renders it.
 */
@Injectable({ providedIn: "root" })
export class FilesRootService {
  private readonly ui = inject(UiStore);
  private readonly projects = inject(ProjectActionsService);
  private readonly runtime = inject(AgentRuntimeService);

  /** What the section follows when the user hasn't overridden: the active
   *  tab's scoped agent, else the first project's main worktree. The v2
   *  project pseudo-agent (id === projectId) maps to the `proj:` ROOT key —
   *  its raw id would read as an agent root and the chip would show "—". */
  readonly followKey = computed<string | null>(() => {
    const ag = this.runtime.activeAgent();
    if (ag) return ag.id === ag.projectId ? projectRootKey(ag.projectId) : ag.id;
    const first = this.projects.all()[0];
    return first ? projectRootKey(first.id) : null;
  });

  /** The effective root: the active tab's explicit pick, else follow. A stale
   *  override (agent removed) falls back to follow. */
  readonly rootKey = computed<string | null>(() => {
    const override = this.ui.filesRootOverride()[this.ui.activeTab()];
    if (override && this.rootExists(override)) return override;
    return this.followKey();
  });

  readonly overridden = computed(
    () => this.rootKey() !== null && this.rootKey() !== this.followKey(),
  );

  private rootExists(key: string): boolean {
    if (key.startsWith("proj:")) return this.projects.all().some((p) => p.id === key.slice(5));
    return this.runtime.agents().some((a) => a.id === key);
  }

  agentsOf(projectId: string): Agent[] {
    return this.runtime.agents().filter((a) => a.projectId === projectId);
  }

  pickRoot(key: string): void {
    // picking what follow already shows clears the override (chip un-highlights)
    this.ui.setFilesRootOverride(this.ui.activeTab(), key === this.followKey() ? null : key);
  }

  /**
   * Clicking a project row roots the tree at that project's main — but only
   * when the project is not already in scope. If the effective root is one of
   * THIS project's worktrees, re-rooting would yank the tree away from the
   * worktree the user is actually working in, so the click stays a pure
   * collapse/expand.
   */
  selectProject(projectId: string): void {
    const key = this.rootKey();
    if (key && this.projectOf(key) === projectId) return;
    this.pickRoot(projectRootKey(projectId));
  }

  /** The project a ROOT key belongs to — `proj:<id>` directly, an agent root
   *  through the agent that owns the worktree. */
  private projectOf(key: string): string | null {
    if (key.startsWith("proj:")) return key.slice(5);
    return this.runtime.agents().find((a) => a.id === key)?.projectId ?? null;
  }
}
