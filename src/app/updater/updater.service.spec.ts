import { Injector, runInInjectionContext } from '@angular/core';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { Settings } from '../models';
import { settingsDefaults, SettingsStore } from '../settings/settings.store';
import { Updater, UpdateHandle, UPDATER } from './updater';
import { UpdaterService } from './updater.service';

const ATTEMPT_KEY = 'orrery:update-attempt';

interface Made {
  svc: UpdaterService;
  noteUpdate: ReturnType<typeof vi.fn>;
}

function make(updater: Partial<Updater>, settings: Partial<Settings> = {}): Made {
  const full: Updater = {
    isAvailable: () => true,
    check: async () => null,
    relaunch: async () => {},
    ...updater,
  };
  const noteUpdate = vi.fn();
  // Install-path tests pin "auto" (also the real default since 2026-06-11)
  // unless a test passes its own policy.
  const store = {
    ready: async () => ({ ...settingsDefaults(), updatePolicy: "auto", ...settings }),
    noteUpdate,
  } as unknown as SettingsStore;
  const injector = Injector.create({
    providers: [
      { provide: UPDATER, useValue: full },
      { provide: SettingsStore, useValue: store },
    ],
  });
  const svc = runInInjectionContext(injector, () => new UpdaterService());
  return { svc, noteUpdate };
}

const attemptVersion = (): string | undefined => {
  const raw = localStorage.getItem(ATTEMPT_KEY);
  return raw ? JSON.parse(raw).version : undefined;
};

// The loop guard persists the attempted version + timestamp in localStorage.
afterEach(() => localStorage.clear());

describe('UpdaterService.run', () => {
  it('skips when not running under Tauri', async () => {
    const relaunch = vi.fn(async () => {});
    const { svc } = make({ isAvailable: () => false, relaunch });
    expect(await svc.run()).toBe('no-update');
    expect(relaunch).not.toHaveBeenCalled();
  });

  it('returns no-update when no update is available', async () => {
    expect(await make({ check: async () => null }).svc.run()).toBe('no-update');
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
    const { svc } = make({ check: async () => handle, relaunch });
    const outcome = await svc.run();
    expect(outcome).toBe('updating');
    expect(svc.progress()).toBe(1);
    expect(statusDuringDownload).toContain('1.2.0');
    expect(svc.status()).toBe('restarting');
    expect(relaunch).toHaveBeenCalledOnce();
  });

  it('shows the installing phase (full bar) when the installer takes over', async () => {
    let statusAfterPhase = '';
    let progressAfterPhase = 0;
    const handle: UpdateHandle = {
      version: '1.2.0',
      downloadAndInstall: async (onProgress, onPhase) => {
        onProgress(10, 100);
        onPhase?.('installing');
        statusAfterPhase = svc.status();
        progressAfterPhase = svc.progress();
      },
    };
    const { svc } = make({ check: async () => handle, relaunch: vi.fn(async () => {}) });
    expect(await svc.run()).toBe('updating');
    expect(statusAfterPhase).toBe('installing update · 1.2.0');
    expect(progressAfterPhase).toBe(1);
  });

  it('swallows check errors and returns no-update', async () => {
    const { svc } = make({ check: async () => { throw new Error('offline'); } });
    expect(await svc.run()).toBe('no-update');
  });

  it('records the attempted version with a timestamp before installing', async () => {
    const handle: UpdateHandle = { version: '2.0.0', downloadAndInstall: async () => {} };
    await make({ check: async () => handle }).svc.run();
    const raw = localStorage.getItem(ATTEMPT_KEY)!;
    expect(JSON.parse(raw).version).toBe('2.0.0');
    expect(JSON.parse(raw).ts).toBeGreaterThan(0);
  });

  it('does not reinstall a version attempted moments ago (tight-loop guard)', async () => {
    localStorage.setItem(ATTEMPT_KEY, JSON.stringify({ version: '1.2.0', ts: Date.now() }));
    const relaunch = vi.fn(async () => {});
    const downloadAndInstall = vi.fn(async () => {});
    const handle: UpdateHandle = { version: '1.2.0', downloadAndInstall };
    const { svc, noteUpdate } = make({ check: async () => handle, relaunch });
    expect(await svc.run()).toBe('no-update');
    expect(downloadAndInstall).not.toHaveBeenCalled();
    expect(relaunch).not.toHaveBeenCalled();
    expect(svc.status()).toContain('manually');
    // the blocked update is still surfaced in Settings → Updates (manual escape hatch)
    expect(noteUpdate).toHaveBeenCalledWith(expect.objectContaining({ version: '1.2.0' }));
  });

  it('retries the same version on a later restart (stale/legacy marker)', async () => {
    // Legacy plain-string marker (no timestamp) → treated as long ago → retried.
    // This is the dead-end the timestamp guard fixes: a stale marker from the old
    // build must not block the new build's first install attempt forever.
    localStorage.setItem(ATTEMPT_KEY, '1.2.0');
    const downloadAndInstall = vi.fn(async () => {});
    const handle: UpdateHandle = { version: '1.2.0', downloadAndInstall };
    expect(await make({ check: async () => handle }).svc.run()).toBe('updating');
    expect(downloadAndInstall).toHaveBeenCalledOnce();
  });

  it('still installs a different (newer) version than last attempt', async () => {
    localStorage.setItem(ATTEMPT_KEY, JSON.stringify({ version: '1.2.0', ts: Date.now() }));
    const downloadAndInstall = vi.fn(async () => {});
    const handle: UpdateHandle = { version: '1.3.0', downloadAndInstall };
    expect(await make({ check: async () => handle }).svc.run()).toBe('updating');
    expect(downloadAndInstall).toHaveBeenCalledOnce();
    expect(attemptVersion()).toBe('1.3.0');
  });

  it('clears the attempt marker once up to date', async () => {
    localStorage.setItem(ATTEMPT_KEY, JSON.stringify({ version: '1.2.0', ts: Date.now() }));
    await make({ check: async () => null }).svc.run();
    expect(localStorage.getItem(ATTEMPT_KEY)).toBeNull();
  });
});

