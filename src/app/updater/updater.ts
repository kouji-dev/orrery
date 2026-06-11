import { InjectionToken } from '@angular/core';

/** Outcome of a launch-time update check. `updating` means the app is about to
 *  relaunch into a new version — the caller must NOT navigate onward. */
export type UpdateOutcome = 'no-update' | 'updating';

/** A pending update. `downloadAndInstall` reports bytes via `onProgress`
 *  (`total` is null when the server sends no content-length) and fires
 *  `onPhase("installing")` once the download is done and the installer takes
 *  over (on Windows the process exits shortly after). `date`/`notes` are
 *  optional release metadata for the Settings → Updates card. */
export interface UpdateHandle {
  version: string;
  date?: string | null;
  notes?: string | null;
  downloadAndInstall(
    onProgress: (downloaded: number, total: number | null) => void,
    onPhase?: (phase: 'installing') => void,
  ): Promise<void>;
}

/** Thin, mockable boundary over the Tauri updater/process plugins. */
export interface Updater {
  /** True only when running inside the Tauri webview. */
  isAvailable(): boolean;
  /** Resolve an available update on `channel` (omitted = the backend default,
   *  stable), or null. Rejects/throws on transport errors. */
  check(timeoutMs: number, channel?: string): Promise<UpdateHandle | null>;
  /** Restart the app (does not return in a real Tauri process). */
  relaunch(): Promise<void>;
}

export const UPDATER = new InjectionToken<Updater>('UPDATER');
