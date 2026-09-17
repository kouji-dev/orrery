import { Injector, runInInjectionContext, signal } from "@angular/core";
import { describe, expect, it } from "vitest";
import { SettingsStore } from "../settings/settings.store";
import { COST_FEATURES_ENABLED } from "./cost-flags";
import { EstimateService } from "./estimate.service";

// SettingsStore stub with an aggressive budget (cap $1, confirm above $0.01):
// with the kill switch ON these settings would block/confirm any big rebase.
// The specs prove that with the switch OFF every guard is inert — the cost
// chrome can vanish without breaking the git action buttons.
function makeService() {
  const settings = {
    settings: signal({ budgetCapUsd: 1, confirmAboveUsd: 0.01, costRates: {} }),
  } as unknown as SettingsStore;
  const injector = Injector.create({ providers: [{ provide: SettingsStore, useValue: settings }] });
  return runInInjectionContext(injector, () => new EstimateService());
}

describe("EstimateService kill switch (COST_FEATURES_ENABLED=false)", () => {
  it("the switch is off", () => {
    expect(COST_FEATURES_ENABLED).toBe(false);
  });

  it("a huge rebase estimate never trips the cap or confirm guards", () => {
    const svc = makeService();
    const est = svc.estimate({ op: "rebase", files: 200, diffBytes: 5_000_000, conflicts: 30, verbose: true });
    expect(est.usdHigh).toBeGreaterThan(1); // would exceed the $1 cap if enforced
    expect(svc.overCap(est)).toBe(false);
    expect(svc.needsConfirm(est)).toBe(false);
  });

  it("recordSpend is a no-op — the session tally stays at 0", () => {
    const svc = makeService();
    const est = svc.estimate({ op: "merge", files: 10, diffBytes: 100_000, conflicts: 3 });
    svc.recordSpend(est);
    svc.recordSpend(est);
    expect(svc.sessionSpentUsd()).toBe(0);
  });
});
