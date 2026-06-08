import {
  ApplicationConfig,
  provideBrowserGlobalErrorListeners,
  provideZoneChangeDetection,
} from "@angular/core";
import { provideRouter } from "@angular/router";

import { routes } from "./app.routes";
import { BRIDGE } from "./data-source/bridge";
import { TauriBridge } from "./data-source/tauri-bridge";
import { UPDATER } from "./updater/updater";
import { TauriUpdater } from "./updater/tauri-updater";

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideZoneChangeDetection({ eventCoalescing: true }),
    provideRouter(routes),
    // Real backend only — data comes from the Tauri/SQLite layer.
    { provide: BRIDGE, useFactory: () => new TauriBridge() },
    { provide: UPDATER, useFactory: () => new TauriUpdater() },
  ],
};
