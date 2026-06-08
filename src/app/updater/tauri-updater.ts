import { check, type DownloadEvent } from '@tauri-apps/plugin-updater';
import { relaunch } from '@tauri-apps/plugin-process';
import { Updater, UpdateHandle } from './updater';

/** Real boundary over the Tauri plugins. `isAvailable` checks the injected
 *  Tauri internals so the app still boots under `ng serve` (plain browser). */
export class TauriUpdater implements Updater {
  isAvailable(): boolean {
    return typeof (window as unknown as { __TAURI_INTERNALS__?: unknown }).__TAURI_INTERNALS__ !== 'undefined';
  }

  async check(timeoutMs: number): Promise<UpdateHandle | null> {
    const update = await check({ timeout: timeoutMs });
    if (!update) return null;
    return {
      version: update.version,
      downloadAndInstall: (onProgress) => {
        let downloaded = 0;
        let total: number | null = null;
        return update.downloadAndInstall((e: DownloadEvent) => {
          if (e.event === 'Started') total = e.data.contentLength ?? null;
          else if (e.event === 'Progress') {
            downloaded += e.data.chunkLength;
            onProgress(downloaded, total);
          }
        });
      },
    };
  }

  relaunch(): Promise<void> {
    return relaunch();
  }
}
