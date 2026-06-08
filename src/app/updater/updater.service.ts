import { inject, Injectable, signal } from '@angular/core';
import { UPDATER, UpdateOutcome } from './updater';

const CHECK_TIMEOUT_MS = 10_000;

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
      if (!update) return 'no-update';
      this.status.set(`downloading update · ${update.version}`);
      await update.downloadAndInstall((downloaded, total) => {
        this.progress.set(total && total > 0 ? downloaded / total : 0);
      });
      this.status.set('restarting');
      await this.updater.relaunch();
      return 'updating';
    } catch {
      return 'no-update';
    }
  }
}
