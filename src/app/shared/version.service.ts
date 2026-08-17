import { Injectable, isDevMode, signal } from "@angular/core";
import { getVersion } from "@tauri-apps/api/app";

/** App version + release channel, shared by the two version badges (top bar
 *  near the logo, and the footer).
 *
 *  - **version** comes from Tauri's `getVersion()` (reads tauri.conf), so it
 *    always matches the shipped build. Under plain `ng serve` there's no Tauri,
 *    so it stays empty and the badge shows just the channel tag.
 *  - **channel** is derived from `isDevMode()`: a dev build (`ng serve` /
 *    `tauri dev`) reads as **DEV**, a production build as **BETA**. Colors match
 *    the design — attention amber for dev, quiet ink for release. */
@Injectable({ providedIn: "root" })
export class VersionService {
  /** e.g. "0.1.1"; empty until resolved / when Tauri is unavailable. */
  readonly version = signal("");
  readonly isDev = isDevMode();
  readonly label = this.isDev ? "DEV" : "BETA";
  readonly color = this.isDev ? "var(--sem-attn)" : "var(--ink-2)";

  constructor() {
    getVersion()
      .then((v) => this.version.set(v))
      .catch(() => {
        /* no Tauri (browser `ng serve`) — leave version empty */
      });
  }
}
