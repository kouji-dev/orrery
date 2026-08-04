import { InjectionToken } from '@angular/core';

export interface AppErrorShape { kind: string; message: string; }

export class BridgeError extends Error {
  constructor(public kind: string, message: string) {
    super(message);
    this.name = 'BridgeError';
  }
}

export interface Bridge {
  invoke<R>(command: string, payload?: Record<string, unknown>): Promise<R>;
  on<T>(event: string, handler: (payload: T) => void): Promise<() => void>;
  /** Open a native folder picker, optionally starting at `defaultPath`.
   *  Resolves the absolute path, or null if cancelled. */
  pickDirectory(defaultPath?: string): Promise<string | null>;
  /** Open a native file picker, optionally starting at `defaultPath` (a file or
   *  dir). Resolves the absolute path, or null if cancelled. */
  pickFile(defaultPath?: string): Promise<string | null>;
}

export const Commands = {
  ProjectList: 'project_list',
  ProjectCreate: 'project_create',
  ProjectUpdate: 'project_update',
  ProjectInitGit: 'project_init_git',
  ProjectRemove: 'project_remove',
  ProjectDetectGit: 'project_detect_git',
  ProjectCommits: 'project_commits',
  AgentList: 'agent_list',
  AgentSpawn: 'agent_spawn',
  AgentUpdate: 'agent_update',
  AgentRemove: 'agent_remove',
  AgentTree: 'agent_tree',
  AgentDir: 'agent_dir',
  AgentChanges: 'agent_changes',
  AgentCommits: 'agent_commits',
  AgentCommit: 'agent_commit',
  AgentDiscard: 'agent_discard',
  AgentPush: 'agent_push',
  AgentAction: 'agent_action',
  AgentDiff: 'agent_diff',
  AgentWatch: 'agent_watch',
  AgentStart: 'agent_start',
  AgentStop: 'agent_stop',
  AgentInput: 'agent_input',
  AgentAllow: 'agent_allow',
  AgentDeny: 'agent_deny',
  AgentDecide: 'agent_decide',
  AgentResize: 'agent_resize',
  /** A0.2 interest subscription — supersedes agent_focus. The frontend
   *  publishes the full per-agent {id, mode} set derived from what is VISIBLE
   *  (stream = terminal pane, digest = overview mini-preview, absent = none);
   *  the backend diffs it. `none` means do-not-EMIT (the PTY keeps being read
   *  into the bounded backend scrollback ring). */
  RuntimeSubscribe: 'runtime_subscribe',
  /** A1.2 scrollback snapshot `{text, endSeq}` — the terminal recovery source
   *  (reload / stale-terminal-shown / none→stream resubscribe). */
  RuntimeSnapshot: 'runtime_snapshot',
  /** ONE-SHOT drain of the agents that were running when the app last shut
   *  down (captured before reset_running) — the auto-resume flow's source. */
  AgentsInterrupted: 'agents_interrupted',
  TicketList: 'ticket_list',
  TicketCreate: 'ticket_create',
  TicketUpdate: 'ticket_update',
  TicketRemove: 'ticket_remove',
  TicketSetStatus: 'ticket_set_status',
  CommentList: 'comment_list',
  CommentAdd: 'comment_add',
  DetectTools: 'detect_tools',
  VerifyToolPath: 'verify_tool_path',
  SystemMetrics: 'system_metrics',
  /** A7.7 recursive process tree (Orrery + each agent PTY child as roots).
   *  PULL-based: poll only while the perf panel is open (A1.7 gating). */
  ProcessTree: 'process_tree',
  /** A0.7 emit-telemetry aggregate table (per event name: count/bytes/p50/p95/rate). */
  TelemetryEmits: 'telemetry_emits',
  /** Raw emit-trace indicator state `{active, startedMs, bytes}`. */
  TelemetryTraceState: 'telemetry_trace_state',
  SystemCost: 'system_cost',
  SetWindowIcon: 'set_window_icon',
  SettingsGet: 'settings_get',
  SettingsSet: 'settings_set',
  /** Channel-aware update check → `{version,date,notes} | null` (legacy shape: a
   *  bare version string). */
  UpdateCheck: 'update_check',
  /** Channel-aware download + install + relaunch (may never resolve on Windows —
   *  the installer exits the process). */
  UpdateInstall: 'update_install',
  /** Open the app's rolling diagnostics log file (app_log_dir/orrery.log) in the
   *  OS default handler. */
  OpenLog: 'open_log',
  /** Open an arbitrary filesystem path (e.g. an agent's worktree dir) in the OS
   *  file manager. */
  OpenPath: 'open_path',
  // ---- git-inspection commands ----
  AgentCommitDiff: 'agent_commit_diff',
  AgentCommitFileDiff: 'agent_commit_file_diff',
  AgentRangeFiles: 'agent_range_files',
  AgentRangeFileDiff: 'agent_range_file_diff',
  AgentBlame: 'agent_blame',
  AgentWorkingBlame: 'agent_working_blame',
  AgentFileHistory: 'agent_file_history',
  // ---- native merge + conflict session (A3.5 / A3.6) ----
  /** Native merge of a branch into the agent's branch → MergeSession (empty
   *  conflicts = merged clean; non-empty = session in progress). */
  AgentMerge: 'agent_merge',
  /** Still-conflicted files of the in-progress session (stages 1/2/3 + markers). */
  AgentConflicts: 'agent_conflicts',
  /** Write a file's resolution and stage it. */
  AgentConflictResolve: 'agent_conflict_resolve',
  /** Abort the merge: drop session state, hard-reset to HEAD. */
  AgentMergeAbort: 'agent_merge_abort',
  /** Commit the fully-resolved merge → short sha. */
  AgentMergeContinue: 'agent_merge_continue',
  /** Merge/rebase/cherry-pick in progress? + remaining conflict count. */
  AgentSessionState: 'agent_session_state',
  // ---- find in files / search everywhere (B3.1 / B2.1) ----
  /** Start a streaming search → search id; results arrive on `search://results`
   *  batches and the run ends with one `search://done`. */
  SearchStart: 'search_start',
  /** Cancel a running search by id (no-op when already done). */
  SearchCancel: 'search_cancel',
  /** All gitignore-filtered file paths of an agent's worktree (capped 20k) —
   *  the Files corpus for Search Everywhere / Go to File. */
  SearchFiles: 'search_files',
} as const;

