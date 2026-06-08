import { inject, Injectable, signal } from "@angular/core";
import { BRIDGE, Commands, Events } from "../data-source/bridge";
import { SystemMetrics } from "../models";

/**
 * Live cpu/memory snapshot for the app + every running agent. The backend pushes
 * a fresh `system://metrics` payload every 3s; this store just mirrors the latest
 * one into a signal the status-bar gauge reads. Stays null until the first push
 * (or the optional initial synchronous fetch) arrives.
 */
@Injectable({ providedIn: "root" })
export class MetricsStore {
  private bridge = inject(BRIDGE);
  readonly metrics = signal<SystemMetrics | null>(null);

  constructor() {
    void this.init();
  }

  private async init() {
    // subscribe first so no push is missed, then prime with a one-shot value
    try {
      await this.bridge.on<SystemMetrics>(Events.SystemMetrics, (m) => this.metrics.set(m));
    } catch {
      // backend unavailable — gauge stays empty
    }
    try {
      const initial = await this.bridge.invoke<SystemMetrics>(Commands.SystemMetrics);
      // don't clobber a push that landed during the await
      if (this.metrics() === null) this.metrics.set(initial);
    } catch {
      // optional command — fine to skip
    }
  }
}
