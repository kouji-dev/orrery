import { Injectable, signal } from "@angular/core";

export type DevPanelTab = "perf" | "agents" | "projects" | "resources";

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
}
