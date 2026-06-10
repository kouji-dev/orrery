import { isDevMode } from '@angular/core';
import { invoke } from '@tauri-apps/api/core';
import { listen } from '@tauri-apps/api/event';
import { relaunch } from '@tauri-apps/plugin-process';
import { Updater, UpdateHandle } from './updater';

/** Real boundary over the Tauri updater. Update resolution, download, and Ed25519
 *  signature verification all run in Rust (`update_check` / `update_perform`); the
 *  Rust side also picks the install strategy by how the app was installed — a
 *  per-user NSIS install uses the plugin's silent installer, while a per-machine
 *  MSI is upgraded via an elevated, one-UAC-prompt step that relaunches the app
 *  non-elevated. `isAvailable` checks the injected Tauri internals so the app still
 *  boots under `ng serve` (plain browser). */
export class TauriUpdater implements Updater {
  isAvailable(): boolean {
    // Never self-update a `tauri dev` session: the dev build (ng serve, hence
    // isDevMode) carries the in-repo version, which usually trails the published
    // release — checking would download the installer over the running dev build.
    if (isDevMode()) return false;
    return typeof (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ !== 'undefined';
  }

  async check(timeoutMs: number, channel?: string): Promise<UpdateHandle | null> {
    // The channel-aware backend returns `{version,date,notes}`; the older shape
    // was a bare version string. Tolerate both so this works across the T1
    // rollout (a Tauri command simply ignores args it doesn't declare).
    const payload: Record<string, unknown> = channel ? { timeoutMs, channel } : { timeoutMs };
    const res = await invoke<string | { version: string; date?: string | null; notes?: string | null } | null>(
      'update_check',
      payload,
    );
    if (!res) return null;
    const meta = typeof res === 'string' ? { version: res } : res;
    return {
      ...meta,
      downloadAndInstall: async (onProgress) => {
        // Rust emits cumulative download progress while fetching the installer.
        const unlisten = await listen<{ downloaded: number; total: number | null }>(
          'update://progress',
          (e) => onProgress(e.payload.downloaded, e.payload.total),
        );
        try {
          // On Windows this never resolves on success: `update_perform` hands off
          // to the installer (elevated for a per-machine MSI) and exits the
          // process. It rejects only on a pre-install failure (network / signature
          // / no update), which the caller treats as "no update". Same payload as
          // the check, so a channel-aware backend installs from the same channel.
          await invoke('update_perform', payload);
        } finally {
          unlisten();
        }
      },
    };
  }

  relaunch(): Promise<void> {
    // Unreached on Windows — the install step exits the process and the installer
    // relaunches the app. Kept for completeness / non-Windows targets.
    return relaunch();
  }
}
