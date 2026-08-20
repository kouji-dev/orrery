import { computed, effect, inject, Injectable, signal } from "@angular/core";
import {
  ActivityKind,
  Agent,
  AgentNotification,
  PermissionQuestion,
  PermissionSuggestion,
  ToolDetection,
} from "../models";
import { NotificationAlertService } from "../notifications/notification-alert.service";
import { SettingsStore } from "../settings/settings.store";
import { NotificationStore } from "../stores/notifications.store";
import { AgentsStore } from "../stores/agents.store";
import { AgentWorkStore } from "./agent-work.store";
import { TerminalService } from "../terminal.service";
import { UiStore } from "../ui/ui.store";
import { treeAgentIds } from "../workspace/pane-model";
import { AgentPtyStatusPayload } from "../data-source/bridge";

/**
 * The live runtime layer for agents: a per-agent overlay of transient metrics
 * (working / needsInput / worktree scans) merged over the backend record, fed
 * by the PTY output/exit streams and the backend's hook + heuristic events.
 * Also owns the liveness tick and the shared elapsed clock (`now` /
 * `elapsedFor`). Status detection itself lives in the backend: hooks for
 * claude/codex/cursor, Rust PTY heuristics (A0.3, agent://pty-status) for
 * un-hooked tools — this service only edge-tracks and raises notifications.
 */
@Injectable({ providedIn: "root" })
export class AgentRuntimeService {
  private agentsStore = inject(AgentsStore);
  private work = inject(AgentWorkStore);
  private terminals = inject(TerminalService);
  private ui = inject(UiStore);
  private notifications = inject(NotificationStore);
  // settings-gated raise path: per-event toggles + native toast + sound cue
  private alerts = inject(NotificationAlertService);
  private settings = inject(SettingsStore);

  // backend agents merged with an in-memory runtime overlay (live metrics)
  private runtime = signal<Record<string, Partial<Agent>>>({});
  readonly agents = computed<Agent[]>(() => {
    const rt = this.runtime();
    return this.agentsStore.all().map((a) => {
      const o = rt[a.id];
      return o ? { ...a, ...o } : a;
    });
  });
  // Shared wall clock for elapsed displays. Advanced by the liveness tick ONLY
  // while at least one agent is running — and it is the ONLY thing the clock
  // writes. elapsed is deliberately NOT patched into the runtime overlay: that
  // rebuilt the agents array (new array + merged-object identities) every tick,
  // re-rendering every agents() consumer ~1.25x/s and already caused one bug
  // (diff-view refetch). Consumers derive elapsed locally via elapsedFor().
  readonly now = signal(Date.now());
  readonly toolsAvailable = signal<Record<string, boolean>>({});
  /** Full per-tool detection (path/version/status/reason), driving the Settings
   *  → Agent defaults runtime rows. Keyed by tool id. */
  readonly detections = signal<Record<string, ToolDetection>>({});
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

  // real liveness tracking (when launched, last output)
  private startedAt: Record<string, number> = {};
  // elapsed seconds captured at process exit — what elapsedFor() reports once
  // the run is over (cleared on the next start / on dispose)
  private finalElapsed: Record<string, number> = {};
  private lastOutputAt: Record<string, number> = {};
  // A0.3: Rust-side PTY heuristics state for un-hooked tools (gemini) —
  // pushed on transitions over agent://pty-status. Replaces the renderer's
  // title/promptTail parsing, which a none-mode agent (no bytes shipped)
  // would silently starve.
  private ptyState: Record<string, AgentPtyStatusPayload> = {};
  // notification edge-tracking + user-stop flag (a stop is not "work finished")
  private prevNeedsInput: Record<string, boolean> = {};
  private stoppingByUser: Record<string, boolean> = {};
  // liveness tick handle — started lazily when the first agent runs, cleared
  // when the last running agent exits.  null = not currently scheduled.
  private tickHandle: ReturnType<typeof setInterval> | null = null;
  // backend hook status (working/idle) — authoritative over PTY title parsing
  private hookState: Record<string, string> = {};
  // agents whose worktree we've already set up watching + an initial scan for
  private watched = new Set<string>();
  private scanReady = signal(false);

