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

  async check(timeoutMs: number): Promise<UpdateHandle | null> {
    const version = await invoke<string | null>('update_check', { timeoutMs });
    if (!version) return null;
    return {
      version,
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
          // / no update), which the caller treats as "no update".
          await invoke('update_perform', { timeoutMs });
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
