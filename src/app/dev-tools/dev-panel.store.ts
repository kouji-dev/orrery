import { Injectable, signal } from "@angular/core";

export type DevPanelTab = "perf" | "agents" | "projects" | "resources" | "processes" | "emits";

/**
 * Visibility + active tab of the floating dev console, lifted out of the
 * component so other surfaces can deep-link into it (the status-bar cpu/mem
 * readout opens the Resources tab).
 */
@Injectable({ providedIn: "root" })
export class DevPanelStore {
  readonly open = signal(false);
  readonly tab = signal<DevPanelTab>("perf");

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
