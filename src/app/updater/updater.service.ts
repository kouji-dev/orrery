import { inject, Injectable, signal } from '@angular/core';
import { UPDATER, UpdateOutcome } from './updater';

const CHECK_TIMEOUT_MS = 10_000;
// Survives the post-update relaunch (per-origin localStorage). Lets us notice an
// update that installs but never advances the version — so we don't reinstall
// the same version on every boot forever (which bricks the app).
const ATTEMPT_KEY = 'orrery:update-attempt';

@Injectable({ providedIn: 'root' })
export class UpdaterService {
  private readonly updater = inject(UPDATER);

  /** Human-readable phase shown on the loading screen. */
  readonly status = signal('');
  /** Download progress 0..1 (0 while indeterminate). */
  readonly progress = signal(0);

  /** Best-effort: any failure resolves `no-update` so boot is never blocked. */
  async run(): Promise<UpdateOutcome> {
    if (!this.updater.isAvailable()) return 'no-update';
    try {
      const update = await this.updater.check(CHECK_TIMEOUT_MS);
      if (!update) {
        this.setAttempt(null); // we're up to date — forget any prior attempt
        return 'no-update';
      }
      // Loop guard: we already installed this exact version last boot, yet it's
      // still on offer — the install/relaunch didn't take (e.g. a per-machine
      // install the silent updater can't touch). Don't reinstall in a loop; boot
      // normally and tell the user to update manually.
      if (this.getAttempt() === update.version) {
        this.status.set(`update ${update.version} available — install manually`);
        return 'no-update';
      }
      // Mark BEFORE installing: on Windows the installer can exit this process
      // before downloadAndInstall resolves, so the marker must already be set.
      this.setAttempt(update.version);
      this.status.set(`downloading update · ${update.version}`);
      await update.downloadAndInstall((downloaded, total) => {
        this.progress.set(total && total > 0 ? downloaded / total : 0);
      });
      this.status.set('restarting');
      await this.updater.relaunch();
      return 'updating';
    } catch {
      // Install didn't complete (e.g. network) — clear the marker so the next
      // boot retries instead of being suppressed by the loop guard.
      this.setAttempt(null);
      return 'no-update';
    }
  }

  private getAttempt(): string | null {
    try {
      return localStorage.getItem(ATTEMPT_KEY);
    } catch {
      return null;
    }
  }
  private setAttempt(v: string | null): void {
    try {
      if (v) localStorage.setItem(ATTEMPT_KEY, v);
      else localStorage.removeItem(ATTEMPT_KEY);
    } catch {
      /* no storage available */
    }
  }
}
