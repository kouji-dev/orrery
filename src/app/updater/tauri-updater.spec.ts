import { isDevMode } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { TauriUpdater } from './tauri-updater';

// Mock the Tauri boundaries the updater now talks to (Rust commands + events).
vi.mock('@tauri-apps/api/core', () => ({ invoke: vi.fn() }));
vi.mock('@tauri-apps/api/event', () => ({ listen: vi.fn() }));
vi.mock('@tauri-apps/plugin-process', () => ({ relaunch: vi.fn() }));
// Override only isDevMode; keep the rest of @angular/core real.
vi.mock('@angular/core', async (orig) => ({ ...(await orig<object>()), isDevMode: vi.fn() }));

const win = window as unknown as { __TAURI_INTERNALS__?: unknown };

afterEach(() => {
  delete win.__TAURI_INTERNALS__;
  vi.mocked(isDevMode).mockReset();
  vi.mocked(invoke).mockReset();
  vi.mocked(listen).mockReset();
});

describe('TauriUpdater.isAvailable', () => {
  it('is false in dev mode even inside the Tauri webview', () => {
    vi.mocked(isDevMode).mockReturnValue(true);
    win.__TAURI_INTERNALS__ = {};
    expect(new TauriUpdater().isAvailable()).toBe(false);
  });

  it('is true in a production Tauri build', () => {
    vi.mocked(isDevMode).mockReturnValue(false);
    win.__TAURI_INTERNALS__ = {};
    expect(new TauriUpdater().isAvailable()).toBe(true);
  });

  it('is false outside the Tauri webview (plain browser)', () => {
    vi.mocked(isDevMode).mockReturnValue(false);
    expect(new TauriUpdater().isAvailable()).toBe(false);
  });
});

describe('TauriUpdater.check', () => {
  it('returns null when update_check reports no update', async () => {
    vi.mocked(invoke).mockResolvedValueOnce(null);
    expect(await new TauriUpdater().check(10_000)).toBeNull();
    expect(invoke).toHaveBeenCalledWith('update_check', { timeoutMs: 10_000 });
  });

  it('forwards the channel to update_check AND update_perform when given', async () => {
    vi.mocked(invoke).mockResolvedValueOnce('0.1.7'); // update_check
    vi.mocked(listen).mockResolvedValueOnce(vi.fn());
    vi.mocked(invoke).mockResolvedValueOnce(undefined); // update_perform

    const handle = await new TauriUpdater().check(10_000, 'beta');
    expect(invoke).toHaveBeenCalledWith('update_check', { timeoutMs: 10_000, channel: 'beta' });
    await handle!.downloadAndInstall(vi.fn());
    expect(invoke).toHaveBeenLastCalledWith('update_perform', { timeoutMs: 10_000, channel: 'beta' });
  });

  it('normalizes the channel-aware object response (version/date/notes)', async () => {
    vi.mocked(invoke).mockResolvedValueOnce({
      version: '0.2.0',
      date: 'Jun 9, 2026',
      notes: 'https://example/releases',
    });
    const handle = await new TauriUpdater().check(10_000, 'stable');
    expect(handle?.version).toBe('0.2.0');
    expect(handle?.date).toBe('Jun 9, 2026');
    expect(handle?.notes).toBe('https://example/releases');
  });

  it('returns a handle whose install runs update_perform and cleans up the listener', async () => {
    vi.mocked(invoke).mockResolvedValueOnce('0.1.7'); // update_check
    const unlisten = vi.fn();
    vi.mocked(listen).mockResolvedValueOnce(unlisten);
    vi.mocked(invoke).mockResolvedValueOnce(undefined); // update_perform

    const handle = await new TauriUpdater().check(10_000);
    expect(handle?.version).toBe('0.1.7');

    await handle!.downloadAndInstall(vi.fn());
    expect(listen).toHaveBeenCalledWith('update://progress', expect.any(Function));
    expect(invoke).toHaveBeenLastCalledWith('update_perform', { timeoutMs: 10_000 });
    expect(unlisten).toHaveBeenCalled(); // listener removed even though install resolved
  });

  it('forwards cumulative download progress to onProgress', async () => {
    vi.mocked(invoke).mockResolvedValueOnce('0.1.7'); // update_check
    let emit: (e: { payload: { downloaded: number; total: number | null } }) => void = () => {};
    vi.mocked(listen).mockImplementationOnce(async (_event, handler) => {
      emit = handler as typeof emit;
      return vi.fn();
    });
    // update_perform: simulate the Rust side emitting a progress event mid-install.
    vi.mocked(invoke).mockImplementationOnce(async () => {
      emit({ payload: { downloaded: 512, total: 1024 } });
    });

    const handle = await new TauriUpdater().check(10_000);
    const onProgress = vi.fn();
    await handle!.downloadAndInstall(onProgress);
    expect(onProgress).toHaveBeenCalledWith(512, 1024);
  });
});
