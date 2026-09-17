import { inject, Injectable, signal } from '@angular/core';
import { SettingsStore } from '../settings/settings.store';
import { UPDATER, UpdateOutcome } from './updater';

const CHECK_TIMEOUT_MS = 10_000;
// Survives the post-update relaunch (per-origin localStorage). Records the version
// we last TRIED to install and when. Lets us notice an update that installs but
// never advances the version, so we don't reinstall it in a tight relaunch loop
// (which would brick the app) — while still RETRYING on a genuine later restart.
// "Didn't advance" can still happen (e.g. the user cancelled the passive MSI's
// progress dialog), and a permanent suppress would dead-end them.
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
  private readonly settings = inject(SettingsStore);

  /** Human-readable phase shown on the loading screen. */
  readonly status = signal('');
  /** Download progress 0..1 (0 while indeterminate). */
  readonly progress = signal(0);

  /** Best-effort: any failure resolves `no-update` so boot is never blocked.
   *
   *  The settings `updatePolicy` branches the startup flow:
   *  - `manual`: no startup check at all;
   *  - `notify` (default): check on the configured channel, surface the result via
   *    the bottom-center toast (+ settings card / nav dot), but never install —
   *    the user chooses "Install" or "Later";
   *  - `auto`: check, install, relaunch, with the tight-loop guard below.
   *
   *  The check runs regardless of `updater.isAvailable()` so a dev/local build
   *  still surfaces the toast at startup (a non-Tauri `ng serve` simply throws
   *  inside `check()` → caught below → `no-update`). Only the auto-INSTALL is
   *  gated on `isAvailable()`, so a dev build never self-installs a release. */
  async run(): Promise<UpdateOutcome> {
    const { updatePolicy, channel } = await this.settings.ready();
    if (updatePolicy === 'manual') return 'no-update';
    try {
      const update = await this.updater.check(CHECK_TIMEOUT_MS, channel);
      if (!update) {
        this.setAttempt(null); // we're up to date — forget any prior attempt
        return 'no-update';
      }
      // Whatever happens next (notify / loop-guard / install), make the update
      // discoverable via the toast + Settings → Updates.
      this.settings.noteUpdate({ version: update.version, date: update.date, notes: update.notes });
      // Surface-only path: the notify default, or any build we can't install onto
      // (dev/local — `isAvailable()` false). Show the toast; never install.
      if (updatePolicy === 'notify' || !this.updater.isAvailable()) {
        this.status.set(`update ${update.version} available`);
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
      await update.downloadAndInstall(
        (downloaded, total) => {
          this.progress.set(total && total > 0 ? downloaded / total : 0);
        },
        () => {
          // Download done — the installer takes over (and exits this process on
          // Windows; the MSI shows its own native progress window from here).
          this.progress.set(1);
          this.status.set(`installing update · ${update.version}`);
        },
      );
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
