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
  /** Open a native folder picker. Resolves the absolute path, or null if cancelled. */
  pickDirectory(): Promise<string | null>;
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
  AgentFocus: 'agent_focus',
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
  // ---- git-inspection commands ----
  AgentCommitDiff: 'agent_commit_diff',
  AgentCommitFileDiff: 'agent_commit_file_diff',
  AgentRangeFiles: 'agent_range_files',
  AgentRangeFileDiff: 'agent_range_file_diff',
  AgentBlame: 'agent_blame',
  AgentWorkingBlame: 'agent_working_blame',
  AgentFileHistory: 'agent_file_history',
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

export const Events = {
  ProjectCreated: 'project://created',
  ProjectUpdated: 'project://updated',
  ProjectDeleted: 'project://deleted',
  AgentCreated: 'agent://created',
  AgentUpdated: 'agent://updated',
  AgentDeleted: 'agent://deleted',
  AgentChanged: 'agent://changed',
  AgentOutput: 'agent://output',
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
  TicketCreated: 'ticket://created',
  TicketUpdated: 'ticket://updated',
  TicketDeleted: 'ticket://deleted',
  CommentCreated: 'comment://created',
  /** Cumulative update-download bytes: `{downloaded, total|null}`. */
  UpdateProgress: 'update://progress',
  /** Install handoff: payload `"installing"` once the download is done and the
   *  installer is about to take over (the process exits shortly after). */
  UpdatePhase: 'update://phase',
} as const;

export const BRIDGE = new InjectionToken<Bridge>('BRIDGE');
