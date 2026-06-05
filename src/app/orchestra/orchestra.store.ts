import { computed, effect, inject, Injectable, signal } from "@angular/core";
import { AGENT_TOOLS, ORG, WORKTREE_ROOT } from "./data";
import { AgentsStore } from "./stores/agents.store";
import { ProjectsStore } from "./stores/projects.store";
import { TerminalService } from "./terminal.service";
import {
  Agent,
  AgentNotification,
  Commit,
  ContextMenuState,
  FileNode,
  LogLine,
  MenuItem,
  PendingItem,
  Project,
  Tab,
  Tweaks,
  VizMode,
} from "./models";
import {
  appendPtyTail,
  detectTitleStatus,
  hexRgb,
  isAwaitingInput,
  isPermissionPrompt,
  TitleStatus,
  toolMeta,
} from "./utils";
import { NotificationStore } from "./stores/notifications.store";

const TWEAK_DEFAULTS: Tweaks = {
  theme: "dark",
  palette: ["#a855f7", "#22d3ee"],
  density: "regular",
  defaultViz: "grid",
  rightPanel: true,
  motion: true,
};

const TWEAKS_KEY = "katrix.tweaks";
function loadTweaks(): Tweaks {
  try {
    const saved = JSON.parse(localStorage.getItem(TWEAKS_KEY) || "null");
    return saved ? { ...TWEAK_DEFAULTS, ...saved } : { ...TWEAK_DEFAULTS };
  } catch {
    return { ...TWEAK_DEFAULTS };
  }
}

export interface SpawnRequest {
  projectId: string;
  branch: string;
  toolId: Agent["tool"];
  model: string;
  effort: string | null;
  name: string;
  prompt: string;
}
export interface AddProjectRequest {
  path: string;
  name: string;
  icon: string;
  color: string;
  gitInit: boolean;
}

@Injectable({ providedIn: "root" })
export class OrchestraStore {
  // ---- core state ----
  private projectsStore = inject(ProjectsStore);
  private agentsStore = inject(AgentsStore);
  private terminals = inject(TerminalService);
  readonly notifications = inject(NotificationStore);
  readonly tweaks = signal<Tweaks>(loadTweaks());
  readonly projects = this.projectsStore.all;
  // backend agents (identity/status) merged with an in-memory runtime overlay
  // that holds the live-sim metrics (elapsed/progress/files/pending) until task #7.
  private runtime = signal<Record<string, Partial<Agent>>>({});
  readonly agents = computed<Agent[]>(() => {
    const rt = this.runtime();
    return this.agentsStore.all().map((a) => {
      const o = rt[a.id];
      return o ? { ...a, ...o } : a;
    });
  });
  readonly tabs = signal<Tab[]>([{ id: "orchestrator" }]);
  readonly activeTab = signal<string>("orchestrator");
  readonly query = signal<string>("");
  readonly viz = signal<VizMode>(TWEAK_DEFAULTS.defaultViz);
  readonly liveLogs = signal<Record<string, LogLine[]>>({});
  readonly commits = signal<Commit[]>([]);
  readonly running = signal<boolean>(true);
  readonly toast = signal<string>("");
  // which CLI agent tools are installed (best-effort; empty until detected)
  readonly toolsAvailable = signal<Record<string, boolean>>({});

  // ---- transient ui state ----
  readonly spawning = signal<{ project: string | null } | null>(null);
  readonly addingProject = signal<boolean>(false);
  readonly contextMenu = signal<ContextMenuState | null>(null);

  // pane hint when an agent tab is opened from a deep action
  readonly paneHint: Record<string, string> = {};

  // real liveness tracking (replaces the simulated stream): when a process was
  // launched, and when it last produced output — drives elapsed + the working flag.
  private startedAt: Record<string, number> = {};
  private lastOutputAt: Record<string, number> = {};
  // last recognized terminal-title state + when it last changed (a working
  // spinner refreshes constantly, so a stale "working" title is treated as quiet)
  private titleStatus: Record<string, TitleStatus> = {};
  private titleAt: Record<string, number> = {};
  // notification edge-tracking: last "needs input" state per agent + whether a
  // stop was user-initiated (so killing a process doesn't fire a "done" toast).
  private prevNeedsInput: Record<string, boolean> = {};
  private stoppingByUser: Record<string, boolean> = {};
  private toastTimer: ReturnType<typeof setTimeout> | null = null;