  /** Tools driven by native blocking hooks — their permission/question signals
   *  come from the backend, so the PTY heuristic must not also raise them. */
  private static readonly HOOK_TOOLS = new Set(["claude", "codex", "cursor"]);
  private hookDriven(tool: string): boolean {
    return AgentRuntimeService.HOOK_TOOLS.has(tool);
  }

  // A0.2 digest lines per agent (last ≤5 rendered rows, folded backend-side,
  // pushed at 1Hz over agent://digest) — the overview mini-terminals' feed.
  // Replaces reading the full-stream xterm buffer for previews.
  readonly digests = signal<Record<string, string[]>>({});

  constructor() {
    // detect installed CLI tools once (path + version + ok/error/missing)
    void this.refreshDetections();

    // Watch EVERY agent's worktree — the backend watcher runs the initial scan
    // and pushes results (changes + HEAD), so no eager pull is needed here.
    // Tree + commits stay lazy — ensured on first open below.
    effect(() => {
      // Why: the backend pushes an initial scan on watch registration — don't register until the onScan listener is live, or that push is lost.
      if (!this.scanReady()) return;
      for (const a of this.agentsStore.all()) {
        if (this.watched.has(a.id)) continue;
        this.watched.add(a.id);
        void this.agentsStore.watch(a.id).catch(() => {});
      }
    });
    // Lazy: first time an agent becomes the active/scoped one, load its tree +
    // first commits page (no-ops when already loaded).
    effect(() => {
      const ag = this.activeAgent();
      if (!ag) return;
      this.work.ensureTree(ag.id);
      this.work.ensureCommits(ag.id);
    });
    void this.agentsStore
      .onScan((p) => this.work.applyScan(p.id, p.changes, p.head, p.countsFull ?? true))
      .then(() => this.scanReady.set(true))
      .catch(() => this.scanReady.set(true));

    // A0.3: PTY-derived heuristics for un-hooked tools (gemini) arrive from
    // Rust as transition events — title parsing + prompt-tail folding moved
    // next to the batcher (see runtime/heuristics.rs). Works regardless of the
    // agent's interest mode, since it never depends on bytes reaching us.
    void this.agentsStore
      .onPtyStatus((p) => this.onPtyStatus(p))
      .catch(() => {});
    // A0.2 digest lines for overview mini-previews (1Hz, ≤5 lines per agent).
    void this.agentsStore
      .onDigest((entries) => {
        this.digests.update((m) => {
          const next = { ...m };
          for (const e of entries) {
            if (!this.agentsStore.all().some((a) => a.id === e.id)) continue;
            next[e.id] = e.lines;
          }
          return next;
        });
      })
      .catch(() => {});

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

    // stream output: raw bytes → xterm (scheduler-paced). Only STREAM-mode
    // agents ship frames (A0.2); `seq` rides along so the terminal service can
    // dedup against a replayed A1.2 snapshot.
    // The payload is one multiplexed ~16ms frame: [{id, chunk, seq}, …] with
    // one coalesced entry per agent that produced output during the frame.
    void this.agentsStore
      .onOutput((entries) => {
        const now = Date.now();
        for (const { id, chunk, seq } of entries) {
          // Why: the mux's exit force-drain can land after agent removal —
          // writing then would recreate (and leak) a disposed terminal.
          if (!this.agentsStore.all().some((a) => a.id === id)) continue;
          this.lastOutputAt[id] = now;
          this.terminals.write(id, chunk, seq);
        }
      })
      .catch(() => {});
    void this.agentsStore
      .onExit((id) => this.onExit(id))
      .catch(() => {});

    // Tick timer is started lazily on the first run transition and cleared when
    // the last running agent exits (see ensureTicking / stopTicking below).
    // Arm immediately if there are already running agents (app loaded mid-session).
    if (this.agentsStore.all().some((a) => a.status === "running")) {
      this.ensureTicking();
    }

    // Settings-driven auto-resume — fully async (awaits the persisted settings
    // AND the initial agent list), so nothing blocks the constructor hot path.
    void this.autoResume();
  }

