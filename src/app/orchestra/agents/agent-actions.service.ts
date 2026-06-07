import { inject, Injectable } from "@angular/core";
import { Agent, MenuItem } from "../models";
import { NotificationStore } from "../stores/notifications.store";
import { AgentsStore } from "../stores/agents.store";
import { TerminalService } from "../terminal.service";
import { UiStore } from "../ui/ui.store";
import { toolMeta } from "../utils";
import { ProjectActionsService } from "../projects/project-actions.service";
import { AgentRuntimeService } from "./agent-runtime.service";

export interface SpawnRequest {
  projectId: string;
  branch: string;
  toolId: Agent["tool"];
  model: string;
  effort: string | null;
  name: string;
  prompt: string;
}

/**
 * Agent-level user actions: spawn / duplicate / remove, the run-control verbs
 * (start/pause/commit/discard/merge), and the agent context menu. Orchestrates
 * the runtime + data stores; holds no live state of its own.
 */
@Injectable({ providedIn: "root" })
export class AgentActionsService {
  private agentsStore = inject(AgentsStore);
  private runtime = inject(AgentRuntimeService);
  private projects = inject(ProjectActionsService);
  private terminals = inject(TerminalService);
  private notifications = inject(NotificationStore);
  private ui = inject(UiStore);

  private get agents() {
    return this.runtime.agents;
  }

  act(id: string, action: string) {
    const ag = this.agents().find((a) => a.id === id);
    const nm = ag ? ag.name : id;
    switch (action) {
      case "pause":
        this.runtime.stopProcess(id);
        this.ui.flash("stopped " + nm);
        break;
      case "resume":
      case "start":
        this.runtime.startProcess(id);
        this.ui.flash((action === "start" ? "started " : "resumed ") + nm);
        break;
      case "continueSession":
        this.runtime.startProcess(id, { resume: true });
        this.ui.flash("continuing session for " + nm);
        break;
      case "commit": // all changes (the git-tab uses commitAgent with a selection)
        this.commitAgent(id, [], "wip: " + nm);
        break;
      case "discard":
        this.discardAgent(id, []);
        break;
      case "merge":
        this.mergeAgent(id);
        break;
      case "push": // remote ops need auth — not wired yet
        this.ui.flash("push not configured yet");
        break;
      case "pr":
        this.ui.flash("PR not configured yet");
        break;
    }
  }

  /** Real commit of the selected paths (empty = all) in the agent's worktree. */
  commitAgent(id: string, paths: string[], message: string) {
    const ag = this.agents().find((a) => a.id === id);
    void this.agentsStore
      .commit(id, message, paths)
      .then(() => {
        this.ui.flash("committed in " + (ag?.name ?? id));
        this.runtime.loadChanges(id);
        void this.projects.refreshCommits(this.projects.all().map((p) => p.id));
        if (ag) this.runtime.patchRuntime(id, { commits: ag.commits + 1 });
      })
      .catch((e: { message?: string }) => this.ui.flash(e?.message ?? "commit failed"));
  }

  discardAgent(id: string, paths: string[]) {
    void this.agentsStore
      .discard(id, paths)
      .then(() => {
        this.ui.flash("discarded changes");
        this.runtime.loadChanges(id);
      })
      .catch((e: { message?: string }) => this.ui.flash(e?.message ?? "discard failed"));
  }

  /** Merge the agent's branch into its source project's branch. */
  mergeAgent(id: string) {
    const ag = this.agents().find((a) => a.id === id);
    const proj = ag ? this.projects.all().find((p) => p.id === ag.projectId) : null;
    void this.agentsStore
      .merge(id)
      .then(() => {
        this.ui.flash("merged " + (ag?.name ?? id) + " → " + (proj?.branch ?? "main"));
        void this.agentsStore.update(id, { status: "done" }).catch(() => {});
        this.runtime.patchRuntime(id, { progress: 1 });
        this.runtime.loadChanges(id);
        void this.projects.refreshCommits(this.projects.all().map((p) => p.id));
      })
      .catch((e: { message?: string }) => this.ui.flash(e?.message ?? "merge failed"));
  }

