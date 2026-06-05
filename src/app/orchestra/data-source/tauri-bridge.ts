import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { open } from '@tauri-apps/plugin-dialog';
import { AppErrorShape, Bridge, BridgeError } from './bridge';

export class TauriBridge implements Bridge {
  async invoke<R>(command: string, payload?: Record<string, unknown>): Promise<R> {
    try {
      return await invoke<R>(command, payload);
    } catch (e) {
      const err = e as AppErrorShape;
      if (err && typeof err === 'object' && 'kind' in err) {
        throw new BridgeError(err.kind, err.message);
      }
      throw new BridgeError('unknown', String(e));
    }
  }

  async on<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
    return listen<T>(event, e => handler(e.payload));
  }

  async pickDirectory(): Promise<string | null> {
    const selected = await open({ directory: true, multiple: false });
    // dialog returns string (path) | string[] | null; we requested a single dir.
    return typeof selected === 'string' ? selected : null;
  }
}
