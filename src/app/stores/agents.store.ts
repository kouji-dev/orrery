import { inject, Injectable } from "@angular/core";
import {
  AgentDigestEntry,
  AgentOutputEntry,
  AgentPtyStatusPayload,
  BRIDGE,
  Commands,
  Events,
  InterestEntry,
  RuntimeSnapshot,
} from "../data-source/bridge";
import {
  ActivityKind,
  Agent,
  AgentFile,
  Commit,
  ConflictFile,
  FileDiff,
  FileNode,
  GitSessionState,
  MergeSession,
  PermissionQuestion,
  PermissionSuggestion,
  ToolDetection,
} from "../models";
import { bindFacade } from "../state/entity-facade";
import { createEntityStore } from "../state/entity-store";

/**
 * Backend-backed source of truth for agent identity/config/status.
 * Live runtime metrics (working/needsInput/files) are an overlay kept in
 * AgentRuntimeService and merged over these records; elapsed time is derived
 * there from a shared clock (elapsedFor) and never written into the records.
 */
@Injectable({ providedIn: "root" })
export class AgentsStore {
  private bridge = inject(BRIDGE);
  private store = createEntityStore<Agent>((a) => a.id);

  readonly all = this.store.all;
  readonly loading = this.store.loading;

  private facade = bindFacade(this.store, this.bridge, {
    listCommand: Commands.AgentList,
    events: {
      created: Events.AgentCreated,
      updated: Events.AgentUpdated,
      deleted: Events.AgentDeleted,
    },
  });

  private readonly loadPromise: Promise<void>;

  constructor() {
    this.loadPromise = this.init();
  }
  private async init() {
    try {
      await this.facade.listen();
      await this.facade.load();
    } catch {
      // backend unavailable — start empty
    }
  }

  /** Resolves once the initial agent list landed (or the load failed and the
   *  store stays empty) — the auto-resume flow awaits this before matching ids. */
  ready(): Promise<void> {
    return this.loadPromise;
  }

  /** Ids of the agents that were running when the app last shut down. ONE-SHOT:
   *  the backend drains its snapshot on read, so a frontend reload can't
   *  relaunch the same agents twice. */
  interrupted(): Promise<string[]> {
    return this.bridge.invoke<string[]>(Commands.AgentsInterrupted);
  }

