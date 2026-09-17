import { Injector, runInInjectionContext } from "@angular/core";
import { describe, expect, it } from "vitest";
import { BRIDGE, Bridge, Events } from "../data-source/bridge";
import { SystemMetrics } from "../models";
import { MetricsStore } from "./metrics.store";

// A bridge stub that captures the `system://metrics` handler so the test can
// drive it directly. invoke (the optional initial fetch) just hangs/rejects —
// we only assert the event→signal mapping. No TestBed/zone needed (this repo's
// vitest setup has no Angular test environment).
function makeStore() {
  let handler: ((m: SystemMetrics) => void) | undefined;
  const bridge = {
    on<T>(event: string, cb: (p: T) => void): Promise<() => void> {
      if (event === Events.SystemMetrics) handler = cb as (m: SystemMetrics) => void;
      return Promise.resolve(() => {});
    },
    invoke<R>(): Promise<R> {
      return Promise.reject(new Error("no initial value")); // skip the prime path
    },
    pickDirectory: () => Promise.resolve(null),
  } as unknown as Bridge;

  const injector = Injector.create({ providers: [{ provide: BRIDGE, useValue: bridge }] });
  const store = runInInjectionContext(injector, () => new MetricsStore());
  return { store, emit: (m: SystemMetrics) => handler?.(m) };
}

const sample: SystemMetrics = {
  totalCpu: 12.4, // sum of the rows below (machine-relative %)
  totalMemBytes: 540 * 1024 * 1024, // sum of the rows below (200 + 340 MB)
  procs: [
    { id: "app", label: "orrery", cpu: 4.0, memBytes: 200 * 1024 * 1024 },
    { id: "uuid-1", label: "nova", cpu: 8.4, memBytes: 340 * 1024 * 1024 },
  ],
};

describe("MetricsStore", () => {
  it("starts null before any push", () => {
    expect(makeStore().store.metrics()).toBeNull();
  });

  it("mirrors a pushed system://metrics payload into the signal", async () => {
    const { store, emit } = makeStore();
    await Promise.resolve(); // let init() subscribe
    emit(sample);
    expect(store.metrics()).toEqual(sample);
    expect(store.metrics()?.procs.length).toBe(2);
  });

  it("keeps the latest pushed snapshot", async () => {
    const { store, emit } = makeStore();
    await Promise.resolve();
    emit(sample);
    const next: SystemMetrics = { ...sample, totalCpu: 50, procs: [] };
    emit(next);
    expect(store.metrics()?.totalCpu).toBe(50);
    expect(store.metrics()?.procs).toEqual([]);
  });
});
