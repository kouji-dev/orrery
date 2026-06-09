import { isDevMode } from '@angular/core';
import { afterEach, describe, expect, it, vi } from 'vitest';
import { TauriUpdater } from './tauri-updater';

// Don't pull in the real Tauri plugin internals when importing the boundary.
vi.mock('@tauri-apps/plugin-updater', () => ({ check: vi.fn() }));
vi.mock('@tauri-apps/plugin-process', () => ({ relaunch: vi.fn() }));
// Override only isDevMode; keep the rest of @angular/core real.
vi.mock('@angular/core', async (orig) => ({ ...(await orig<object>()), isDevMode: vi.fn() }));

const win = window as unknown as { __TAURI_INTERNALS__?: unknown };

afterEach(() => {
  delete win.__TAURI_INTERNALS__;
  vi.mocked(isDevMode).mockReset();
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
