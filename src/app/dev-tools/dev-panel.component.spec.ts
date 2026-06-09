import { ElementRef, Injector, runInInjectionContext, signal } from "@angular/core";
import { describe, expect, it, vi } from "vitest";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ProjectsStore } from "../stores/projects.store";
import { PerfRow, PerfStore } from "../perf/perf.store";
import { DevPanelComponent } from "./dev-panel.component";

function row(p: Partial<PerfRow> & { cmd: string }): PerfRow {
  return {
    cmd: p.cmd,
    calls10s: p.calls10s ?? 0,
    avgRt: p.avgRt ?? null,
    p95Rt: p.p95Rt ?? null,
    maxRt: p.maxRt ?? null,
    errPct: p.errPct ?? 0,
    avgExec: p.avgExec ?? null,
    overhead: p.overhead ?? null,
    hist: p.hist ?? [],
    recent: p.recent ?? [],
  };
}

function make(rows: PerfRow[]) {
  const injector = Injector.create({
    providers: [
      { provide: PerfStore, useValue: { rows: signal(rows), tick() {}, clear() {} } },
      { provide: AgentRuntimeService, useValue: { agents: signal([]) } },
      { provide: ProjectsStore, useValue: { all: signal([]) } },
      { provide: ElementRef, useValue: { nativeElement: document.createElement("div") } },
    ],
  });
  return runInInjectionContext(injector, () => new DevPanelComponent());
}

describe("DevPanelComponent.copyPerf", () => {
  it("copies aggregates as a JSON envelope, slowest first, with hist/recent dropped and floats rounded", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });

    const cmp = make([
      row({ cmd: "fast", calls10s: 40, avgRt: 8.13, avgExec: 6.2, overhead: 1.93, p95Rt: 14, maxRt: 22, errPct: 0 }),
      row({ cmd: "slow", calls10s: 12, avgRt: 142.34, avgExec: 95.1, overhead: 47.24, p95Rt: 210, maxRt: 318, errPct: 0 }),
      row({ cmd: "idle", calls10s: 0, avgRt: null, avgExec: null, overhead: null, p95Rt: null, maxRt: null, errPct: 0 }),
    ]);

    cmp.copyPerf();
    expect(writeText).toHaveBeenCalledOnce();

    const payload = JSON.parse(writeText.mock.calls[0][0] as string);
    expect(payload.tier).toBe("dev");
    expect(payload.windowMs).toBe(10_000);
    expect(typeof payload.capturedAt).toBe("string");

    // visible (sorted) order — default sort is avgRT desc, so slowest first
    expect(payload.rows.map((r: { cmd: string }) => r.cmd)).toEqual(["slow", "fast", "idle"]);

    // floats rounded to 1dp; aggregates only (no hist/recent)
    expect(payload.rows[0]).toEqual({
      cmd: "slow",
      calls10s: 12,
      avgRt: 142.3,
      avgExec: 95.1,
      overhead: 47.2,
      p95Rt: 210,
      maxRt: 318,
      errPct: 0,
    });
    expect(payload.rows[0]).not.toHaveProperty("hist");
    expect(payload.rows[0]).not.toHaveProperty("recent");

    // nulls survive the round helper
    expect(payload.rows[2]).toMatchObject({ cmd: "idle", avgRt: null, overhead: null, maxRt: null });

    // success flips the transient "Copied" state
    await Promise.resolve();
    expect(cmp.copied()).toBe(true);

    cmp.ngOnDestroy();
  });

  it("does not throw and leaves copied=false when the clipboard write rejects", async () => {
    const writeText = vi.fn(() => Promise.reject(new Error("denied")));
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });

    const cmp = make([row({ cmd: "x", calls10s: 1, avgRt: 5 })]);
    cmp.copyPerf();
    await Promise.resolve();
    expect(cmp.copied()).toBe(false);

    cmp.ngOnDestroy();
  });
});
