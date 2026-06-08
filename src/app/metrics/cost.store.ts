import { inject, Injectable, signal } from "@angular/core";
import { BRIDGE, Commands, Events } from "../data-source/bridge";
import { CostSnapshot } from "../models";

/**
 * Global Claude cost total from ccusage. The backend pushes a fresh
 * `system://cost` payload every 60s; this store mirrors the latest into a signal
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
      const initial = await this.bridge.invoke<CostSnapshot>(Commands.SystemCost);
      if (this.cost() === null) this.cost.set(initial);
    } catch {
      // optional command — fine to skip
    }
  }
}
