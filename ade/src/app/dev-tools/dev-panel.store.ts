import { computed, inject, Injectable, signal } from "@angular/core";
import { PerfStore } from "../perf/perf.store";

export type DevPanelTab = "perf" | "agents" | "projects" | "resources" | "emits";

/**
 * Visibility + active tab of the floating dev console, lifted out of the
 * component so other surfaces can deep-link into it (the status-bar cpu/mem
 * readout opens the Resources tab).
 */
@Injectable({ providedIn: "root" })
export class DevPanelStore {
  private readonly perf = inject(PerfStore);

  readonly open = signal(false);
  readonly tab = signal<DevPanelTab>("perf");

  /** Live perf alerts (error'd or slow commands) — the status-bar Dev chip's
   *  count pill and the console's own header both read this. */
  readonly alertCount = computed(
    () => this.perf.rows().filter((r) => !r.stale && (r.errPct > 0 || (r.avgRt != null && r.avgRt > 100))).length,
  );

  toggle() {
    this.open.update((v) => !v);
  }

  openResources() {
    this.tab.set("resources");
    this.open.set(true);
  }

  /** Deep-link for the status-bar raw-trace chip (A0.7 visible indicator). */
  openEmits() {
    this.tab.set("emits");
    this.open.set(true);
  }
}