describe('UpdaterService.run policy branching', () => {
  it('manual: skips the startup check entirely', async () => {
    const check = vi.fn(async () => null);
    const { svc, noteUpdate } = make({ check }, { updatePolicy: 'manual' });
    expect(await svc.run()).toBe('no-update');
    expect(check).not.toHaveBeenCalled();
    expect(noteUpdate).not.toHaveBeenCalled();
  });

  it('manual: the dev-skip still wins first (isAvailable short-circuits)', async () => {
    const check = vi.fn(async () => null);
    const { svc } = make({ isAvailable: () => false, check }, { updatePolicy: 'auto' });
    expect(await svc.run()).toBe('no-update');
    expect(check).not.toHaveBeenCalled();
  });

  it('notify: checks, surfaces the update for the settings card, but never installs', async () => {
    const downloadAndInstall = vi.fn(async () => {});
    const relaunch = vi.fn(async () => {});
    const handle: UpdateHandle = {
      version: '2.0.0',
      date: 'Jun 9, 2026',
      notes: 'https://example/releases',
      downloadAndInstall,
    };
    const { svc, noteUpdate } = make({ check: async () => handle, relaunch }, { updatePolicy: 'notify' });
    expect(await svc.run()).toBe('no-update');
    expect(downloadAndInstall).not.toHaveBeenCalled();
    expect(relaunch).not.toHaveBeenCalled();
    expect(noteUpdate).toHaveBeenCalledWith({ version: '2.0.0', date: 'Jun 9, 2026', notes: 'https://example/releases' });
    expect(svc.status()).toContain('2.0.0');
    // notify never attempts an install, so no loop-guard marker is written
    expect(localStorage.getItem(ATTEMPT_KEY)).toBeNull();
  });

  it('forwards the configured channel to the check', async () => {
    const check = vi.fn(async (_t: number, _c?: string) => null);
    await make({ check }, { channel: 'beta' }).svc.run();
    expect(check).toHaveBeenCalledWith(expect.any(Number), 'beta');
  });

  it('auto: installs (current behavior) and also surfaces the update', async () => {
    const handle: UpdateHandle = { version: '3.0.0', downloadAndInstall: async () => {} };
    const { svc, noteUpdate } = make({ check: async () => handle }, { updatePolicy: 'auto' });
    expect(await svc.run()).toBe('updating');
    expect(noteUpdate).toHaveBeenCalledWith(expect.objectContaining({ version: '3.0.0' }));
  });
});
