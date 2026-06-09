import { computed, effect, inject, Injectable, signal } from "@angular/core";
import {
  ActivityKind,
  Agent,
  AgentNotification,
  FileNode,
  LogLine,
  PermissionQuestion,
  PermissionSuggestion,
} from "../models";
import { NotificationStore } from "../stores/notifications.store";
import { AgentsStore } from "../stores/agents.store";
import { TerminalService } from "../terminal.service";
import { UiStore } from "../ui/ui.store";
import { treeAgentIds } from "../workspace/pane-model";
import {
  appendPtyTail,
  detectTitleStatus,
  isAwaitingInput,
  isPermissionPrompt,
  TitleStatus,
} from "../utils";

/**
 * The live runtime layer for agents: a per-agent overlay of transient metrics
 * (elapsed / working / needsInput / worktree scans) merged over the backend
 * record, fed by the PTY output/title/exit streams. Also owns the liveness
 * tick and — for now — the heuristic that raises notifications (this detection
 * moves to the backend in a later step).
 */
@Injectable({ providedIn: "root" })
export class AgentRuntimeService {
  private agentsStore = inject(AgentsStore);
  private terminals = inject(TerminalService);
  private ui = inject(UiStore);
  private notifications = inject(NotificationStore);

  // backend agents merged with an in-memory runtime overlay (live metrics)
  private runtime = signal<Record<string, Partial<Agent>>>({});
  readonly agents = computed<Agent[]>(() => {
    const rt = this.runtime();
    return this.agentsStore.all().map((a) => {
      const o = rt[a.id];
      return o ? { ...a, ...o } : a;
    });
  });
  readonly liveLogs = signal<Record<string, LogLine[]>>({});
  readonly toolsAvailable = signal<Record<string, boolean>>({});
  // Per-agent ROLLING list of hook-driven activity entries — each
  // `{detail, event, kind}` where detail is the latest message content scraped
  // from the agent's transcript (assistant text / thinking / tool use) or the
  // structured tool summary, event is the precise hook that produced it
  // ("PreToolUse", "SessionStart", …), and kind classifies the line
  // (user/agent/tool/success/error/question/info) so the preview can colorize it.
  // Capped at the last 10 entries per agent; consecutive identical entries are
  // skipped. The card mirrors the LAST 3 entries as a single-line feed (oldest
  // first, newest last).
  readonly activity = signal<
    Record<string, { detail: string; event: string; kind: ActivityKind }[]>
  >({});

  // The agent the rest of the shell is "scoped" to: the focused pane's agent
  // within the active tab (a tab may tile several agents). Drives the sidebar
  // highlight and the right panel. Falls back to the tab's first agent.
  readonly activeAgent = computed<Agent | null>(() => {
    const tab = this.ui.activeTab();
    if (tab === "orchestrator") return null;
    const root = this.ui.paneRoots()[tab];
    if (!root) return null;
    const ids = treeAgentIds(root);
    if (!ids.length) return null;
    const scope = this.ui.scopeAgentId();
    const id = scope && ids.includes(scope) ? scope : ids[0];
    return this.agents().find((a) => a.id === id) ?? null;
  });

  // real liveness tracking (when launched, last output) + title-derived state
  private startedAt: Record<string, number> = {};
  private lastOutputAt: Record<string, number> = {};
  private titleStatus: Record<string, TitleStatus> = {};
  private titleAt: Record<string, number> = {};
  // notification edge-tracking + user-stop flag (a stop is not "work finished")
  private prevNeedsInput: Record<string, boolean> = {};
  private stoppingByUser: Record<string, boolean> = {};
  // backend hook status (working/idle) — authoritative over PTY title parsing
  private hookState: Record<string, string> = {};
  // agents whose worktree we've already set up watching + an initial scan for
  private watched = new Set<string>();

  /** Tools driven by native blocking hooks — their permission/question signals
   *  come from the backend, so the PTY heuristic must not also raise them. */
  private static readonly HOOK_TOOLS = new Set(["claude", "codex", "cursor"]);
  private hookDriven(tool: string): boolean {
    return AgentRuntimeService.HOOK_TOOLS.has(tool);
  }

