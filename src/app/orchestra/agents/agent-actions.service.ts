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
      case "commit": // backend commit-all (the git-tab uses commitAgent with a selection)
        this.commitAgent(id, [], "wip: " + nm);
        break;
      case "discard":
        this.discardAgent(id, []);
        break;
      case "push": // deterministic backend push
        this.pushAgent(id);
        break;
      case "rebase":
      case "merge":
        this.aiAction(id, action as "rebase" | "merge");
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
        this.runtime.loadCommits(id);
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

  /** Deterministic backend push of the agent's branch to origin. */
  pushAgent(id: string) {
    const ag = this.agents().find((a) => a.id === id);
    void this.agentsStore
      .push(id)
      .then(() => this.ui.flash("pushed " + (ag?.name ?? id)))
      .catch((e: { message?: string }) => this.ui.flash(e?.message ?? "push failed"));
  }

  /** AI-driven completion action: type the predefined prompt into the agent's PTY,
   *  switching to the terminal so the user watches it run. If the agent is idle it
   *  is launched first (resuming its session when it has one), then the prompt is
   *  sent after a short delay so the tool has reached its input — the PTY must be
   *  live to receive the keystrokes. */
  aiAction(id: string, kind: "commit" | "push" | "rebase" | "merge") {
    const ag = this.agents().find((a) => a.id === id);
    if (!ag) return;
    this.ui.openAgent(id, "terminal");
    const send = () =>
      void this.agentsStore
        .action(id, kind)
        .catch((e: { message?: string }) => this.ui.flash(e?.message ?? "action failed"));
    if (ag.status === "running") {
      send();
      return;
    }
    this.ui.flash("starting " + ag.name + "…");
    this.runtime.startProcess(id, { resume: ag.started });
    setTimeout(send, 1800);
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
      {
        label: "Rebase onto " + branchTarget,
        icon: "sparkles",
        onClick: () => this.act(id, "rebase"),
      },
      {
        label: "Merge " + branchTarget + " → " + ag.branch.replace("agent/", ""),
        icon: "sparkles",
        accent: "var(--st-done)",
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