  readonly org = ORG;
  readonly worktreeRoot = WORKTREE_ROOT;

  // ---- derived ----
  readonly activeAgent = computed<Agent | null>(() => {
    const id = this.activeTab();
    if (id === "orchestrator") return null;
    return this.agents().find((a) => a.id === id) ?? null;
  });

  constructor() {
    // reflect tweaks onto <html> + accent css vars
    effect(() => {
      const t = this.tweaks();
      const r = document.documentElement;
      r.setAttribute("data-theme", t.theme);
      r.setAttribute("data-density", t.density);
      r.setAttribute("data-motion", t.motion ? "on" : "off");
      const [a1, a2] = t.palette;
      r.style.setProperty("--accent", a1);
      r.style.setProperty("--accent-2", a2);
      r.style.setProperty("--accent-rgb", hexRgb(a1));
      r.style.setProperty("--accent-2-rgb", hexRgb(a2));
    });

    // keep viz in sync with default
    effect(() => this.viz.set(this.tweaks().defaultViz));

    // persist tweaks across sessions (task #8)
    effect(() => {
      try {
        localStorage.setItem(TWEAKS_KEY, JSON.stringify(this.tweaks()));
      } catch {
        /* storage unavailable — ignore */
      }
    });

    // load real commit history whenever the set of projects changes (task #3)
    effect(() => {
      const ids = this.projects().map((p) => p.id);
      void this.refreshCommits(ids);
    });

    // detect installed CLI tools once (task #8)
    void this.agentsStore
      .detectTools()
      .then((list) =>
        this.toolsAvailable.set(
          Object.fromEntries(list.map((t) => [t.id, t.available])),
        ),
      )
      .catch(() => {});

    // load the active agent's worktree transients (debounced via supersession)
    effect(() => {
      const id = this.activeTab();
      if (id !== "orchestrator") {
        this.loadChanges(id);
        this.loadFiles(id);
        void this.agentsStore.watch(id).catch(() => {}); // fs-watch the active worktree
      }
    });

    // reload the active agent's worktree when it changes on disk (task #9)
    void this.agentsStore
      .onWorktreeChanged((id) => {
        if (this.activeTab() === id) {
          this.loadFiles(id);
          this.loadChanges(id);
        }
      })
      .catch(() => {});

    // live agent state from the terminal title (spinner = working, ✋ = needs input)
    this.terminals.onTitle((id, title) => {
      const s = detectTitleStatus(title);
      if (!s) return; // unrecognized title (e.g. a cwd) — keep the last known state
      this.titleStatus[id] = s;
      this.titleAt[id] = Date.now();
    });

    // stream agent process output: raw bytes → the agent's xterm terminal,
    // a plain-text tail → liveLogs (for the compact overview mini-term).
    void this.agentsStore
      .onOutput((id, chunk) => {
        this.lastOutputAt[id] = Date.now(); // real activity signal
        this.terminals.write(id, chunk);
        this.liveLogs.update((m) => {
          const prev = (m[id] || []).map((l) => l.s);
          return {
            ...m,
            [id]: appendPtyTail(prev, chunk).map((s) => ({
              t: "out" as const,
              s,
            })),
          };
        });
      })
      .catch(() => {});
    void this.agentsStore
      .onExit((id) => {
        this.terminals.exit(id);
        delete this.startedAt[id];
        delete this.titleStatus[id];
        delete this.titleAt[id];
        delete this.prevNeedsInput[id];
        this.patchRuntime(id, { working: false, needsInput: false });
        // its question/permission is moot now that the process is gone
        this.notifications.dismissPendingFor(id, ["permission", "question"]);
        // process ended → reflect liveness: drop back to idle unless it reported done
        const ag = this.agents().find((a) => a.id === id);
        if (ag && ag.status !== "done") {
          void this.agentsStore.update(id, { status: "idle" }).catch(() => {});
        }
        // a natural exit (not a user-initiated stop) = work finished → notify
        if (ag && !this.stoppingByUser[id]) {
          this.raise({
            agentId: id,
            agentName: ag.name,
            kind: "done",
            title: `${ag.name} finished`,
            detail: this.promptTail(id) || "Process ended — review or merge its work.",
          });
        }
        delete this.stoppingByUser[id];
        this.liveLogs.update((m) => ({
          ...m,
          [id]: [
            ...(m[id] || []),
            { t: "sys" as const, s: "▪ process exited" },
          ],
        }));
      })
      .catch(() => {});

    // liveness loop: refresh elapsed + working for running agents
    setInterval(() => this.tick(), 800);
  }

