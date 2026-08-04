import { Injector, runInInjectionContext } from "@angular/core";
import { describe, expect, it } from "vitest";
import { BRIDGE, Bridge, Events } from "../data-source/bridge";
import { CostSnapshot } from "../models";
import { CostStore } from "./cost.store";

// Bridge stub capturing the `system://cost` handler so the test drives it
// directly. invoke (the optional prime) resolves null — the backend's one-shot
// is a cache PEEK that returns null while the push loop's first ccusage run is
// still in flight (A1.8) — so these specs also cover that startup shape.
// Mirrors metrics.store.spec.ts (this repo's vitest has no Angular test env).
function makeStore() {
  let handler: ((c: CostSnapshot) => void) | undefined;
  const bridge = {
    on<T>(event: string, cb: (p: T) => void): Promise<() => void> {
      if (event === Events.SystemCost) handler = cb as (c: CostSnapshot) => void;
      return Promise.resolve(() => {});
    },
    invoke<R>(): Promise<R> {
      return Promise.resolve(null as R);
    },
    pickDirectory: () => Promise.resolve(null),
  } as unknown as Bridge;

  const injector = Injector.create({ providers: [{ provide: BRIDGE, useValue: bridge }] });
  const store = runInInjectionContext(injector, () => new CostStore());
  return { store, emit: (c: CostSnapshot) => handler?.(c) };
}

describe("CostStore", () => {
  it("starts null before any push", () => {
    expect(makeStore().store.cost()).toBeNull();
  });

  it("mirrors a pushed system://cost payload into the signal", async () => {
    const { store, emit } = makeStore();
    await Promise.resolve(); // let init() subscribe
    const snap: CostSnapshot = { totalCost: 42.5, currency: "USD", available: true };
    emit(snap);
    expect(store.cost()?.totalCost).toBe(42.5);
    expect(store.cost()?.available).toBe(true);
  });

  it("a null one-shot (cold cache) leaves the signal untouched for the push", async () => {
    const { store, emit } = makeStore();
    // Drain init(): subscribe + the null invoke resolution.
    await Promise.resolve();
    await Promise.resolve();
    expect(store.cost()).toBeNull();
    emit({ totalCost: 1.25, currency: "USD", available: true });
    expect(store.cost()?.totalCost).toBe(1.25);
  });
});
