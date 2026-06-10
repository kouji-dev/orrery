import { computed, effect, inject, Injectable, signal } from "@angular/core";
import {
  ActivityKind,
  Agent,
  AgentNotification,
  PermissionQuestion,
  PermissionSuggestion,
} from "../models";
import { NotificationAlertService } from "../notifications/notification-alert.service";
import { SettingsStore } from "../settings/settings.store";
import { NotificationStore } from "../stores/notifications.store";
import { AgentsStore } from "../stores/agents.store";
import { AgentWorkStore } from "./agent-work.store";
import { TerminalService } from "../terminal.service";
import { UiStore } from "../ui/ui.store";
import { treeAgentIds } from "../workspace/pane-model";
import { detectTitleStatus, isAwaitingInput, isPermissionPrompt, TitleStatus } from "../utils";
import { createPtyTailBuffer } from "./pty-tail-buffer";

/**
 * The live runtime layer for agents: a per-agent overlay of transient metrics
 * (working / needsInput / worktree scans) merged over the backend record, fed
 * by the PTY output/title/exit streams. Also owns the liveness tick, the
 * shared elapsed clock (`now` / `elapsedFor`) and — for now — the heuristic
 * that raises notifications (this detection moves to the backend in a later
 * step).
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
  // elapsed seconds captured at process exit — what elapsedFor() reports once
  // the run is over (cleared on the next start / on dispose)
  private finalElapsed: Record<string, number> = {};
  private lastOutputAt: Record<string, number> = {};
  private titleStatus: Record<string, TitleStatus> = {};
  private titleAt: Record<string, number> = {};
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

  // Raw PTY tail, folded LAZILY: a chunk is just appended to a bounded
  // per-agent ring (no parsing); appendPtyTail runs only when promptTail() is
  // actually read — at process exit and in the needs-input heuristic for
  // un-hooked tools (gemini). Hook-driven tools never pay for the fold while
  // streaming.
  private tailBuf = createPtyTailBuffer();

  constructor() {
    // detect installed CLI tools once
    void this.agentsStore
      .detectTools()
      .then((list) =>
        this.toolsAvailable.set(Object.fromEntries(list.map((t) => [t.id, t.available]))),
      )
      .catch(() => {});

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
      .onScan((p) => this.work.applyScan(p.id, p.changes, p.head))
      .then(() => this.scanReady.set(true))
      .catch(() => this.scanReady.set(true));

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

    // stream output: raw bytes → xterm (scheduler-paced), and the SAME raw
    // string into the lazy tail ring (no folding here — see tailBuf above).
    // The payload is one multiplexed ~16ms frame: [{id, chunk, seq}, …] with
    // one coalesced entry per agent that produced output during the frame.
    void this.agentsStore
      .onOutput((entries) => {
        const now = Date.now();
        for (const { id, chunk } of entries) {
          // Why: the mux's exit force-drain can land after agent removal —
          // writing then would recreate (and leak) a disposed terminal.
          if (!this.agentsStore.all().some((a) => a.id === id)) continue;
          this.lastOutputAt[id] = now;
          this.terminals.write(id, chunk);
          this.tailBuf.push(id, chunk);
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
   *  double-launch), continuing each agent's recorded CLI session. */
  private async autoResume(): Promise<void> {
    try {
      const s = await this.settings.ready();
      if (!s.autoResume) return;
      await this.agentsStore.ready();
      const ids = await this.agentsStore.interrupted();
      for (const id of ids) {
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
    delete this.titleStatus[id];
    delete this.titleAt[id];
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
    this.tailBuf.clear(id);
    delete this.startedAt[id];
    delete this.finalElapsed[id];
    delete this.titleStatus[id];
    delete this.titleAt[id];
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
    // Why: like the output path, exit can land after removeAgent (mux drain
    // ordering) — patching the overlay / pushing the tail below would recreate
    // (and leak) runtime + tailBuf entries for a disposed agent.
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
    // exited marker AFTER the notification read its tail — so the "finished"
    // detail excludes it but any later promptTail() read includes it.
    this.tailBuf.push(id, "\r\n▪ process exited");
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

  /** Last few non-empty terminal lines — the prompt context for a notification.
   *  This is THE read that triggers the lazy fold of buffered raw chunks. */
  promptTail(id: string): string {
    return this.tailBuf
      .tail(id)
      .map((s) => s.trim())
      .filter((s) => s.length > 0)
      .slice(-5)
      .join("\n");
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