  // ---- worktree scans (async, superseded per worktree) ----
  private changesGen: Record<string, number> = {};
  /** (Re)scan an agent's working-tree changes; a newer request supersedes this one. */
  loadChanges(agentId: string) {
    const gen = (this.changesGen[agentId] ?? 0) + 1;
    this.changesGen[agentId] = gen;
    this.patchRuntime(agentId, { git_changes: { loading: true, files: [] } });
    void this.agentsStore
      .changes(agentId)
      .then((files) => {
        if (this.changesGen[agentId] !== gen) return; // superseded — drop stale result
        this.patchRuntime(agentId, { git_changes: { loading: false, files } });
      })
      .catch(() => {
        if (this.changesGen[agentId] !== gen) return;
        this.patchRuntime(agentId, {
          git_changes: { loading: false, files: [] },
        });
      });
  }

  private filesGen: Record<string, number> = {};
  /** (Re)scan an agent's worktree file tree; a newer request supersedes this one. */
  loadFiles(agentId: string) {
    const gen = (this.filesGen[agentId] ?? 0) + 1;
    this.filesGen[agentId] = gen;
    this.patchRuntime(agentId, { files: { loading: true, nodes: [] } });
    void this.agentsStore
      .tree(agentId)
      .then((nodes) => {
        if (this.filesGen[agentId] !== gen) return; // superseded
        this.patchRuntime(agentId, { files: { loading: false, nodes } });
      })
      .catch(() => {
        if (this.filesGen[agentId] !== gen) return;
        this.patchRuntime(agentId, { files: { loading: false, nodes: [] } });
      });
  }

  /** Lazily load an unexpanded folder's children into the tree overlay. */
  expandDir(agentId: string, path: string) {
    void this.agentsStore.listDir(agentId, path).then((kids) => {
      const cur = this.runtime()[agentId]?.files;
      if (!cur) return;
      const patch = (list: FileNode[]): FileNode[] =>
        list.map((n) => {
          if (n.path === path) return { ...n, children: kids };
          if (n.children) return { ...n, children: patch(n.children) };
          return n;
        });
      this.patchRuntime(agentId, {
        files: { loading: cur.loading, nodes: patch(cur.nodes) },
      });
    });
  }

  /** True unless detection has positively reported the tool as missing. */
  toolAvailable(id: string): boolean {
    return this.toolsAvailable()[id] !== false;
  }

  // pull real git log for every project and merge into one feed (newest first)
  private async refreshCommits(projectIds: string[]) {
    const lists = await Promise.all(
      projectIds.map((id) =>
        this.projectsStore.commits(id).catch(() => [] as Commit[]),
      ),
    );
    this.commits.set(
      lists.flat().sort((a, b) => (b.ts ?? Infinity) - (a.ts ?? Infinity)),
    );
  }

  // merge a partial into an agent's runtime overlay (live-sim fields only)
  private patchRuntime(id: string, patch: Partial<Agent>) {
    this.runtime.update((m) => ({
      ...m,
      [id]: { ...(m[id] ?? {}), ...patch },
    }));
  }
  private clearRuntime(id: string) {
    this.runtime.update((m) => {
      const { [id]: _drop, ...rest } = m;
      return rest;
    });
  }

  // ---- tweaks ----
  setTweak<K extends keyof Tweaks>(key: K, value: Tweaks[K]) {
    this.tweaks.update((t) => ({ ...t, [key]: value }));
  }
  toggleTheme() {
    this.setTweak("theme", this.tweaks().theme === "dark" ? "light" : "dark");
  }

  // ---- toast ----
  flash(msg: string) {
    this.toast.set(msg);
    if (this.toastTimer) clearTimeout(this.toastTimer);
    this.toastTimer = setTimeout(() => this.toast.set(""), 2600);
  }

