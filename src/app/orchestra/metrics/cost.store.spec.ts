import { Injector, runInInjectionContext } from "@angular/core";
import { describe, expect, it } from "vitest";
import { BRIDGE, Bridge, Events } from "../data-source/bridge";
import { CostSnapshot } from "../models";
import { CostStore } from "./cost.store";

// Bridge stub capturing the `system://cost` handler so the test drives it
// directly. invoke (the optional prime) rejects — we only assert event→signal.
// Mirrors metrics.store.spec.ts (this repo's vitest has no Angular test env).
function makeStore() {
  let handler: ((c: CostSnapshot) => void) | undefined;
  const bridge = {
    on<T>(event: string, cb: (p: T) => void): Promise<() => void> {
      if (event === Events.SystemCost) handler = cb as (c: CostSnapshot) => void;
      return Promise.resolve(() => {});
    },
    invoke<R>(): Promise<R> {
      return Promise.reject(new Error("no initial value"));
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
});