  constructor() {
    // detect installed CLI tools once
    void this.agentsStore
      .detectTools()
      .then((list) =>
        this.toolsAvailable.set(Object.fromEntries(list.map((t) => [t.id, t.available]))),
      )
      .catch(() => {});

    // watch + initially scan EVERY agent's worktree (not just the active tab), so
    // background agents show live file-tree/diff too. Each id is set up once; the
    // backend keeps one watcher per agent.
    effect(() => {
      for (const a of this.agentsStore.all()) {
        if (this.watched.has(a.id)) continue;
        this.watched.add(a.id);
        this.loadChanges(a.id);
        this.loadCommits(a.id);
        this.loadFiles(a.id);
        void this.agentsStore.watch(a.id).catch(() => {});
      }
    });
    void this.agentsStore
      .onWorktreeChanged((id) => {
        this.loadFiles(id);
        this.loadChanges(id);
      })
      .catch(() => {});

    // live agent state from the terminal title (spinner = working, ✋ = needs input)
    this.terminals.onTitle((id, title) => {
      const s = detectTitleStatus(title);
      if (!s) return; // unrecognized title — keep last known state
      this.titleStatus[id] = s;
      this.titleAt[id] = Date.now();
    });

    // backend native-hook signals (authoritative for tools that support hooks):
    //  • permission → a held tool call; raise a notification carrying its requestId
    //  • status     → working/idle pings (override the PTY title heuristic)
    void this.agentsStore
      .onPermission((p) => this.onPermissionRequest(p))
      .catch(() => {});
    void this.agentsStore
      .onStatus((id, state) => {
        this.hookState[id] = state;
      })
      .catch(() => {});
    // (the agent's CLI session id is captured + persisted entirely in the backend
    // hook bridge, which re-emits agent://updated — so it arrives via the store's
    // normal entity-update path; no separate subscription needed here.)
    // activity feed: append each {detail, event, kind} entry, dedupe consecutive
    // duplicates, cap at the last 10 — drives the overview mini-term preview.
    void this.agentsStore
      .onActivity((id, detail, event, kind) => this.pushActivity(id, detail, event, kind))
      .catch(() => {});

    // stream output: raw bytes → xterm, plain-text tail → liveLogs (mini-term)
    void this.agentsStore
      .onOutput((id, chunk) => {
        this.lastOutputAt[id] = Date.now();
        this.terminals.write(id, chunk);
        this.liveLogs.update((m) => {
          const prev = (m[id] || []).map((l) => l.s);
          return { ...m, [id]: appendPtyTail(prev, chunk).map((s) => ({ t: "out" as const, s })) };
        });
      })
      .catch(() => {});
    void this.agentsStore
      .onExit((id) => this.onExit(id))
      .catch(() => {});

    setInterval(() => this.tick(), 800);
  }

  // ---- runtime overlay ----
  patchRuntime(id: string, patch: Partial<Agent>) {
    this.runtime.update((m) => ({ ...m, [id]: { ...(m[id] ?? {}), ...patch } }));
  }
  clearRuntime(id: string) {
    this.runtime.update((m) => {
      const { [id]: _drop, ...rest } = m;
      return rest;
    });
  }

  toolAvailable(id: string): boolean {
    return this.toolsAvailable()[id] !== false;
  }

  // ---- hook-driven activity (rolling last-10 message entries) ----
  /** Append an {detail, event, kind} entry for an agent, capped at the last 10.
   *  Skips a blank detail or one identical to the current last entry (the backend
   *  already dedups across hooks, but consecutive identical entries are dropped
   *  here too). */
  private pushActivity(id: string, detail: string, event: string, kind: ActivityKind) {
    const next = detail.trim();
    if (!next) return;
    this.activity.update((m) => {
      const list = m[id] ?? [];
      const last = list[list.length - 1];
      if (last && last.detail === next && last.event === event) return m;
      return { ...m, [id]: [...list, { detail: next, event, kind }].slice(-10) };
    });
  }
  /** The LAST 3 stored messages, each collapsed to ONE trimmed single line
   *  (first non-empty line of the detail) carrying its `kind`, in chronological
   *  order — oldest first, newest last. [] before any hook fires. The overview
   *  card renders this as a 3-message feed (newest at the bottom), colorized by
   *  kind. */
  activityFor(id: string): { text: string; kind: ActivityKind }[] {
    const list = this.activity()[id];
    if (!list?.length) return [];
    return list
      .slice(-3)
      .map((e) => ({
        text:
          e.detail
            .split("\n")
            .map((l) => l.trim())
            .find((l) => l.length > 0) ?? "",
        kind: e.kind,
      }))
      .filter((r) => r.text.length > 0);
  }
  /** The hook `event` of the latest activity entry (for logging), or null if none. */
  latestEvent(id: string): string | null {
    const list = this.activity()[id];
    return list?.[list.length - 1]?.event ?? null;
  }
  private clearActivity(id: string) {
    this.activity.update((m) => {
      const { [id]: _drop, ...rest } = m;
      return rest;
    });
  }

