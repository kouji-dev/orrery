import {
  ApplicationConfig,
  inject,
  provideBrowserGlobalErrorListeners,
  provideZoneChangeDetection,
} from "@angular/core";
import { provideRouter } from "@angular/router";

import { routes } from "./app.routes";
import { BRIDGE } from "./data-source/bridge";
import { TauriBridge } from "./data-source/tauri-bridge";
import { InstrumentedBridge } from "./data-source/instrumented-bridge";
import { PerfStore } from "./perf/perf.store";
import { UPDATER } from "./updater/updater";
import { TauriUpdater } from "./updater/tauri-updater";

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideZoneChangeDetection({ eventCoalescing: true }),
    provideRouter(routes),
    // Real backend only — data comes from the Tauri/SQLite layer. The bridge is
    // wrapped to time every command round-trip into the PerfStore (DevTools).
    { provide: BRIDGE, useFactory: () => new InstrumentedBridge(new TauriBridge(), inject(PerfStore)) },
    { provide: UPDATER, useFactory: () => new TauriUpdater() },
  ],
};
