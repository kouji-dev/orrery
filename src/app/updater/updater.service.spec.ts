import { Injector, runInInjectionContext } from '@angular/core';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { Updater, UpdateHandle, UPDATER } from './updater';
import { UpdaterService } from './updater.service';

const ATTEMPT_KEY = 'orrery:update-attempt';

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

// The loop guard persists the attempted version in localStorage across runs.
afterEach(() => localStorage.clear());

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
    // status is transient (downloading → restarting), so capture it mid-download.
    let statusDuringDownload = '';
    const handle: UpdateHandle = {
      version: '1.2.0',
      downloadAndInstall: async (onProgress) => {
        statusDuringDownload = svc.status();
        onProgress(50, 100);
        onProgress(100, 100);
      },
    };
    const svc = make({ check: async () => handle, relaunch });
    const outcome = await svc.run();
    expect(outcome).toBe('updating');
    expect(svc.progress()).toBe(1);
    expect(statusDuringDownload).toContain('1.2.0');
    expect(svc.status()).toBe('restarting');
    expect(relaunch).toHaveBeenCalledOnce();
  });

  it('swallows check errors and returns no-update', async () => {
    const svc = make({ check: async () => { throw new Error('offline'); } });
    expect(await svc.run()).toBe('no-update');
  });

  it('records the attempted version before installing', async () => {
    const handle: UpdateHandle = { version: '2.0.0', downloadAndInstall: async () => {} };
    await make({ check: async () => handle }).run();
    expect(localStorage.getItem(ATTEMPT_KEY)).toBe('2.0.0');
  });

  it('does not reinstall a version already attempted last boot (loop guard)', async () => {
    localStorage.setItem(ATTEMPT_KEY, '1.2.0');
    const relaunch = vi.fn(async () => {});
    const downloadAndInstall = vi.fn(async () => {});
    const handle: UpdateHandle = { version: '1.2.0', downloadAndInstall };
    const svc = make({ check: async () => handle, relaunch });
    expect(await svc.run()).toBe('no-update');
    expect(downloadAndInstall).not.toHaveBeenCalled();
    expect(relaunch).not.toHaveBeenCalled();
    expect(svc.status()).toContain('manually');
  });

  it('still installs a different (newer) version than last attempt', async () => {
    localStorage.setItem(ATTEMPT_KEY, '1.2.0');
    const downloadAndInstall = vi.fn(async () => {});
    const handle: UpdateHandle = { version: '1.3.0', downloadAndInstall };
    expect(await make({ check: async () => handle }).run()).toBe('updating');
    expect(downloadAndInstall).toHaveBeenCalledOnce();
    expect(localStorage.getItem(ATTEMPT_KEY)).toBe('1.3.0');
  });

  it('clears the attempt marker once up to date', async () => {
    localStorage.setItem(ATTEMPT_KEY, '1.2.0');
    await make({ check: async () => null }).run();
    expect(localStorage.getItem(ATTEMPT_KEY)).toBeNull();
  });
});