  // ---- worktree scans (async, superseded per worktree) ----
  private changesGen: Record<string, number> = {};
  loadChanges(agentId: string) {
    const gen = (this.changesGen[agentId] ?? 0) + 1;
    this.changesGen[agentId] = gen;
    this.patchRuntime(agentId, { git_changes: { loading: true, files: [] } });
    void this.agentsStore
      .changes(agentId)
      .then((files) => {
        if (this.changesGen[agentId] !== gen) return; // superseded
        this.patchRuntime(agentId, { git_changes: { loading: false, files } });
      })
      .catch(() => {
        if (this.changesGen[agentId] !== gen) return;
        this.patchRuntime(agentId, { git_changes: { loading: false, files: [] } });
      });
  }

  private commitsGen: Record<string, number> = {};
  loadCommits(agentId: string) {
    const gen = (this.commitsGen[agentId] ?? 0) + 1;
    this.commitsGen[agentId] = gen;
    const prev = this.runtime()[agentId]?.git_commits?.commits ?? [];
    this.patchRuntime(agentId, { git_commits: { loading: true, commits: prev } });
    void this.agentsStore
      .commits(agentId, 50, 0)
      .then((commits) => {
        if (this.commitsGen[agentId] !== gen) return; // superseded
        this.patchRuntime(agentId, { git_commits: { loading: false, commits } });
      })
      .catch(() => {
        if (this.commitsGen[agentId] !== gen) return;
        this.patchRuntime(agentId, { git_commits: { loading: false, commits: [] } });
      });
  }

