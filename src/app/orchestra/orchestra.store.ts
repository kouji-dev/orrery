import { computed, effect, inject, Injectable, signal } from "@angular/core";
import {
  AGENTS,
  AGENT_TOOLS,
  COMMITS,
  LOGS,
  ORG,
  SPAWN_NAMES,
  STREAM,
  WORKTREE_ROOT,
} from "./data";
import { ProjectsStore } from "./stores/projects.store";
import {
  Agent,
  Commit,
  ContextMenuState,
  LogLine,
  MenuItem,
  PendingItem,
  Project,
  Tab,
  Tweaks,
  VizMode,
} from "./models";
import { hexRgb, toolMeta } from "./utils";

const clone = <T>(v: T): T => JSON.parse(JSON.stringify(v));

const TWEAK_DEFAULTS: Tweaks = {
  theme: "dark",
  palette: ["#a855f7", "#22d3ee"],
  density: "regular",
  defaultViz: "grid",
  rightPanel: true,
  motion: true,
};

export interface SpawnRequest {
  projectId: string;
  branch: string;
  toolId: Agent["tool"];
  model: string;
  effort: string | null;
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
  readonly tweaks = signal<Tweaks>({ ...TWEAK_DEFAULTS });
  readonly projects = this.projectsStore.all;
  readonly agents = signal<Agent[]>(clone(AGENTS));
  readonly tabs = signal<Tab[]>([{ id: "orchestrator" }, { id: "a1" }]);
  readonly activeTab = signal<string>("orchestrator");
  readonly query = signal<string>("");
  readonly viz = signal<VizMode>(TWEAK_DEFAULTS.defaultViz);
  readonly liveLogs = signal<Record<string, LogLine[]>>(this.initLogs());
  readonly commits = signal<Commit[]>(clone(COMMITS));
  readonly running = signal<boolean>(true);
  readonly toast = signal<string>("");

  // ---- transient ui state ----
  readonly spawning = signal<{ project: string | null } | null>(null);
  readonly addingProject = signal<boolean>(false);
  readonly contextMenu = signal<ContextMenuState | null>(null);

  // pane hint when an agent tab is opened from a deep action
  readonly paneHint: Record<string, string> = {};

  private streamIdx: Record<string, number> = {};
  private spawnCount = 0;
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