  // ---- live tick: derive REAL elapsed + working/needsInput from process state ----
  private tick() {
    const now = Date.now();
    this.agents().forEach((ag) => {
      if (ag.status !== "running") return;
      const elapsed = Math.max(
        0,
        Math.round((now - (this.startedAt[ag.id] ?? now)) / 1000),
      );
      const { working, needsInput } = this.liveState(ag.id, now);
      if (
        ag.elapsed !== elapsed ||
        ag.working !== working ||
        ag.needsInput !== needsInput
      ) {
        this.patchRuntime(ag.id, { elapsed, working, needsInput });
      }
      this.detectNeedsInput(ag, needsInput);
    });
  }

  // ---- notifications: detect agent → user transitions ----
  /** On the rising edge of "needs input", raise a question/permission notification;
   *  auto-dismiss it when the agent moves on by itself. */
  private detectNeedsInput(ag: Agent, needsInput: boolean) {
    const was = this.prevNeedsInput[ag.id] ?? false;
    if (needsInput && !was) {
      const detail = this.promptTail(ag.id);
      const permission = isPermissionPrompt(detail);
      this.raise({
        agentId: ag.id,
        agentName: ag.name,
        kind: permission ? "permission" : "question",
        title: permission ? `${ag.name} needs permission` : `${ag.name} has a question`,
        detail,
      });
    } else if (!needsInput && was) {
      this.notifications.dismissPendingFor(ag.id, ["permission", "question"]);
    }
    this.prevNeedsInput[ag.id] = needsInput;
  }

  /** Last few non-empty terminal lines — the prompt context for a notification. */
  private promptTail(id: string): string {
    return (this.liveLogs()[id] || [])
      .map((l) => l.s.trim())
      .filter((s) => s.length > 0)
      .slice(-5)
      .join("\n");
  }

  /** Push to the in-app feed and (if the window is unfocused) the OS. */
  private raise(input: {
    agentId: string;
    agentName: string;
    kind: AgentNotification["kind"];
    title: string;
    detail: string;
  }) {
    const note = this.notifications.push(input);
    if (note) void this.notifyOS(note);
  }

  /** Fire a native OS notification when the app isn't focused (best-effort). */
  private async notifyOS(note: AgentNotification) {
    if (typeof document !== "undefined" && document.hasFocus()) return;
    try {
      const m = await import("@tauri-apps/plugin-notification");
      let granted = await m.isPermissionGranted();
      if (!granted) granted = (await m.requestPermission()) === "granted";
      if (granted) {
        m.sendNotification({ title: note.title, body: note.detail || "tap to open" });
      }
    } catch {
      // not running under Tauri / plugin unavailable — in-app feed still has it
    }
  }

  // ---- notification actions (perform side effect + record the decision) ----
  /** Accept a permission request: send an affirmative keystroke to the PTY. */
  acceptNotification(n: AgentNotification) {
    if (n.kind === "permission") {
      void this.agentsStore.input(n.agentId, "y\r").catch(() => {});
    }
    this.notifications.decide(n.id, "accepted", "accepted");
    this.flash("accepted · " + n.agentName);
  }
  /** Reject a permission request: send a negative keystroke to the PTY. */
  rejectNotification(n: AgentNotification) {
    if (n.kind === "permission") {
      void this.agentsStore.input(n.agentId, "n\r").catch(() => {});
    }
    this.notifications.decide(n.id, "rejected", "rejected");
    this.flash("rejected · " + n.agentName);
  }
  /** Open the agent's terminal for full context (and resolve a question). */
  openNotification(n: AgentNotification) {
    this.openAgent(n.agentId, "terminal");
    if (n.kind === "question") this.notifications.decide(n.id, "accepted", "opened");
  }
  mergeNotification(n: AgentNotification) {
    this.act(n.agentId, "merge");
    this.notifications.decide(n.id, "accepted", "merge");
  }
  reviewNotification(n: AgentNotification) {
    this.openAgent(n.agentId, "diff");
    this.notifications.decide(n.id, "accepted", "review");
  }
  dismissNotification(n: AgentNotification) {
    this.notifications.dismiss(n.id);
  }

