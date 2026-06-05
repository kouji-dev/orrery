import { computed, effect, inject, Injectable, signal } from "@angular/core";
import { Agent, AgentNotification, FileNode, LogLine } from "../models";
import { NotificationStore } from "../stores/notifications.store";
import { AgentsStore } from "../stores/agents.store";
import { TerminalService } from "../terminal.service";
import { UiStore } from "../ui/ui.store";
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

  readonly activeAgent = computed<Agent | null>(() => {
    const id = this.ui.activeTab();
    if (id === "orchestrator") return null;
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
  private hookStateAt: Record<string, number> = {};

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

    // load the active agent's worktree transients (debounced via supersession)
    effect(() => {
      const id = this.ui.activeTab();
      if (id !== "orchestrator") {
        this.loadChanges(id);
        this.loadFiles(id);
        void this.agentsStore.watch(id).catch(() => {});
      }
    });
    void this.agentsStore
      .onWorktreeChanged((id) => {
        if (this.ui.activeTab() === id) {
          this.loadFiles(id);
          this.loadChanges(id);
        }
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
        this.hookStateAt[id] = Date.now();
      })
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
  startProcess(id: string) {
    const sz = this.terminals.size(id); // open the PTY at the visible terminal's size
    this.startedAt[id] = Date.now();
    delete this.titleStatus[id];
    delete this.titleAt[id];
    delete this.hookState[id];
    delete this.hookStateAt[id];
    this.prevNeedsInput[id] = false;
    this.patchRuntime(id, { elapsed: 0, working: true, needsInput: false });
    void this.agentsStore
      .start(id, sz?.rows ?? 0, sz?.cols ?? 0)
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
    delete this.hookStateAt[id];
    delete this.prevNeedsInput[id];
  }

  // ---- live tick: real elapsed + working/needsInput, plus notification edges ----
  private tick() {
    const now = Date.now();
    this.agents().forEach((ag) => {
      if (ag.status !== "running") return;
      const elapsed = Math.max(0, Math.round((now - (this.startedAt[ag.id] ?? now)) / 1000));
      const { working, needsInput } = this.liveState(ag.id, now);
      if (ag.elapsed !== elapsed || ag.working !== working || ag.needsInput !== needsInput) {
        this.patchRuntime(ag.id, { elapsed, working, needsInput });
      }
      this.detectNeedsInput(ag, needsInput);
    });
  }

  private liveState(id: string, now: number): { working: boolean; needsInput: boolean } {
    const outputRecent = now - (this.lastOutputAt[id] ?? 0) < 1500;
    // a hook status ping (working at turn start, idle at Stop) is authoritative
    // and event-driven, so it persists until the next ping — not time-decayed.
    const hs = this.hookState[id];
    if (hs === "working") return { working: true, needsInput: false };
    if (hs === "idle") return { working: outputRecent, needsInput: false };
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

  private onExit(id: string) {
    this.terminals.exit(id);
    delete this.startedAt[id];
    delete this.titleStatus[id];
    delete this.titleAt[id];
    delete this.hookState[id];
    delete this.hookStateAt[id];
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

  // ---- backend hook-driven permission requests (authoritative) ----
  private onPermissionRequest(p: {
    requestId: string;
    agentId: string;
    tool: string;
    detail: string;
  }) {
    const ag = this.agents().find((a) => a.id === p.agentId);
    const name = ag?.name ?? "agent";
    const note = this.notifications.push({
      agentId: p.agentId,
      agentName: name,
      kind: "permission",
      title: `${name} needs permission`,
      detail: p.detail,
      requestId: p.requestId,
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