  // ---- mutations: invoke only; the store updates from agent:// events ----
  spawn(req: {
    projectId: string;
    tool: string;
    model: string;
    effort: string | null;
    name: string;
    task: string;
    base: string;
    ticketId?: string;
  }): Promise<Agent> {
    return this.bridge.invoke<Agent>(Commands.AgentSpawn, { req });
  }
  update(
    id: string,
    patch: { status?: string; task?: string; model?: string; name?: string },
  ): Promise<Agent> {
    return this.bridge.invoke<Agent>(Commands.AgentUpdate, { id, req: patch });
  }
  /** Drop an agent. `hard` also deletes its worktree folder from disk —
   *  opt-in, because the folder holds any uncommitted work. */
  async remove(id: string, hard = false): Promise<void> {
    await this.bridge.invoke(Commands.AgentRemove, { id, hard });
  }
  /** Per-tool detection (resolved path + version + ok/error/missing status). */
  detectTools(): Promise<ToolDetection[]> {
    return this.bridge.invoke(Commands.DetectTools);
  }
  /** Probe a candidate executable path for a tool (`<path> --version`). */
  verifyToolPath(id: string, path: string): Promise<ToolDetection> {
    return this.bridge.invoke(Commands.VerifyToolPath, { id, path });
  }
  /** Worktree file tree (source recursed, ignored dirs as lazy stubs). */
  tree(id: string): Promise<FileNode[]> {
    return this.bridge.invoke<FileNode[]>(Commands.AgentTree, { id });
  }
  /** Immediate children of one directory — to lazily expand an unloaded folder. */
  listDir(id: string, path: string): Promise<FileNode[]> {
    return this.bridge.invoke<FileNode[]>(Commands.AgentDir, { id, path });
  }
  /** Working-tree changes in the agent's worktree (git status). */
  changes(id: string): Promise<AgentFile[]> {
    return this.bridge.invoke<AgentFile[]>(Commands.AgentChanges, { id });
  }
  /** Commits on the agent's branch (worktree HEAD log), tagged with the agent id.
   *  Paged: `limit` newest-first commits starting at `offset`. */
  commits(id: string, limit: number, offset = 0): Promise<Commit[]> {
    return this.bridge.invoke<Commit[]>(Commands.AgentCommits, { id, limit, offset });
  }
  /** Commit selected paths (empty = all) in the worktree; resolves the short sha. */
  commit(id: string, message: string, paths: string[]): Promise<string> {
    return this.bridge.invoke<string>(Commands.AgentCommit, { id, message, paths });
  }
  /** Discard selected paths (empty = all) in the worktree. */
  discard(id: string, paths: string[]): Promise<void> {
    return this.bridge.invoke(Commands.AgentDiscard, { id, paths });
  }
  /** Deterministic backend push of the agent's branch to origin. */
  push(id: string): Promise<void> {
    return this.bridge.invoke(Commands.AgentPush, { id });
  }
  /** AI-driven completion action: type the predefined prompt for `kind`
   *  (commit/push/rebase/merge) into the agent's running PTY. */
  action(id: string, kind: "commit" | "push" | "rebase" | "merge"): Promise<void> {
    return this.bridge.invoke(Commands.AgentAction, { id, kind });
  }
  // ---- native merge + conflict session (A3.5 / A3.6) ----
  /** Native merge of `branch` into the agent's branch. Empty `conflicts` =
   *  merged clean (or FF/up to date); non-empty = session now in progress. */
  merge(id: string, branch: string): Promise<MergeSession> {
    return this.bridge.invoke<MergeSession>(Commands.AgentMerge, { id, branch });
  }
  /** Still-conflicted files of the in-progress session. */
  conflicts(id: string): Promise<ConflictFile[]> {
    return this.bridge.invoke<ConflictFile[]>(Commands.AgentConflicts, { id });
  }
  /** Write `content` as the resolution of `path` and stage it. */
  conflictResolve(id: string, path: string, content: string): Promise<void> {
    return this.bridge.invoke(Commands.AgentConflictResolve, { id, path, content });
  }
  /** Abort the in-progress merge (drop state, hard-reset to HEAD). */
  mergeAbort(id: string): Promise<void> {
    return this.bridge.invoke(Commands.AgentMergeAbort, { id });
  }
  /** Commit the fully-resolved merge → short sha. */
  mergeContinue(id: string, message?: string): Promise<string> {
    return this.bridge.invoke<string>(Commands.AgentMergeContinue, { id, message });
  }
  /** Merge/rebase/cherry-pick in progress? + remaining conflict count. */
  sessionState(id: string): Promise<GitSessionState> {
    return this.bridge.invoke<GitSessionState>(Commands.AgentSessionState, { id });
  }
  /** Old/new content of a file for the diff view. `oldPath` (the pre-move path)
   *  is passed for renamed/moved files so the OLD side reads the right content. */
  diff(id: string, path: string, oldPath?: string): Promise<FileDiff> {
    return this.bridge.invoke<FileDiff>(Commands.AgentDiff, { id, path, oldPath });
  }
  /** Start watching an agent's worktree for changes (replaces any previous watch). */
  watch(id: string): Promise<void> {
    return this.bridge.invoke(Commands.AgentWatch, { id });
  }
  /** Subscribe to backend worktree scan pushes — the watcher computes the
   *  changes + HEAD oid and ships them with the notification (no pull needed). */
  onScan(
    cb: (p: { id: string; changes: AgentFile[]; head: string | null; countsFull?: boolean }) => void,
  ): Promise<() => void> {
    return this.bridge.on<{ id: string; changes: AgentFile[]; head: string | null; countsFull?: boolean }>(
      Events.AgentChanged,
      cb,
    );
  }