  // Prefer the terminal-title signal (precise: distinguishes working vs waiting
  // on the user); fall back to recent PTY output for agents that set no title.
  private liveState(
    id: string,
    now: number,
  ): { working: boolean; needsInput: boolean } {
    const outputRecent = now - (this.lastOutputAt[id] ?? 0) < 1500;
    const ts = this.titleStatus[id];
    if (ts === "permission") return { working: false, needsInput: true };
    let working: boolean;
    if (ts === "working") {
      // a real working spinner keeps refreshing its title; if it went stale,
      // trust output activity instead of a frozen "working" label
      const titleFresh = now - (this.titleAt[id] ?? 0) < 1500;
      working = titleFresh || outputRecent;
    } else if (ts === "idle") {
      working = false;
    } else {
      working = outputRecent; // no title signal
    }
    // quiet + the last output reads like a prompt → the agent is waiting on us
    // (catches claude/codex y/n prompts that don't change the terminal title)
    const needsInput = !working && isAwaitingInput(this.promptTail(id));
    return { working, needsInput };
  }

  toggleRunAll() {
    const wasRunning = this.running();
    this.running.set(!wasRunning);
    this.flash(wasRunning ? "paused all agents" : "resumed all agents");
  }

  // ---- tabs ----
  openAgent(id: string, pane?: string) {
    if (pane) this.paneHint[id] = pane;
    this.tabs.update((prev) =>
      prev.find((x) => x.id === id) ? prev : [...prev, { id }],
    );
    this.activeTab.set(id);
  }
  selectTab(id: string) {
    this.activeTab.set(id);
  }
  closeTab(id: string) {
    this.tabs.update((prev) => prev.filter((x) => x.id !== id));
    if (this.activeTab() === id) this.activeTab.set("orchestrator");
  }

  // ---- agent actions ----
  act(id: string, action: string) {
    const ag = this.agents().find((a) => a.id === id);
    const nm = ag ? ag.name : id;
    switch (action) {
      case "pause":
        this.stopProcess(id);
        this.flash("stopped " + nm);
        break;
      case "resume":
      case "start":
        this.startProcess(id);
        this.flash((action === "start" ? "started " : "resumed ") + nm);
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
        this.flash("push not configured yet");
        break;
      case "pr":
        this.flash("PR not configured yet");
        break;
    }
  }

  /** Real commit of the selected paths (empty = all) in the agent's worktree. */
  commitAgent(id: string, paths: string[], message: string) {
    const ag = this.agents().find((a) => a.id === id);
    void this.agentsStore
      .commit(id, message, paths)
      .then(() => {
        this.flash("committed in " + (ag?.name ?? id));
        this.loadChanges(id);
        void this.refreshCommits(this.projects().map((p) => p.id));
        if (ag) this.patchRuntime(id, { commits: ag.commits + 1 });
      })
      .catch((e: { message?: string }) =>
        this.flash(e?.message ?? "commit failed"),
      );
  }

  /** Real discard of the selected paths (empty = all). */
  discardAgent(id: string, paths: string[]) {
    void this.agentsStore
      .discard(id, paths)
      .then(() => {
        this.flash("discarded changes");
        this.loadChanges(id);
      })
      .catch((e: { message?: string }) =>
        this.flash(e?.message ?? "discard failed"),
      );
  }

  /** Merge the agent's branch into its source project's branch. */
  mergeAgent(id: string) {
    const ag = this.agents().find((a) => a.id === id);
    const proj = ag ? this.projects().find((p) => p.id === ag.projectId) : null;
    void this.agentsStore
      .merge(id)
      .then(() => {
        this.flash(
          "merged " + (ag?.name ?? id) + " → " + (proj?.branch ?? "main"),
        );
        void this.agentsStore.update(id, { status: "done" }).catch(() => {});
        this.patchRuntime(id, { progress: 1 });
        this.loadChanges(id);
        void this.refreshCommits(this.projects().map((p) => p.id));
      })
      .catch((e: { message?: string }) =>
        this.flash(e?.message ?? "merge failed"),
      );
  }

