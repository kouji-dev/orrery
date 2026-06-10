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
  DetectTools: 'detect_tools',
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
} as const;

export const BRIDGE = new InjectionToken<Bridge>('BRIDGE');
