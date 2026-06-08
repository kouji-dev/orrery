import {
  ApplicationConfig,
  provideBrowserGlobalErrorListeners,
  provideZoneChangeDetection,
} from "@angular/core";
import { provideRouter } from "@angular/router";

import { routes } from "./app.routes";
import { BRIDGE } from "./data-source/bridge";
import { TauriBridge } from "./data-source/tauri-bridge";

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideZoneChangeDetection({ eventCoalescing: true }),
    provideRouter(routes),
    // Real backend only — data comes from the Tauri/SQLite layer.
    { provide: BRIDGE, useFactory: () => new TauriBridge() },
  ],
};
