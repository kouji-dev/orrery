import { PerfStore } from "./perf.store";

/**
 * Feeds JS main-thread long tasks (>50ms blocks, the `longtask` performance
 * entry) into the {@link PerfStore} as the pseudo-row `js_longtask`:
 * calls/10s = long tasks in the window, max RT = worst main-thread stall.
 * This is the direct gauge for jank that per-command round-trips only hint at.
 * `longtask` may be unsupported by the webview → try/catch no-op (returns null).
 */
export function startLongTaskProbe(perf: PerfStore): PerformanceObserver | null {
  try {
    const obs = new PerformanceObserver((list) => {
      for (const e of list.getEntries()) perf.record("js_longtask", e.duration, true);
    });
    // buffered: catch the startup burst that happened before we attached
    obs.observe({ type: "longtask", buffered: true });
    return obs;
  } catch {
    return null; // entry type (or observer) unsupported → probe silently off
  }
}