/** One agent's coalesced PTY output inside a multiplexed `agent://output`
 *  frame. The event payload is an ARRAY of these — the backend mux emits one
 *  event per ~16ms frame TOTAL, carrying an entry per agent that produced
 *  output, with that agent's chunks coalesced. `seq` is the agent's cumulative
 *  emitted-byte count (monotonic per agent — the snapshot-dedup foundation). */
export interface AgentOutputEntry {
  id: string;
  chunk: string;
  seq: number;
}

/** A0.2 interest mode: how much of an agent's PTY output the renderer wants.
 *  `stream` = full output per frame (a visible terminal pane); `digest` =
 *  last few rendered lines at 1Hz (overview mini-preview); `none`/absent =
 *  nothing ships (the backend ring keeps the bytes, bounded). */
export type InterestMode = 'stream' | 'digest' | 'none';

/** One entry of the `runtime_subscribe` interest set. */
export interface InterestEntry {
  id: string;
  mode: InterestMode;
}

/** `runtime_snapshot` reply: the backend scrollback ring's content plus the
 *  cumulative byte seq of its last byte. Recovery contract: term.clear() →
 *  write `text` → drop live chunks with `seq <= endSeq` → resume. */
export interface RuntimeSnapshot {
  text: string;
  endSeq: number;
}

/** One agent's entry in an `agent://digest` payload: the last ≤5 rendered
 *  lines (backend-side VT fold) + the seq of the last folded chunk. */
