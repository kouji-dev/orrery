import { Injector, runInInjectionContext } from '@angular/core';
import { describe, expect, it, vi } from 'vitest';
import { Updater, UpdateHandle, UPDATER } from './updater';
import { UpdaterService } from './updater.service';

function make(updater: Partial<Updater>): UpdaterService {
  const full: Updater = {
    isAvailable: () => true,
    check: async () => null,
    relaunch: async () => {},
    ...updater,
  };
  const injector = Injector.create({ providers: [{ provide: UPDATER, useValue: full }] });
  return runInInjectionContext(injector, () => new UpdaterService());
}

describe('UpdaterService.run', () => {
  it('skips when not running under Tauri', async () => {
    const relaunch = vi.fn(async () => {});
    const svc = make({ isAvailable: () => false, relaunch });
    expect(await svc.run()).toBe('no-update');
    expect(relaunch).not.toHaveBeenCalled();
  });

  it('returns no-update when no update is available', async () => {
    expect(await make({ check: async () => null }).run()).toBe('no-update');
  });

  it('downloads, tracks progress, relaunches, returns updating', async () => {
    const relaunch = vi.fn(async () => {});
    const handle: UpdateHandle = {
      version: '1.2.0',
      downloadAndInstall: async (onProgress) => {
        onProgress(50, 100);
        onProgress(100, 100);
      },
    };
    const svc = make({ check: async () => handle, relaunch });
    const outcome = await svc.run();
    expect(outcome).toBe('updating');
    expect(svc.progress()).toBe(1);
    expect(svc.status()).toContain('1.2.0');
    expect(relaunch).toHaveBeenCalledOnce();
  });

  it('swallows check errors and returns no-update', async () => {
    const svc = make({ check: async () => { throw new Error('offline'); } });
    expect(await svc.run()).toBe('no-update');
  });
});
