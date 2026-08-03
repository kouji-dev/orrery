import { inject, Injectable, signal } from "@angular/core";
import { BRIDGE, Commands, Events } from "../data-source/bridge";
import { TelemetryTraceState } from "../models";
import { SettingsStore } from "../settings/settings.store";

/**
 * Raw emit-trace indicator state (A0.7 Phase 1). The trace is opt-in and
 * BOUNDED — the backend auto-disables it after 30min or 200MB — so the UI must
 * always show whether it is recording: a status-bar chip and the dev panel's
 * Emits tab both read `active` from here.
 */
@Injectable({ providedIn: "root" })
export class TelemetryStore {
  private readonly bridge = inject(BRIDGE);
  private readonly settings = inject(SettingsStore);

  /** The raw trace is currently recording. */
  readonly traceActive = signal(false);
  /** Last state-change reason ("user" | "time cap (30min)" | "size cap (200MB)"). */
  readonly traceReason = signal<string | null>(null);

  constructor() {
    void this.init();
  }

  private async init() {
    // subscribe first so no state change is missed, then prime with a one-shot
    try {
      await this.bridge.on<{ active: boolean; reason: string }>(Events.TelemetryTrace, (s) => {
        this.traceActive.set(s.active);
        this.traceReason.set(s.reason);
        // An AUTO-disable (cap hit) must also un-check the settings toggle the
        // user sees — the backend already persisted it; this syncs the signal.
        if (!s.active && s.reason !== "user" && this.settings.settings().telemetryRawTrace) {
          this.settings.set({ telemetryRawTrace: false });
        }
      });
    } catch {
      // backend unavailable (plain `ng serve`) — indicator stays off
    }
    try {
      const s = await this.bridge.invoke<TelemetryTraceState>(Commands.TelemetryTraceState);
      this.traceActive.set(s.active);
    } catch {
      // optional command — fine to skip
    }
  }

  /** Toggle the raw trace: one settings write — the backend applies it
   *  immediately inside `settings_set` (flushed so the toggle can't be lost
   *  in the debounce window). */
  setTrace(on: boolean): void {
    this.settings.set({ telemetryRawTrace: on });
    this.settings.flush();
    // optimistic — the `telemetry://trace` event confirms
    this.traceActive.set(on);
  }
}
