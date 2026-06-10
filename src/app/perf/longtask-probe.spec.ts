import { afterEach, describe, expect, it } from "vitest";
import { PerfStore } from "./perf.store";
import { startLongTaskProbe } from "./longtask-probe";

const realPO = globalThis.PerformanceObserver;
afterEach(() => {
  (globalThis as { PerformanceObserver?: unknown }).PerformanceObserver = realPO;
});

function stub(po: unknown) {
  (globalThis as { PerformanceObserver?: unknown }).PerformanceObserver = po;
}

describe("startLongTaskProbe", () => {
  it("records observed long tasks as the js_longtask pseudo-row", () => {
    let cb: PerformanceObserverCallback | undefined;
    class FakePO {
      constructor(c: PerformanceObserverCallback) {
        cb = c;
      }
      observe() {}
      disconnect() {}
    }
    stub(FakePO);
    const s = new PerfStore();
    expect(startLongTaskProbe(s)).not.toBeNull();
    const list = { getEntries: () => [{ duration: 120 }, { duration: 64 }] };
    cb!(list as unknown as PerformanceObserverEntryList, {} as PerformanceObserver);
    const r = s.rows().find((x) => x.cmd === "js_longtask")!;
    expect(r.calls10s).toBe(2); // long tasks in the window
    expect(r.maxRt).toBe(120); // worst main-thread stall
    expect(r.errPct).toBe(0);
  });

  it("no-ops when the longtask entry type is unsupported", () => {
    class ThrowPO {
      observe() {
        throw new TypeError("unsupported entryTypes");
      }
    }
    stub(ThrowPO);
    expect(startLongTaskProbe(new PerfStore())).toBeNull();
  });

  it("no-ops when PerformanceObserver is missing entirely", () => {
    stub(undefined);
    expect(startLongTaskProbe(new PerfStore())).toBeNull();
  });
});