  private filesGen: Record<string, number> = {};
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
      this.patchRuntime(agentId, { files: { loading: cur.loading, nodes: patch(cur.nodes) } });
    });
  }

  // ---- process lifecycle ----
  /** Launch (or, with `{resume:true}`, continue the captured CLI session via
   *  `claude --resume <sessionId>`) the agent's tool process. */
  startProcess(id: string, opts?: { resume?: boolean }) {
    const sz = this.terminals.size(id); // open the PTY at the visible terminal's size
    this.startedAt[id] = Date.now();
    delete this.titleStatus[id];
    delete this.titleAt[id];
    delete this.hookState[id];
    this.prevNeedsInput[id] = false;
    this.clearActivity(id); // a fresh run starts with an empty feed
    this.patchRuntime(id, { elapsed: 0, working: true, needsInput: false });
    void this.agentsStore
      .start(id, sz?.rows ?? 0, sz?.cols ?? 0, opts?.resume ?? false)
      .then(() => this.terminals.syncSize(id))
      .catch((e: { message?: string }) => this.ui.flash(e?.message ?? "start failed"));
  }
  stopProcess(id: string) {
    this.stoppingByUser[id] = true; // a user stop is not a "finished work" event
    void this.agentsStore.stop(id).catch(() => {});
  }

  /** Drop all overlay/terminal state for an agent (on removal). */
  dispose(id: string) {
    this.terminals.dispose(id);
    this.clearRuntime(id);
    delete this.startedAt[id];
    delete this.titleStatus[id];
    delete this.titleAt[id];
    delete this.hookState[id];
    delete this.prevNeedsInput[id];
    this.clearActivity(id);
    this.watched.delete(id);
  }

  // ---- live tick: real elapsed + working/needsInput, plus notification edges ----
  private tick() {
    const now = Date.now();
    this.agents().forEach((ag) => {
      if (ag.status !== "running") return;
      const elapsed = Math.max(0, Math.round((now - (this.startedAt[ag.id] ?? now)) / 1000));
      const { working, needsInput } = this.liveState(ag, now);
      if (ag.elapsed !== elapsed || ag.working !== working || ag.needsInput !== needsInput) {
        this.patchRuntime(ag.id, { elapsed, working, needsInput });
      }
      this.detectNeedsInput(ag, needsInput);
    });
  }

  /**
   * Live working/needsInput for an agent. Two disjoint sources — no overlap:
   *  • hooked tools (claude/codex/cursor): the backend is the single source of
   *    truth. working = a turn is in progress (status ping); needsInput = a held
   *    permission request is pending in the feed.
   *  • un-hooked tools (gemini): the PTY title/output heuristic — the fallback
   *    floor for tools we can't hook.
   */
  private liveState(ag: Agent, now: number): { working: boolean; needsInput: boolean } {
    const id = ag.id;
    const outputRecent = now - (this.lastOutputAt[id] ?? 0) < 1500;

    if (this.hookDriven(ag.tool)) {
      const hs = this.hookState[id];
      // status pings are event-driven (working at turn start, idle at Stop) so
      // they persist until the next ping; output recency only bridges the gap
      // before the first ping arrives.
      const working = hs === "working" || (hs === undefined && outputRecent);
      return { working, needsInput: this.hasPendingPermission(id) };
    }

    const ts = this.titleStatus[id];
    if (ts === "permission") return { working: false, needsInput: true };
    let working: boolean;
    if (ts === "working") {
      const titleFresh = now - (this.titleAt[id] ?? 0) < 1500;
      working = titleFresh || outputRecent;
    } else if (ts === "idle") {
      working = false;
    } else {
      working = outputRecent;
    }
    const needsInput = !working && isAwaitingInput(this.promptTail(id));
    return { working, needsInput };
  }

  /** Is a hook-driven permission request currently pending for this agent? */
  private hasPendingPermission(id: string): boolean {
    return this.notifications
      .pending()
      .some((n) => n.agentId === id && n.kind === "permission");
  }

  private onExit(id: string) {
    this.terminals.exit(id);
    delete this.startedAt[id];
    delete this.titleStatus[id];
    delete this.titleAt[id];
    delete this.hookState[id];
    delete this.prevNeedsInput[id];
    this.patchRuntime(id, { working: false, needsInput: false });
    this.notifications.dismissPendingFor(id, ["permission", "question"]);
    const ag = this.agents().find((a) => a.id === id);
    if (ag && ag.status !== "done") {
      void this.agentsStore.update(id, { status: "idle" }).catch(() => {});
    }
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
      [id]: [...(m[id] || []), { t: "sys" as const, s: "▪ process exited" }],
    }));
  }

  // ---- backend hook-driven needs-input signal (authoritative) ----
  // Carries the FULL permission detail now: tool + mode + command/description/
  // filePath + suggested settings rules, plus a human `summary` headline and, for
  // AskUserQuestion-style prompts, structured `questions` (header + options). We
  // keep a concise `detail` for the OS notification / collapsed views — preferring
  // the backend `summary` when present — and store the structured fields so the
  // card can render the full breakdown (display-only).
  private onPermissionRequest(p: {
    agentId: string;
    tool: string;
    mode?: string;
    command?: string;
    description?: string;
    filePath?: string;
    suggestions?: PermissionSuggestion[];
    summary?: string;
    questions?: PermissionQuestion[];
  }) {
    const ag = this.agents().find((a) => a.id === p.agentId);
    const name = ag?.name ?? "agent";
    const detail =
      p.summary || p.command || p.description || p.filePath || p.tool || "needs your input";
    const note = this.notifications.push({
      agentId: p.agentId,
      agentName: name,
      kind: "permission",
      title: `${name} needs your input`,
      detail,
      tool: p.tool,
      command: p.command,
      description: p.description,
      filePath: p.filePath,
      mode: p.mode,
      suggestions: p.suggestions,
      summary: p.summary,
      questions: p.questions,
    });
    if (note) void this.notifyOS(note);
  }

  // ---- PTY-parsing fallback (for tools WITHOUT native hooks, e.g. gemini) ----
  private detectNeedsInput(ag: Agent, needsInput: boolean) {
    // hook-driven tools get permission/question from the backend — don't double-raise
    if (this.hookDriven(ag.tool)) return;
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
  promptTail(id: string): string {
    return (this.liveLogs()[id] || [])
      .map((l) => l.s.trim())
      .filter((s) => s.length > 0)
      .slice(-5)
      .join("\n");
  }

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

  private async notifyOS(note: AgentNotification) {
    if (typeof document !== "undefined" && document.hasFocus()) return;
    try {
      const m = await import("@tauri-apps/plugin-notification");
      let granted = await m.isPermissionGranted();
      if (!granted) granted = (await m.requestPermission()) === "granted";
      if (granted) m.sendNotification({ title: note.title, body: note.detail || "tap to open" });
    } catch {
      // not under Tauri / plugin unavailable — in-app feed still has it
    }
  }
}