export interface AgentDigestEntry {
  id: string;
  lines: string[];
  seq: number;
}

/** `agent://pty-status` payload (A0.3): Rust-side PTY heuristics for tools
 *  without a blocking permission hook (gemini). Emitted on state TRANSITIONS
 *  only; `detail` is the folded prompt tail (notification context). */
export interface AgentPtyStatusPayload {
  id: string;
  working: boolean;
  needsInput: boolean;
  permission: boolean;
  detail: string;
}

export const Events = {
  ProjectCreated: 'project://created',
  ProjectUpdated: 'project://updated',
  ProjectDeleted: 'project://deleted',
  AgentCreated: 'agent://created',
  AgentUpdated: 'agent://updated',
  AgentDeleted: 'agent://deleted',
  AgentChanged: 'agent://changed',
  AgentOutput: 'agent://output',
  /** 1Hz per-agent digests (last ≤5 rendered lines) for digest-mode agents —
   *  the overview mini-terminals' feed. Payload: AgentDigestEntry[]. */
  AgentDigest: 'agent://digest',
  /** Rust-side PTY status heuristics for un-hooked tools (gemini) — A0.3.
   *  Payload: AgentPtyStatusPayload, transitions only. */
  AgentPtyStatus: 'agent://pty-status',
  AgentExit: 'agent://exit',
  AgentAsk: 'agent://ask',
  /** A hook signalled the agent needs the user (permission prompt / question). */
  AgentPermission: 'agent://permission',
  /** Non-blocking status ping from a hook (working / idle). */
  AgentStatus: 'agent://status',
  /** Action-carrying activity from a pre-tool hook (e.g. "Bash: npm test"). */
  AgentActivity: 'agent://activity',
  /** Fresh cpu/memory snapshot for the app + every running agent (pushed every 5s while agents run, 20s idle). */
  SystemMetrics: 'system://metrics',
  /** Global Claude cost total from ccusage (pushed every 5 minutes). */
  SystemCost: 'system://cost',
  /** Per-command backend exec aggregates (pushed every 2s; dev + prod). */
  PerfStats: 'perf://stats',
  /** Raw emit-trace state change `{active, reason}` — drives the visible
   *  "recording" indicator (trace auto-disables after 30min / 200MB). */
  TelemetryTrace: 'telemetry://trace',
  TicketCreated: 'ticket://created',
  TicketUpdated: 'ticket://updated',
  TicketDeleted: 'ticket://deleted',
  CommentCreated: 'comment://created',
  /** Cumulative update-download bytes: `{downloaded, total|null}`. */
  UpdateProgress: 'update://progress',
  /** Install handoff: payload `"installing"` once the download is done and the
   *  installer is about to take over (the process exits shortly after). */
  UpdatePhase: 'update://phase',
  /** A batch of streamed find-in-files matches: `{searchId, items, files}`. */
  SearchResults: 'search://results',
  /** A search finished: `{searchId, files, matches, truncated, cancelled}`. */
  SearchDone: 'search://done',
} as const;

// ---- find-in-files payloads (serde camelCase from src-tauri/src/search) ----

/** One find-in-files match line. `ranges` are UTF-16 [start, end) offsets into
 *  `text` (JS string indexing), so highlighting needs no client re-match. */
export interface SearchMatchEntry {
  /** Root-relative path, forward-slash separated. */
  path: string;
  /** Agent owning the worktree the match came from (absent = project checkout). */
  agentId?: string;
  /** Human label of the root (agent name); absent = project checkout. */
  root?: string;
  line: number;
  text: string;
  ranges: [number, number][];
}

export interface SearchResultsPayload {
  searchId: string;
  items: SearchMatchEntry[];
  /** Cumulative files scanned so far. */
  files: number;
}

export interface SearchDonePayload {
  searchId: string;
  files: number;
  matches: number;
  truncated: boolean;
  cancelled: boolean;
}

export const BRIDGE = new InjectionToken<Bridge>('BRIDGE');
