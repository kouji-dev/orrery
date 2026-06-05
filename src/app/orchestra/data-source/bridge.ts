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
}

export const Commands = {
  ProjectList: 'project_list',
  ProjectCreate: 'project_create',
  ProjectRemove: 'project_remove',
  ProjectDetectGit: 'project_detect_git',
} as const;

export const Events = {
  ProjectCreated: 'project://created',
  ProjectUpdated: 'project://updated',
  ProjectDeleted: 'project://deleted',
} as const;

export const BRIDGE = new InjectionToken<Bridge>('BRIDGE');
