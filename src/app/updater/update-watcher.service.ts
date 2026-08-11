import { DestroyRef, inject, Injectable } from "@angular/core";
import { SettingsStore } from "../settings/settings.store";
import { UPDATER } from "./updater";

/** How often a running instance re-asks the update endpoint. */
export const POLL_MS = 5 * 60_000;
/** Focus fires on every alt-tab back; don't turn that into a request storm. */
export const FOCUS_MIN_GAP_MS = 30_000;
const CHECK_TIMEOUT_MS = 10_000;

/**
 * Keeps a RUNNING instance honest about new releases.
 *
 * `UpdaterService` checks exactly once, on the splash screen, so an instance that
 * booted before a release was published never learned about it — the toast could
 * only appear after a restart. This service re-checks every {@link POLL_MS} and
 * whenever the window regains focus (an alt-tab back is the moment a user is most
 * likely to act on it), so a release that lands mid-session surfaces on its own.
 *
 * Deliberately NOTIFY-ONLY, even under `updatePolicy: 'auto'`. Auto-install
 * belongs to the boot flow, where nothing is running yet; installing mid-session
 * relaunches the app and would kill every live agent PTY without warning. So this
 * only ever populates the toast — the user picks the moment. `'manual'` policy
 * opts out of background checking entirely, matching its startup behaviour.
 */
@Injectable({ providedIn: "root" })
export class UpdateWatcherService {
  private readonly updater = inject(UPDATER);
  private readonly settings = inject(SettingsStore);
  private readonly destroyRef = inject(DestroyRef);

  private timer: ReturnType<typeof setInterval> | null = null;
  private lastCheck = 0;
  private inFlight = false;

  /** Begin watching. Idempotent — a second call is a no-op. */
  start(): void {
    if (this.timer !== null) return;
    this.timer = setInterval(() => void this.check(), POLL_MS);
    globalThis.addEventListener?.("focus", this.onFocus);
    this.destroyRef.onDestroy(() => this.stop());
  }

  stop(): void {
    if (this.timer !== null) clearInterval(this.timer);
    this.timer = null;
    globalThis.removeEventListener?.("focus", this.onFocus);
  }

  private readonly onFocus = (): void => {
    if (Date.now() - this.lastCheck < FOCUS_MIN_GAP_MS) return;
    void this.check();
  };

  /**
   * One background poll. Silent by design: a failed check is a transient network
   * blip the user did not ask about, so it never flashes — unlike "Check now",
   * where they are watching for an answer.
   */
  async check(): Promise<void> {
    // Never race the user's own check, and never move the goalposts mid-install
    // (the toast is showing live download progress at that point).
    if (this.inFlight || this.settings.checking() || this.settings.installing()) return;
    const { updatePolicy, channel } = await this.settings.ready();
    if (updatePolicy === "manual") return;
    this.inFlight = true;
    this.lastCheck = Date.now();
    try {
      const update = await this.updater.check(CHECK_TIMEOUT_MS, channel);
      this.settings.noteBackgroundUpdate(
        update ? { version: update.version, date: update.date, notes: update.notes } : null,
      );
    } catch {
      // offline / endpoint hiccup — keep whatever we already knew and retry next tick
    } finally {
      this.inFlight = false;
    }
  }
}
