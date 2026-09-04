import { inject, Injectable } from "@angular/core";
import { Agent, MenuItem } from "../models";
import { NotificationStore } from "../stores/notifications.store";
import { AgentsStore } from "../stores/agents.store";
import { TerminalService } from "../terminal.service";
import { UiStore } from "../ui/ui.store";
import { toolMeta } from "../utils";
import { ProjectActionsService } from "../projects/project-actions.service";
import { AgentRuntimeService } from "./agent-runtime.service";
import { AgentWorkStore } from "./agent-work.store";
import { ConflictStore } from "./conflict.store";

export interface SpawnRequest {
  projectId: string;
  branch: string;
  toolId: Agent["tool"];
  model: string;
  effort: string | null;
  name: string;
  prompt: string;
  /** When set, the backend attaches this ticket (→ inprogress + agentId). */
  ticketId?: string;
  /** When true, launch the agent's process right after creating it (Spawn);
   *  otherwise it's created idle until the user Starts it (Create). */
  start?: boolean;
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
  private work = inject(AgentWorkStore);
  private conflicts = inject(ConflictStore);
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
        this.aiAction(id, "rebase");
        break;
      case "merge": {
        // native path is the default (A4); the AI variant lives in the
        // git-tab dropdown (aiAction "merge") and stays available there.
        const proj = ag ? this.projects.all().find((p) => p.id === ag.projectId) : undefined;
        this.mergeAgent(id, proj?.branch ?? proj?.defaultBranch ?? "main");
        break;
      }
    }
  }

  /** Real commit of the selected paths (empty = all) in the agent's worktree. */
  commitAgent(id: string, paths: string[], message: string) {
    const ag = this.agents().find((a) => a.id === id);
    void this.agentsStore
      .commit(id, message, paths)
      .then(() => {
        this.ui.flash("committed in " + (ag?.name ?? id));
        // Optimistic instant refresh for the EXPLICIT user action — the watcher
        // push follows (debounce ≥200ms + scan) and supersedes via the store's
        // gen guards; without this the badge/feed feedback lags the click.
        this.work.loadChanges(id);
        this.work.refreshCommits(id);
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
        // optimistic instant refresh; the watcher push follows and supersedes
        this.work.loadChanges(id);
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

  /** NATIVE merge of `branch` (the project base) into the agent's branch —
   *  instant, free, deterministic (A3.5). A clean/FF merge just refreshes; a
   *  conflicted merge keeps the session open backend-side and flips the
   *  agent's diff pane to the 3-way conflict view (B4.2). */
  mergeAgent(id: string, branch: string) {
    const ag = this.agents().find((a) => a.id === id);
    void this.agentsStore
      .merge(id, branch)
      .then((session) => {
        if (session.conflicts.length) {
          this.conflicts.open(id, session.ours, session.theirs, session.conflicts);
          this.ui.setGitView(id, { kind: "conflict" });
          this.ui.flash(
            session.conflicts.length +
              " conflict" +
              (session.conflicts.length === 1 ? "" : "s") +
              " — resolve in the 3-way view",
          );
          return;
        }
        this.ui.flash("merged " + branch + " into " + (ag?.name ?? id));
        // optimistic instant refresh; the watcher push follows and supersedes
        this.work.loadChanges(id);
        this.work.refreshCommits(id);
      })
      .catch((e: { message?: string }) => this.ui.flash(e?.message ?? "merge failed"));
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
    const name = req.name; // user-given (duplicates allowed), drives the worktree name
    const proj = this.projects.all().find((p) => p.id === req.projectId);
    this.ui.closeSpawn();

    // The worktree checkout behind agent_spawn can take many seconds on a big
    // repo, so the row shows up NOW as a placeholder under the id we generate
    // here; agent://created lands under the same id and simply replaces it.
    const id = crypto.randomUUID();
    this.agentsStore.addPlaceholder({
      id,
      projectId: req.projectId,
      ...(req.ticketId ? { ticketId: req.ticketId } : {}),
      tool: req.toolId,
      model: req.model,
      effort: req.effort,
      name,
      task: req.prompt,
      status: "idle",
      branch: "",
      worktree: "",
      base: req.branch,
      started: false,
      commits: 0,
      elapsed: 0,
      progress: 0,
      pending: [],
      transition: "creating",
    });

    // round-trip: backend persists the agent + emits agent://created → store upserts it
    let ag: Agent;
    try {
      ag = await this.agentsStore.spawn({
        id,
        projectId: req.projectId,
        tool: req.toolId,
        model: req.model,
        effort: req.effort,
        name,
        task: req.prompt,
        base: req.branch,
        ...(req.ticketId ? { ticketId: req.ticketId } : {}),
      });
    } catch (e) {
      this.ui.flash((e as { message?: string })?.message ?? "spawn failed");
      return;
    } finally {
      this.agentsStore.dropPlaceholder(id);
    }

    const where = proj ? " in " + proj.name : "";
    if (req.start) {
      // Spawn: open the agent's terminal and launch its process now — its initial
      // prompt drives the run.
      this.ui.openAgent(id, "terminal");
      this.runtime.startProcess(id);
      this.ui.flash("spawned " + name + where);
    } else {
      // Create: lazy lifecycle — the agent stays idle until the user Starts it. The
      // real PTY fills the terminal on Start; here we only show an idle hint.
      this.terminals.hint(
        id,
        `idle — press Start to launch ${toolMeta(req.toolId).name} in this worktree.`,
      );
      this.ui.flash("created " + name + where);
    }
  }

  /** One-button run toggle for a single agent (pane header / overview).
   *  running → pause; otherwise resume (if it ever ran) or start fresh. */
  toggleRun(ag: Agent) {
    if (ag.status === "running") this.act(ag.id, "pause");
    else this.act(ag.id, ag.started ? "resume" : "start");
  }

  /** Is any agent currently running? (drives the global Pause-all / Run-all label) */
  anyRunning(): boolean {
    return this.agents().some((a) => a.status === "running");
  }
  /** Stop every running agent's process. */
  pauseAll() {
    const running = this.agents().filter((a) => a.status === "running");
    running.forEach((a) => this.runtime.stopProcess(a.id));
    this.ui.flash(`paused ${running.length} agent${running.length === 1 ? "" : "s"}`);
  }
  /** Start/resume every agent that isn't running and isn't already done. */
  startAll() {
    const startable = this.agents().filter((a) => a.status !== "running" && a.status !== "done");
    startable.forEach((a) => this.runtime.startProcess(a.id, { resume: a.started }));
    this.ui.flash(`started ${startable.length} agent${startable.length === 1 ? "" : "s"}`);
  }
  /** Global toggle: pause all if anything is running, else start all idle agents. */
  toggleRunAll() {
    if (this.anyRunning()) this.pauseAll();
    else this.startAll();
  }

  duplicateAgent(src: Agent) {
    const proj = this.projects.all().find((p) => p.id === src.projectId);
    void this.spawn({
      projectId: src.projectId,
      // the project's resolved default branch, mirroring the spawn modal's default
      branch: proj?.defaultBranch ?? proj?.branches?.[0] ?? "main",
      toolId: src.tool,
      model: src.model,
      effort: src.effort ?? null,
      name: src.name + "-copy",
      prompt: src.task,
    });
  }

  /** Confirmed worktree delete: stop the agent, close its tabs, drop the
   *  worktree, then tear down local state. Called by the delete-worktree
   *  confirm modal — never directly from a menu item.
   *
   *  `hard` is that modal's checkbox: it additionally erases the worktree
   *  folder. Without it the branch and the files stay on disk and only the
   *  agent goes away, so a mis-click never costs uncommitted work. */
  async confirmRemoveAgent(id: string, hard = false) {
    const ag = this.agents().find((a) => a.id === id);
    const label = ag ? ag.name : id;
    this.ui.closeDeleteWorktree();
    // stoppingByUser keeps the exit from being recorded as "finished"; the
    // backend kills the PTY again inside agent_remove as the hard guarantee.
    if (ag?.status === "running") this.runtime.stopProcess(id);
    this.ui.closeTabsForAgent(id);
    // the row dims at once; a hard delete can spend seconds on the folder
    this.runtime.patchRuntime(id, { transition: "removing" });
    try {
      await this.agentsStore.remove(id, hard);
    } catch {
      this.runtime.patchRuntime(id, { transition: undefined });
      // A hard delete fails as a unit — the backend aborts before touching git
      // or the database — so the agent really is still there in both cases.
      this.ui.flash(
        hard
          ? "delete failed — folder locked, worktree kept for " + label
          : "delete failed — worktree kept for " + label,
      );
      return;
    }
    this.runtime.dispose(id);
    this.conflicts.dispose(id);
    this.notifications.clearAgent(id);
    this.ui.flash(hard ? `deleted worktree ${label} and its folder` : `removed worktree ${label} — folder kept`);
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
        disabled: !this.work.changesFor(id).data.length,
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
        disabled: !this.work.changesFor(id).data.length,
        onClick: () => this.act(id, "discard"),
      },
      { label: "Delete worktree", icon: "trash", danger: true, onClick: () => this.ui.openDeleteWorktree(id) },
    ];
  }
}