    // live streaming loop
    setInterval(() => this.tick(), 1100);
  }

  private initLogs(): Record<string, LogLine[]> {
    const o: Record<string, LogLine[]> = {};
    Object.keys(LOGS).forEach((k) => (o[k] = LOGS[k].slice()));
    return o;
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

  // ---- live tick ----
  private tick() {
    if (!this.running()) return;
    const ags = this.agents();
    let changed = false;
    const next = { ...this.liveLogs() };
    ags.forEach((ag) => {
      if (ag.status !== "running") return;
      const pool = STREAM[ag.id];
      if (!pool) return;
      const idx = this.streamIdx[ag.id] || 0;
      if (idx < pool.length && Math.random() > 0.35) {
        next[ag.id] = [...(next[ag.id] || []), pool[idx]];
        this.streamIdx[ag.id] = idx + 1;
        changed = true;
      }
    });
    if (changed) this.liveLogs.set(next);
    this.agents.update((prev) =>
      prev.map((ag) =>
        ag.status === "running"
          ? { ...ag, elapsed: ag.elapsed + 1, progress: Math.min(0.98, ag.progress + 0.004) }
          : ag,
      ),
    );
  }

  toggleRunAll() {
    const wasRunning = this.running();
    this.running.set(!wasRunning);
    this.flash(wasRunning ? "paused all agents" : "resumed all agents");
  }

  // ---- tabs ----
  openAgent(id: string, pane?: string) {
    if (pane) this.paneHint[id] = pane;
    this.tabs.update((prev) => (prev.find((x) => x.id === id) ? prev : [...prev, { id }]));
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
    const proj = ag ? this.projects().find((p) => p.id === ag.projectId) : null;
    this.agents.update((prev) =>
      prev.map((a) => {
        if (a.id !== id) return a;
        if (action === "pause") return { ...a, status: "waiting" };
        if (action === "resume" || action === "start") return { ...a, status: "running" };
        if (action === "merge") return { ...a, status: "done", progress: 1 };
        if (action === "commit") return { ...a, commits: a.commits + 1, files: [] };
        if (action === "discard") return { ...a, files: [] };
        return a;
      }),
    );
    const FLASH: Record<string, string> = {
      merge: "merged " + nm + " → " + (proj ? proj.branch : "main"),
      commit: "committed in " + nm,
      start: "started " + nm,
      resume: "resumed " + nm,
      pause: "paused " + nm,
      push: "pushed " + nm + " to origin",
      pr: "opened PR for " + nm,
      discard: "discarded changes in " + nm,
    };
    if (FLASH[action]) this.flash(FLASH[action]);
    if (action === "merge")
      this.commits.update((c) => [
        {
          agent: id,
          projectId: ag ? ag.projectId : null,
          sha: Math.random().toString(16).slice(2, 9),
          msg: "merge: " + nm + " into " + (proj ? proj.branch : "main"),
          when: "now",
          files: ag ? ag.files.length : 1,
        },
        ...c,
      ]);
    if (action === "commit")
      this.commits.update((c) => [
        {
          agent: id,
          projectId: ag ? ag.projectId : null,
          sha: Math.random().toString(16).slice(2, 9),
          msg: "wip: " + nm,
          when: "now",
          files: ag ? ag.files.length : 1,
        },
        ...c,
      ]);
  }

  // ---- chat decision / message ----
  resolve(id: string, answer: string) {
    const ag = this.agents().find((a) => a.id === id);
    const wasBlocked = ag && ag.status === "blocked";
    this.agents.update((prev) =>
      prev.map((a) =>
        a.id === id && a.status === "blocked"
          ? { ...a, status: "running", pending: (a.pending || []).filter((p) => p.kind !== "decision") }
          : a,
      ),
    );
    if (ag && wasBlocked) {
      this.streamIdx[id] = 0;
      this.liveLogs.update((p) => ({
        ...p,
        [id]: [
          ...(p[id] || []),
          { t: "sys", s: "▶ resumed — applying answer: " + answer },
          { t: "cmd", s: "continuing task…" },
        ],
      }));
      this.flash(ag.name + " resumed");
    }
  }

  // ---- inbox pending action ----
  handleInbox(id: string, item: PendingItem, action: string) {
    const ag = this.agents().find((a) => a.id === id);
    const drop = () =>
      this.agents.update((prev) =>
        prev.map((a) =>
          a.id === id ? { ...a, pending: (a.pending || []).filter((p) => p.id !== item.id) } : a,
        ),
      );
    if (action === "allow" || action === "always") {
      drop();
      this.liveLogs.update((p) => ({
        ...p,
        [id]: [
          ...(p[id] || []),
          { t: "sys", s: (action === "always" ? "✓ always allow · " : "✓ allowed · ") + item.cmd },
          { t: "cmd", s: item.cmd },
        ],
      }));
      this.flash((action === "always" ? "always allow · " : "allowed · ") + (ag ? ag.name : id));
    } else if (action === "deny") {
      drop();
      this.liveLogs.update((p) => ({
        ...p,
        [id]: [...(p[id] || []), { t: "err", s: "✗ denied by user · " + item.cmd }],
      }));
      this.flash("denied · " + (ag ? ag.name : id));
    } else if (action === "merge") {
      drop();
      this.act(id, "merge");
    } else if (action === "diff") {
      this.openAgent(id, "diff");
    } else if (action === "open") {
      this.openAgent(id, "chat");
    }
  }

  // ---- spawn ----
  spawn(req: SpawnRequest) {
    this.spawnCount += 1;
    const name =
      SPAWN_NAMES[(this.spawnCount - 1) % SPAWN_NAMES.length] +
      (this.spawnCount > SPAWN_NAMES.length ? "-" + this.spawnCount : "");
    const id = "s" + this.spawnCount;
    const proj = this.projects().find((p) => p.id === req.projectId)!;
    const ag: Agent = {
      id,
      projectId: req.projectId,
      tool: req.toolId,
      model: req.model,
      effort: req.effort,
      name,
      task: req.prompt,
      status: "running",
      branch: "agent/" + name,
      worktree: proj.id + "-" + id,
      base: proj.head ?? "main",
      commits: 0,
      elapsed: 0,
      progress: 0.02,
      files: [],
      pending: [],
    };
    this.agents.update((prev) => [...prev, ag]);
    const bootLogs: LogLine[] = [
      { t: "sys", s: "allocating worktree → " + WORKTREE_ROOT + "/" + proj.id + "-" + id + " (" + toolMeta(req.toolId).name + ")" },
      { t: "cmd", s: "git worktree add ./" + proj.id + "-" + id + " -b agent/" + name + " " + req.branch },
      { t: "ok", s: "Preparing worktree (new branch 'agent/" + name + "' from " + req.branch + ")" },
      { t: "cmd", s: "cd " + proj.path + " && pnpm install" },
    ];
    LOGS[id] = bootLogs;
    STREAM[id] = [
      { t: "ok", s: "dependencies ready in 1.4s" },
      { t: "sys", s: "analyzing task: " + req.prompt.slice(0, 48) + (req.prompt.length > 48 ? "…" : "") },
      { t: "out", s: "scanning repository for relevant modules…" },
      { t: "cmd", s: "rg -l --type ts" },
      { t: "out", s: "drafting implementation plan (3 steps)" },
    ];
    this.streamIdx[id] = 0;
    this.liveLogs.update((p) => ({ ...p, [id]: bootLogs.slice() }));
    this.spawning.set(null);
    this.openAgent(id, "terminal");
    this.flash("spawned " + name + " in " + proj.name);
  }

  duplicateAgent(src: Agent) {
    this.spawn({
      projectId: src.projectId,
      branch: "main",
      toolId: src.tool,
      model: src.model,
      effort: src.effort ?? null,
      prompt: src.task,
    });
  }

  removeAgent(id: string) {
    const ag = this.agents().find((a) => a.id === id);
    this.agents.update((prev) => prev.filter((a) => a.id !== id));
    this.closeTab(id);
    this.flash("removed worktree " + (ag ? ag.name : id));
  }

  removeProject(id: string) {
    const p = this.projectOf(id);
    void this.projectsStore
      .remove(id)
      .then(() => this.flash("removed project " + (p ? p.name : id)));
    this.agents.update((prev) => prev.filter((a) => a.projectId !== id));
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
            ? e.message ?? "failed"
            : "add failed",
        ),
      );
    this.addingProject.set(false);
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
      { label: "Open workspace", icon: "enter", onClick: () => this.openAgent(id) },
      { label: "Open terminal", icon: "terminal", onClick: () => this.openAgent(id, "terminal") },
      { label: "View diff", icon: "diff", onClick: () => this.openAgent(id, "diff") },
      { sep: true },
      ag.status === "running"
        ? { label: "Pause agent", icon: "pause", onClick: () => this.act(id, "pause") }
        : { label: "Resume agent", icon: "play", disabled: ag.status === "done", onClick: () => this.act(id, "resume") },
      { label: "Commit changes", icon: "commit", disabled: !ag.files.length, onClick: () => this.act(id, "commit") },
      { label: "Push to origin", icon: "push", disabled: !ag.commits, onClick: () => this.act(id, "push") },
      { label: "Open pull request", icon: "pr", disabled: !ag.commits, onClick: () => this.act(id, "pr") },
      { label: "Merge → " + branchTarget, icon: "merge", accent: "var(--st-done)", disabled: !ag.commits, onClick: () => this.act(id, "merge") },
      { sep: true },
      { label: "Rename branch", icon: "rename", onClick: () => this.flash("rename " + ag.branch) },
      { label: "Duplicate agent", icon: "dup", onClick: () => this.duplicateAgent(ag) },
      { sep: true },
      { label: "Discard changes", icon: "discard", disabled: !ag.files.length, onClick: () => this.act(id, "discard") },
      { label: "Delete worktree", icon: "trash", danger: true, onClick: () => this.removeAgent(id) },
    ];
  }

  projectMenu(id: string): MenuItem[] {
    const p = this.projects().find((x) => x.id === id);
    if (!p) return [];
    return [
      { label: "Spawn agent here", icon: "plus", accent: "var(--accent)", onClick: () => this.spawning.set({ project: id }) },
      { sep: true },
      { label: "Pull latest", icon: "refresh", onClick: () => this.flash("pulled " + p.name) },
      { label: "Open in terminal", icon: "terminal", onClick: () => this.flash("opened terminal · " + p.path) },
      { label: "Copy path", icon: "dup", onClick: () => this.flash(p.path) },
      { sep: true },
      { label: "Remove project", icon: "trash", danger: true, onClick: () => this.removeProject(id) },
    ];
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