  /** When settings `autoResume` is on, relaunch the agents the backend captured
   *  as running at the last shutdown (a ONE-SHOT drain — a frontend reload can't
   *  double-launch), continuing each agent's recorded CLI session.
   *
   *  An update relaunch ("Install & relaunch") always resumes, setting or not:
   *  the user didn't choose to stop those terminals, the updater did. Its ids
   *  come from the drained update-resume list (UiStore), NOT from the backend —
   *  the update path exits gracefully, so InterruptedAgents stays empty. */
  private async autoResume(): Promise<void> {
    try {
      const s = await this.settings.ready();
      const updateIds = this.ui.updateResumeIds ?? [];
      if (!s.autoResume && !updateIds.length) return;
      await this.agentsStore.ready();
      const interrupted = s.autoResume ? await this.agentsStore.interrupted() : [];
      for (const id of new Set([...interrupted, ...updateIds])) {
        const ag = this.agentsStore.all().find((a) => a.id === id);
        // Only a captured session can be CONTINUED — relaunching a session-less
        // agent would restart its task from scratch, which resume never promises.
        if (ag?.sessionId) this.startProcess(id, { resume: true });
      }
    } catch {
      // backend unavailable (plain ng serve) — no auto-resume
    }
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

  /** One tool's full detection (or null before detection completes). */
  detection(id: string): ToolDetection | null {
    return this.detections()[id] ?? null;
  }

  /** Re-run backend detection for all tools (honors saved manual path
   *  overrides). Called on startup and after a path is set/reverted. */
  async refreshDetections(): Promise<void> {
    try {
      const list = await this.agentsStore.detectTools();
      this.applyDetections(list);
    } catch {
      // backend unavailable (plain `ng serve`) — leave tools optimistically on
    }
  }

  /** Probe a candidate path for a tool (`<path> --version`). Pure — does NOT
   *  mutate `detections` (a failed probe shouldn't clobber the known state); the
   *  caller folds the result in via {@link setDetection} only when it's good. */
  verifyToolPath(id: string, path: string): Promise<ToolDetection> {
    return this.agentsStore.verifyToolPath(id, path);
  }

  /** Replace one tool's detection (after a successful verify/save). */
  setDetection(id: string, det: ToolDetection): void {
    this.detections.update((m) => ({ ...m, [id]: det }));
    this.toolsAvailable.update((m) => ({ ...m, [id]: det.available }));
  }

  private applyDetections(list: ToolDetection[]): void {
    this.detections.set(Object.fromEntries(list.map((t) => [t.id, t])));
    this.toolsAvailable.set(Object.fromEntries(list.map((t) => [t.id, t.available])));
  }

  /** Elapsed seconds for an agent, derived from the shared clock: live while
   *  the process runs (now − startedAt), frozen at the exit-time value after
   *  it ends, 0 before any run. Reading this subscribes to `now` — so on a
   *  tick ONLY the elapsed text re-renders, never the agents array. */
  elapsedFor(id: string): number {
    const now = this.now();
    const started = this.startedAt[id];
    if (started === undefined) return this.finalElapsed[id] ?? 0;
    return Math.max(0, Math.round((now - started) / 1000));
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

  // ---- process lifecycle ----
  /** Launch (or, with `{resume:true}`, continue the captured CLI session via
   *  `claude --resume <sessionId>`) the agent's tool process. */
  startProcess(id: string, opts?: { resume?: boolean }) {
    const sz = this.terminals.size(id); // open the PTY at the visible terminal's size
    this.startedAt[id] = Date.now();
    delete this.finalElapsed[id]; // a fresh run derives elapsed from startedAt again
    this.now.set(Date.now()); // the clock may have been parked — show 0s immediately
    delete this.ptyState[id];
    delete this.hookState[id];
    this.prevNeedsInput[id] = false;
    this.clearActivity(id); // a fresh run starts with an empty feed
    this.patchRuntime(id, { working: true, needsInput: false });
    this.ensureTicking(); // arm the liveness timer if it wasn't already running
    void this.agentsStore
      .start(id, sz?.rows ?? 0, sz?.cols ?? 0, opts?.resume ?? false)
      .then(() => this.terminals.syncSize(id))
      .catch((e: { message?: string }) => {
        // The run never began — drop its startedAt (set optimistically above) so
        // elapsedFor() doesn't count forever, and park the tick timer when this
        // was the only would-be live run (mirrors the onExit cleanup).
        delete this.startedAt[id];
        if (Object.keys(this.startedAt).length === 0) this.stopTicking();
        this.ui.flash(e?.message ?? "start failed");
      });
  }
  stopProcess(id: string) {
    this.stoppingByUser[id] = true; // a user stop is not a "finished work" event
    void this.agentsStore.stop(id).catch(() => {});
  }

  /** Drop all overlay/terminal state for an agent (on removal). */
  dispose(id: string) {
    this.terminals.dispose(id);
    this.clearRuntime(id);
    this.work.dispose(id);
    this.work.dropTotals(id);
    delete this.startedAt[id];
    delete this.finalElapsed[id];
    delete this.ptyState[id];
    this.digests.update((m) => {
      const { [id]: _drop, ...rest } = m;
      return rest;
    });
    delete this.hookState[id];
    delete this.prevNeedsInput[id];
    delete this.stoppingByUser[id]; // exit may never arrive (guarded) — clear here too
    this.clearActivity(id);
    this.watched.delete(id);
  }

  // ---- lazy tick timer management ----
  /** Start the 800 ms liveness timer if it is not already running. */
  private ensureTicking() {
    if (this.tickHandle !== null) return;
    this.tickHandle = setInterval(() => this.tick(), 800);
  }
  /** Clear the liveness timer — no-op when already stopped. */
  private stopTicking() {
    if (this.tickHandle === null) return;
    clearInterval(this.tickHandle);
    this.tickHandle = null;
  }

  // ---- live tick: working/needsInput edges + the shared elapsed clock ----
  // Runtime state is only patched on a REAL transition, so across plain clock
  // ticks agents() keeps its array and object identities (no consumer churn).
  private tick() {
    const now = Date.now();
    let anyRunning = false;
    this.agents().forEach((ag) => {
      if (ag.status !== "running") return;
      anyRunning = true;
      const { working, needsInput } = this.liveState(ag, now);
      if (ag.working !== working || ag.needsInput !== needsInput) {
        this.patchRuntime(ag.id, { working, needsInput });
      }
      this.detectNeedsInput(ag, needsInput);
    });
    // Use startedAt as the authoritative live-run set for the clock advance — it
    // is always current even when the backing store's status signal hasn't caught
    // up yet (the store only updates asynchronously via backend events).
    const hasLiveRun = Object.keys(this.startedAt).length > 0;
    if (anyRunning || hasLiveRun) {
      // advance the shared clock only while something runs — elapsed displays
      // update through elapsedFor() without touching runtime state.
      this.now.set(now);
    } else {
      // Nothing running — self-clear so the JS timer doesn't keep waking 1.25×/s
      // for no reason.  ensureTicking() re-arms it when a process starts again.
      this.stopTicking();
    }
  }

  /**
   * Live working/needsInput for an agent. Two disjoint sources — no overlap:
   *  • hooked tools (claude/codex/cursor): the backend is the single source of
   *    truth. working = a turn is in progress (status ping); needsInput = a held
   *    permission request is pending in the feed.
   *  • un-hooked tools (gemini): the Rust-side PTY heuristics (A0.3), pushed
   *    over agent://pty-status on transitions. Before the first event lands,
   *    output recency bridges the gap (stream-mode agents only — hidden ones
   *    get their first pty-status within ~2s of launch output anyway).
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

    const ps = this.ptyState[id];
    if (ps) return { working: ps.working, needsInput: ps.needsInput };
    return { working: outputRecent, needsInput: false };
  }

  /** Is a hook-driven permission request currently pending for this agent? */
  private hasPendingPermission(id: string): boolean {
    return this.notifications
      .pending()
      .some((n) => n.agentId === id && n.kind === "permission");
  }

  private onExit(id: string) {
    // Why: like the output path, exit can land after removeAgent (mux drain
    // ordering) — patching the overlay / pushing the tail below would recreate
    // (and leak) runtime entries for a disposed agent.
    if (!this.agentsStore.all().some((a) => a.id === id)) return;
    this.terminals.exit(id);
    // freeze the elapsed display at the run's final duration
    const started = this.startedAt[id];
    if (started !== undefined) {
      this.finalElapsed[id] = Math.max(0, Math.round((Date.now() - started) / 1000));
    }
    delete this.startedAt[id];
    // Stop the liveness timer when this was the last in-flight process.
    // startedAt is the authoritative set of live runs: if it is now empty, no
    // ticks are needed.  The tick's own self-clear is a backstop; stopping here
    // avoids waiting for the next 800ms boundary.
    if (Object.keys(this.startedAt).length === 0) this.stopTicking();
    // ptyState is kept (not deleted) so the "finished" notification below and
    // any later promptTail() read still see the run's final prompt tail; a
    // fresh start clears it.
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
    this.alerts.raise({
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
  }

  // ---- Rust PTY-heuristics fallback (tools WITHOUT native hooks, gemini) ----
  // A0.3: the classification (permission vs question) and the prompt-tail
  // detail now arrive READY from the backend event — the renderer only does
  // edge-tracking so a prompt raises once and dismisses when answered.
  private detectNeedsInput(ag: Agent, needsInput: boolean) {
    // hook-driven tools get permission/question from the backend — don't double-raise
    if (this.hookDriven(ag.tool)) return;
    const was = this.prevNeedsInput[ag.id] ?? false;
    if (needsInput && !was) {
      const ps = this.ptyState[ag.id];
      const permission = ps?.permission ?? false;
      this.raise({
        agentId: ag.id,
        agentName: ag.name,
        kind: permission ? "permission" : "question",
        title: permission ? `${ag.name} needs permission` : `${ag.name} has a question`,
        detail: ps?.detail ?? "",
      });
    } else if (!needsInput && was) {
      this.notifications.dismissPendingFor(ag.id, ["permission", "question"]);
    }
    this.prevNeedsInput[ag.id] = needsInput;
  }

  /** A0.3 event handler: store the Rust heuristics state and reflect the
   *  working/needsInput edge into the runtime overlay immediately (the 800ms
   *  tick would pick it up anyway; this just removes the lag). */
  private onPtyStatus(p: AgentPtyStatusPayload) {
    // Same guard as output/exit: events can race agent removal.
    const ag = this.agents().find((a) => a.id === p.id);
    if (!ag) return;
    this.ptyState[p.id] = p;
    if (ag.status === "running") {
      if (ag.working !== p.working || ag.needsInput !== p.needsInput) {
        this.patchRuntime(p.id, { working: p.working, needsInput: p.needsInput });
      }
      this.detectNeedsInput(ag, p.needsInput);
    }
  }

  /** The backend-folded prompt tail (last few non-empty lines) for un-hooked
   *  tools — notification/exit context. "" before the first pty-status event. */
  promptTail(id: string): string {
    return this.ptyState[id]?.detail ?? "";
  }

  /** A0.2 digest lines (last ≤5 rendered rows) for an agent's mini-preview.
   *  [] before the first digest arrives (or for non-digest-mode agents). */
  digestFor(id: string): string[] {
    return this.digests()[id] ?? [];
  }

  /** All notifications go through the settings-gated alert service: per-event
   *  toggles can drop the raise entirely; the master toggle adds the native
   *  toast + sound cue (see NotificationAlertService for the full policy). */
  private raise(input: {
    agentId: string;
    agentName: string;
    kind: AgentNotification["kind"];
    title: string;
    detail: string;
  }) {
    this.alerts.raise(input);
  }
}