  // ---- inbox pending action ----
  handleInbox(id: string, item: PendingItem, action: string) {
    const ag = this.agents().find((a) => a.id === id);
    const drop = () => {
      if (ag)
        this.patchRuntime(id, {
          pending: (ag.pending || []).filter((p) => p.id !== item.id),
        });
    };
    if (action === "allow" || action === "always") {
      drop();
      this.liveLogs.update((p) => ({
        ...p,
        [id]: [
          ...(p[id] || []),
          {
            t: "sys",
            s:
              (action === "always" ? "✓ always allow · " : "✓ allowed · ") +
              item.cmd,
          },
          { t: "cmd", s: item.cmd },
        ],
      }));
      this.flash(
        (action === "always" ? "always allow · " : "allowed · ") +
          (ag ? ag.name : id),
      );
    } else if (action === "deny") {
      drop();
      this.liveLogs.update((p) => ({
        ...p,
        [id]: [
          ...(p[id] || []),
          { t: "err", s: "✗ denied by user · " + item.cmd },
        ],
      }));
      this.flash("denied · " + (ag ? ag.name : id));
    } else if (action === "merge") {
      drop();
      this.act(id, "merge");
    } else if (action === "diff") {
      this.openAgent(id, "diff");
    } else if (action === "open") {
      this.openAgent(id, "terminal");
    }
  }

  // ---- spawn ----
  async spawn(req: SpawnRequest) {
    const name = req.name; // user-given, unique per project, drives the worktree name
    const proj = this.projects().find((p) => p.id === req.projectId);
    this.spawning.set(null);

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
      this.flash((e as { message?: string })?.message ?? "spawn failed");
      return;
    }