  /** Launch the agent's tool process (PTY-streamed), sized to the visible terminal.
   *  `resume` requests a "Continue session" launch (`claude --resume <sessionId>`)
   *  when the agent has a captured session id; defaults to a normal Start/Resume. */
  start(id: string, rows = 0, cols = 0, resume = false): Promise<void> {
    return this.bridge.invoke(Commands.AgentStart, { id, rows, cols, resume });
  }
  /** Stop the agent's running process. */
  stop(id: string): Promise<void> {
    return this.bridge.invoke(Commands.AgentStop, { id });
  }
  /** Forward terminal keystrokes into the agent's PTY stdin. */
  input(id: string, data: string): Promise<void> {
    return this.bridge.invoke(Commands.AgentInput, { id, data });
  }
  /** Approve the agent's pending permission prompt (tool-correct keystrokes). */
  allow(id: string): Promise<void> {
    return this.bridge.invoke(Commands.AgentAllow, { id });
  }
  /** Deny the agent's pending permission prompt (tool-correct keystrokes). */
  deny(id: string): Promise<void> {
    return this.bridge.invoke(Commands.AgentDeny, { id });
  }
  /** Select option `choice` (1-based) in the agent's pending numbered SELECT
   *  prompt (best-effort PTY keystrokes; assumes a numbered TUI select). */
  decide(id: string, choice: number): Promise<void> {
    return this.bridge.invoke(Commands.AgentDecide, { id, choice });
  }
  /** Resize the agent's PTY to match the visible terminal. */
  resize(id: string, rows: number, cols: number): Promise<void> {
    return this.bridge.invoke(Commands.AgentResize, { id, rows, cols });
  }
  /** A0.2: publish the full interest set (supersedes the old focus() /
   *  agent_focus). Entries absent from the set are `none` — the backend ships
   *  NOTHING for them (their PTYs keep being read into the bounded ring). */
  subscribe(entries: InterestEntry[]): Promise<void> {
    return this.bridge.invoke(Commands.RuntimeSubscribe, { entries });
  }
  /** A1.2: the backend scrollback snapshot for an agent — replayed into a
   *  stale/reloaded terminal; live chunks with seq <= endSeq are dupes. */
  snapshot(id: string): Promise<RuntimeSnapshot> {
    return this.bridge.invoke<RuntimeSnapshot>(Commands.RuntimeSnapshot, { id });
  }
  /** Subscribe to 1Hz digests (last rendered lines) for digest-mode agents. */
  onDigest(cb: (entries: AgentDigestEntry[]) => void): Promise<() => void> {
    return this.bridge.on<AgentDigestEntry[]>(Events.AgentDigest, cb);
  }
  /** Subscribe to Rust-side PTY status heuristics (un-hooked tools — A0.3). */
  onPtyStatus(cb: (p: AgentPtyStatusPayload) => void): Promise<() => void> {
    return this.bridge.on<AgentPtyStatusPayload>(Events.AgentPtyStatus, cb);
  }
  /** Subscribe to streamed process output. The backend multiplexes EVERY
   *  agent's PTY output into one `agent://output` event per ~16ms frame; the
   *  payload is an array with one coalesced `{id, chunk, seq}` entry per agent
   *  that produced output during the frame. */
  onOutput(cb: (entries: AgentOutputEntry[]) => void): Promise<() => void> {
    return this.bridge.on<AgentOutputEntry[]>(Events.AgentOutput, cb);
  }
  /** Subscribe to process-exit events. */
  onExit(cb: (id: string) => void): Promise<() => void> {
    return this.bridge.on<{ id: string }>(Events.AgentExit, (p) => cb(p.id));
  }

  /** Subscribe to hook-driven needs-input signals (the agent wants the user).
   *  Carries the full permission detail: tool + mode + command/description/
   *  filePath + suggested settings rules (suggestions is [] for non-Claude tools),
   *  plus a human `summary` headline and, for AskUserQuestion-style prompts, the
   *  structured `questions` (each with optional header + options). */
  onPermission(
    cb: (p: {
      agentId: string;
      tool: string;
      mode?: string;
      command?: string;
      description?: string;
      filePath?: string;
      suggestions?: PermissionSuggestion[];
      summary?: string;
      questions?: PermissionQuestion[];
    }) => void,
  ): Promise<() => void> {
    return this.bridge.on(Events.AgentPermission, cb);
  }
  /** Subscribe to non-blocking hook status pings (working / idle). */
  onStatus(cb: (id: string, state: string) => void): Promise<() => void> {
    return this.bridge.on<{ id: string; state: string }>(Events.AgentStatus, (p) =>
      cb(p.id, p.state),
    );
  }
  /** Subscribe to action-carrying activity from hooks (e.g. "Bash: npm test").
   *  The payload carries the precise hook `event` (e.g. "PreToolUse") so the
   *  runtime can log/branch on which hook produced the detail, plus a `kind`
   *  (user/agent/tool/success/error/question/info) used to colorize the preview. */
  onActivity(
    cb: (id: string, detail: string, event: string, kind: ActivityKind) => void,
  ): Promise<() => void> {
    return this.bridge.on<{
      agentId: string;
      tool: string;
      event: string;
      kind: ActivityKind;
      detail: string;
    }>(Events.AgentActivity, (p) => cb(p.agentId, p.detail, p.event, p.kind));
  }
}
