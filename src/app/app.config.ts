import {
  ApplicationConfig,
  provideBrowserGlobalErrorListeners,
  provideZoneChangeDetection,
} from "@angular/core";
import { provideRouter } from "@angular/router";

import { routes } from "./app.routes";
import { BRIDGE } from "./orchestra/data-source/bridge";
import { MockBridge } from "./orchestra/data-source/mock-bridge";
import { TauriBridge } from "./orchestra/data-source/tauri-bridge";

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideZoneChangeDetection({ eventCoalescing: true }),
    provideRouter(routes),
    {
      provide: BRIDGE,
      useFactory: () =>
        (window as unknown as { __TAURI__?: unknown }).__TAURI__
          ? new TauriBridge()
          : new MockBridge(),
    },
  ],
};