    const id = ag.id;
    // lazy lifecycle (task #5): the agent stays idle until the user Starts it.
    // The real PTY fills the terminal on Start — here we only show an idle hint.
    this.terminals.hint(
      id,
      `idle — press Start to launch ${toolMeta(req.toolId).name} in this worktree.`,
    );
    this.openAgent(id, "terminal");
    this.flash("spawned " + name + (proj ? " in " + proj.name : ""));
  }

  // Start an idle agent → launches its real tool process.
  startAgent(id: string) {
    this.act(id, "start");
  }

  /** Launch the agent's real tool process (PTY-streamed, env-stamped). */
  startProcess(id: string) {
    const sz = this.terminals.size(id); // open the PTY at the visible terminal's size
    this.startedAt[id] = Date.now();
    delete this.titleStatus[id]; // fresh run — drop any stale title state
    delete this.titleAt[id];
    this.prevNeedsInput[id] = false;
    this.patchRuntime(id, { elapsed: 0, working: true, needsInput: false });
    void this.agentsStore
      .start(id, sz?.rows ?? 0, sz?.cols ?? 0)
      .then(() => this.terminals.syncSize(id)) // re-sync once the process is live
      .catch((e: { message?: string }) =>
        this.flash(e?.message ?? "start failed"),
      );
  }
  /** Stop the agent's running process. */
  stopProcess(id: string) {
    this.stoppingByUser[id] = true; // a user stop is not a "finished work" event
    void this.agentsStore.stop(id).catch(() => {});
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
    this.terminals.dispose(id);
    this.notifications.clearAgent(id);
    this.clearRuntime(id);
    this.closeTab(id);
    this.flash("removed worktree " + (ag ? ag.name : id));
  }

  removeProject(id: string) {
    const p = this.projectOf(id);
    // backend cascade-deletes the project's agents and emits agent://deleted for each
    void this.projectsStore
      .remove(id)
      .then(() => this.flash("removed project " + (p ? p.name : id)))
      .catch(() => {});
  }

  addProject(req: AddProjectRequest) {
    void this.projectsStore
      .create({
        name: req.name,
        path: req.path,
        icon: req.icon,
        color: req.color,
        withGit: req.gitInit,
      })
      .then((p) => this.flash("added project " + p.name))
      .catch((e: { kind?: string; message?: string }) =>
        this.flash(
          e?.kind === "project" || e?.kind === "notFound"
            ? (e.message ?? "failed")
            : "add failed",
        ),
      );
    this.addingProject.set(false);
  }

  // Re-point a project at a new folder (e.g. after it was moved). Reuses the native picker.
  relocateProject(id: string) {
    void this.projectsStore.pickDirectory().then((path) => {
      if (!path) return;
      void this.projectsStore
        .update(id, { path })
        .then((u) => this.flash("relocated " + u.name + " → " + u.path))
        .catch((e: { message?: string }) =>
          this.flash(e?.message ?? "relocate failed"),
        );
    });
  }

  // Initialize a git repo for a project created without one.
  initGitForProject(id: string) {
    void this.projectsStore
      .initGit(id)
      .then((u) => this.flash("git initialized · " + u.name))
      .catch((e: { message?: string }) =>
        this.flash(e?.message ?? "git init failed"),
      );
  }

  // ---- context menus ----
  openMenu(e: MouseEvent, items: MenuItem[]) {
    e.preventDefault();
    e.stopPropagation();
    this.contextMenu.set({ x: e.clientX, y: e.clientY, items });
  }
  closeMenu() {
    this.contextMenu.set(null);
  }

  agentMenu(id: string): MenuItem[] {
    const ag = this.agents().find((a) => a.id === id);
    if (!ag) return [];
    const proj = this.projects().find((p) => p.id === ag.projectId);
    const branchTarget = proj ? proj.branch : "main";
    return [
      {
        label: "Open workspace",
        icon: "enter",
        onClick: () => this.openAgent(id),
      },
      {
        label: "Open terminal",
        icon: "terminal",
        onClick: () => this.openAgent(id, "terminal"),
      },
      {
        label: "View diff",
        icon: "diff",
        onClick: () => this.openAgent(id, "diff"),
      },
      { sep: true },
      ag.status === "running"
        ? {
            label: "Pause agent",
            icon: "pause",
            onClick: () => this.act(id, "pause"),
          }
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
      {
        label: "Push to origin",
        icon: "push",
        disabled: !ag.commits,
        onClick: () => this.act(id, "push"),
      },
      {
        label: "Open pull request",
        icon: "pr",
        disabled: !ag.commits,
        onClick: () => this.act(id, "pr"),
      },
      {
        label: "Merge → " + branchTarget,
        icon: "merge",
        accent: "var(--st-done)",
        disabled: !ag.commits,
        onClick: () => this.act(id, "merge"),
      },
      { sep: true },
      {
        label: "Rename branch",
        icon: "rename",
        onClick: () => this.flash("rename " + ag.branch),
      },
      {
        label: "Duplicate agent",
        icon: "dup",
        onClick: () => this.duplicateAgent(ag),
      },
      { sep: true },
      {
        label: "Discard changes",
        icon: "discard",
        disabled: !ag.git_changes?.files.length,
        onClick: () => this.act(id, "discard"),
      },
      {
        label: "Delete worktree",
        icon: "trash",
        danger: true,
        onClick: () => this.removeAgent(id),
      },
    ];
  }

  projectMenu(id: string): MenuItem[] {
    const p = this.projects().find((x) => x.id === id);
    if (!p) return [];
    const items: MenuItem[] = [
      {
        label: "Spawn agent here",
        icon: "plus",
        accent: "var(--accent)",
        onClick: () => this.spawning.set({ project: id }),
      },
      { sep: true },
      {
        label: "Pull latest",
        icon: "refresh",
        disabled: !p.folderExists,
        onClick: () => this.flash("pulled " + p.name),
      },
    ];
    if (p.folderExists && !p.hasGit) {
      items.push({
        label: "Initialize git",
        icon: "git",
        onClick: () => this.initGitForProject(id),
      });
    }
    items.push(
      {
        label: "Open in terminal",
        icon: "terminal",
        disabled: !p.folderExists,
        onClick: () => this.flash("opened terminal · " + p.path),
      },
      { label: "Copy path", icon: "dup", onClick: () => this.flash(p.path) },
      p.folderExists
        ? {
            label: "Relocate folder…",
            icon: "folderOpen",
            onClick: () => this.relocateProject(id),
          }
        : {
            label: "Folder missing — relocate…",
            icon: "folderOpen",
            accent: "var(--st-blocked)",
            onClick: () => this.relocateProject(id),
          },
      { sep: true },
      {
        label: "Remove project",
        icon: "trash",
        danger: true,
        onClick: () => this.removeProject(id),
      },
    );
    return items;
  }

  // ---- modal openers ----
  openSpawn(projectId: string | null) {
    this.spawning.set({ project: projectId });
  }
  closeSpawn() {
    this.spawning.set(null);
  }
  openAddProject() {
    this.addingProject.set(true);
  }
  closeAddProject() {
    this.addingProject.set(false);
  }

  projectOf(id: string | undefined): Project | undefined {
    return this.projects().find((p) => p.id === id);
  }

  readonly tools = AGENT_TOOLS;
}
