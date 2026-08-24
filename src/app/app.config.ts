import {
  ApplicationConfig,
  inject,
  provideBrowserGlobalErrorListeners,
  provideZonelessChangeDetection,
} from "@angular/core";
import { provideRouter } from "@angular/router";
import { KJ_BUTTON_DEFAULTS, provideKjButton } from "@kouji-ui/core";

import { routes } from "./app.routes";
import { BRIDGE } from "./data-source/bridge";
import { TauriBridge } from "./data-source/tauri-bridge";
import { InstrumentedBridge } from "./data-source/instrumented-bridge";
import { PerfStore } from "./perf/perf.store";
import { startLongTaskProbe } from "./perf/longtask-probe";
import { UPDATER } from "./updater/updater";
import { TauriUpdater } from "./updater/tauri-updater";

export const appConfig: ApplicationConfig = {
  providers: [
    provideBrowserGlobalErrorListeners(),
    provideZonelessChangeDetection(),
    // Orrery's button vocabulary on top of kouji's. `danger` is red ink on a
    // transparent ground (a destructive ACTION, not a destructive surface —
    // kouji's `destructive` is a filled red button, which is far too loud for
    // IDE chrome). `toolbar` is the compact muted control used by panel
    // headers; it replaces the per-panel .br-op / .lh-op / .follow classes,
    // which were three copies of the same recipe.
    // `quiet` is a button that is only its label — no ground, border, padding
    // or shadow. Five components had hand-rolled it as an inline --kj-button-*
    // pack (the toast's "Later", the settings "Reset all", the modal links).
    provideKjButton({
      variants: [...KJ_BUTTON_DEFAULTS.variants, "danger", "toolbar", "quiet"],
      sizes: KJ_BUTTON_DEFAULTS.sizes,
      // `sm` is the app's default control — the top bar's notification button,
      // the Spawn CTA, modal footers. `xs` is the deliberate step down for
      // in-pane toolbars. NOTE this default was INERT until kouji 0.6.0: the
      // element wrappers hardcoded `md` and ignored the provider entirely.
      defaults: { variant: "ghost", size: "sm" },
    }),
    provideRouter(routes),
    // Real backend only — data comes from the Tauri/SQLite layer. The bridge is
    // wrapped to time every command round-trip into the PerfStore (DevTools).
    // The longtask probe rides along: main-thread stalls → js_longtask row.
    {
      provide: BRIDGE,
      useFactory: () => {
        const perf = inject(PerfStore);
        startLongTaskProbe(perf);
        return new InstrumentedBridge(new TauriBridge(), perf);
      },
    },
    { provide: UPDATER, useFactory: () => new TauriUpdater() },
  ],
};
