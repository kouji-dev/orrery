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
  AgentCommit: 'agent_commit',
  AgentDiscard: 'agent_discard',
  AgentMerge: 'agent_merge',
  AgentDiff: 'agent_diff',
  AgentWatch: 'agent_watch',
  AgentStart: 'agent_start',
  AgentStop: 'agent_stop',
  AgentInput: 'agent_input',
  AgentResize: 'agent_resize',
  DetectTools: 'detect_tools',
} as const;

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
} as const;

export const BRIDGE = new InjectionToken<Bridge>('BRIDGE');
