import { inject, Injectable, signal } from "@angular/core";
import { BRIDGE, Commands, Events } from "../data-source/bridge";
import { CostSnapshot } from "../models";

/**
 * Global Claude cost total from ccusage. The backend pushes a fresh
 * `system://cost` payload every 5 minutes; this store mirrors the latest into a signal
 * the status bar reads. Null until the first push / initial fetch. When the
 * payload's `available` is false (ccusage couldn't run) the status bar hides it.
 */
@Injectable({ providedIn: "root" })
export class CostStore {
  private bridge = inject(BRIDGE);
  readonly cost = signal<CostSnapshot | null>(null);

  constructor() {
    void this.init();
  }

  private async init() {
    try {
      await this.bridge.on<CostSnapshot>(Events.SystemCost, (c) => this.cost.set(c));
    } catch {
      // backend unavailable — readout stays hidden
    }
    try {
      // Cache peek only — null when the backend's first ccusage run is still in
      // flight; its `system://cost` push (subscribed above) fills in shortly.
      const initial = await this.bridge.invoke<CostSnapshot | null>(Commands.SystemCost);
      if (initial && this.cost() === null) this.cost.set(initial);
    } catch {
      // optional command — fine to skip
    }
  }
}
