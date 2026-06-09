import { inject, Injectable, signal } from '@angular/core';
import { UPDATER, UpdateOutcome } from './updater';

const CHECK_TIMEOUT_MS = 10_000;
// Survives the post-update relaunch (per-origin localStorage). Records the version
// we last TRIED to install and when. Lets us notice an update that installs but
// never advances the version, so we don't reinstall it in a tight relaunch loop
// (which would brick the app) — while still RETRYING on a genuine later restart.
// That retry matters for the per-machine MSI path: "didn't advance" there usually
// just means the user dismissed the one-time UAC prompt, and a permanent suppress
// would dead-end them (no in-app way to re-trigger).
const ATTEMPT_KEY = 'orrery:update-attempt';
// Suppress a re-attempt of the SAME version only within this window of the last
// attempt — i.e. an immediate failed auto-relaunch. A restart later than this (or a
// legacy marker that carries no timestamp) is allowed to retry.
const RETRY_COOLDOWN_MS = 60_000;

interface Attempt {
  version: string;
  ts: number;
}

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
      // Loop guard: we tried this exact version moments ago yet it's still on offer
      // — the install/relaunch didn't take. Don't reinstall in a tight loop; boot
      // normally. A restart past the cooldown (or a stale/legacy marker) retries.
      const prev = this.getAttempt();
      if (prev && prev.version === update.version && Date.now() - prev.ts < RETRY_COOLDOWN_MS) {
        this.status.set(`update ${update.version} available — install manually`);
        return 'no-update';
      }
      // Mark BEFORE installing: on Windows the installer can exit this process
      // before downloadAndInstall resolves, so the marker must already be set.
      this.setAttempt({ version: update.version, ts: Date.now() });
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

  /** Read the last attempt. Tolerates the legacy plain-string format (no timestamp)
   *  by treating it as long ago, so a stale marker never blocks a retry forever. */
  private getAttempt(): Attempt | null {
    try {
      const raw = localStorage.getItem(ATTEMPT_KEY);
      if (!raw) return null;
      try {
        const parsed = JSON.parse(raw);
        if (parsed && typeof parsed.version === 'string') {
          return { version: parsed.version, ts: Number(parsed.ts) || 0 };
        }
      } catch {
        /* legacy plain-string marker — fall through */
      }
      return { version: raw, ts: 0 };
    } catch {
      return null;
    }
  }

  private setAttempt(a: Attempt | null): void {
    try {
      if (a) localStorage.setItem(ATTEMPT_KEY, JSON.stringify(a));
      else localStorage.removeItem(ATTEMPT_KEY);
    } catch {
      /* no storage available */
    }
  }
}