  // ---- spawn / duplicate / remove ----
  async spawn(req: SpawnRequest) {
    const name = req.name; // user-given, unique per project, drives the worktree name
    const proj = this.projects.all().find((p) => p.id === req.projectId);
    this.ui.closeSpawn();

    // round-trip: backend persists the agent + emits agent://created → store upserts it
    let ag: Agent;
    try {
      ag = await this.agentsStore.spawn({
        projectId: req.projectId,
        tool: req.toolId,
        model: req.model,
        effort: req.effort,
        name,
        task: req.prompt,
        base: req.branch,
      });
    } catch (e) {
      this.ui.flash((e as { message?: string })?.message ?? "spawn failed");
      return;
    }

    const id = ag.id;
    // lazy lifecycle: the agent stays idle until the user Starts it. The real PTY
    // fills the terminal on Start — here we only show an idle hint.
    this.terminals.hint(
      id,
      `idle — press Start to launch ${toolMeta(req.toolId).name} in this worktree.`,
    );
    this.ui.openAgent(id, "terminal");
    this.ui.flash("spawned " + name + (proj ? " in " + proj.name : ""));
  }

  startAgent(id: string) {
    this.act(id, "start");
  }

  duplicateAgent(src: Agent) {
    void this.spawn({
      projectId: src.projectId,
      branch: "main",
      toolId: src.tool,
      model: src.model,
      effort: src.effort ?? null,
      name: src.name + "-copy",
      prompt: src.task,
    });
  }

  removeAgent(id: string) {
    const ag = this.agents().find((a) => a.id === id);
    void this.agentsStore.remove(id).catch(() => {});
    this.runtime.dispose(id);
    this.notifications.clearAgent(id);
    this.ui.closeTab(id);
    this.ui.flash("removed worktree " + (ag ? ag.name : id));
  }

  // ---- context menu ----
  agentMenu(id: string): MenuItem[] {
    const ag = this.agents().find((a) => a.id === id);
    if (!ag) return [];
    const proj = this.projects.all().find((p) => p.id === ag.projectId);
    const branchTarget = proj ? proj.branch : "main";
    return [
      { label: "Open workspace", icon: "enter", onClick: () => this.ui.openAgent(id) },
      { label: "Open terminal", icon: "terminal", onClick: () => this.ui.openAgent(id, "terminal") },
      { label: "View diff", icon: "diff", onClick: () => this.ui.openAgent(id, "diff") },
      { sep: true },
      ag.status === "running"
        ? { label: "Pause agent", icon: "pause", onClick: () => this.act(id, "pause") }
        : {
            label: ag.started ? "Resume agent" : "Start agent",
            icon: "play",
            disabled: ag.status === "done",
            onClick: () => this.act(id, ag.started ? "resume" : "start"),
          },
      {
        label: "Commit changes",
        icon: "commit",
        disabled: !ag.git_changes?.files.length,
        onClick: () => this.act(id, "commit"),
      },
      { label: "Push to origin", icon: "push", disabled: !ag.commits, onClick: () => this.act(id, "push") },
      { label: "Open pull request", icon: "pr", disabled: !ag.commits, onClick: () => this.act(id, "pr") },
      {
        label: "Merge → " + branchTarget,
        icon: "merge",
        accent: "var(--st-done)",
        disabled: !ag.commits,
        onClick: () => this.act(id, "merge"),
      },
      { sep: true },
      { label: "Rename branch", icon: "rename", onClick: () => this.ui.flash("rename " + ag.branch) },
      { label: "Duplicate agent", icon: "dup", onClick: () => this.duplicateAgent(ag) },
      { sep: true },
      {
        label: "Discard changes",
        icon: "discard",
        disabled: !ag.git_changes?.files.length,
        onClick: () => this.act(id, "discard"),
      },
      { label: "Delete worktree", icon: "trash", danger: true, onClick: () => this.removeAgent(id) },
    ];
  }
}
