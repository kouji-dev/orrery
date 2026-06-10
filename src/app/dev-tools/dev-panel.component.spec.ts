import { Component, provideZonelessChangeDetection, signal } from "@angular/core";
import { ComponentFixture, TestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ProjectsStore } from "../stores/projects.store";
import { IconComponent } from "../shared/icon.component";
import { StatusDotComponent } from "../shared/status-dot.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { PerfRow, PerfStore } from "../perf/perf.store";
import {
  resetTerminalSchedulerForTests,
  terminalSchedulerStats,
  writeScheduled,
} from "../terminal-output-scheduler";
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
    bytes10s: p.bytes10s ?? null,
    stale: p.stale ?? false,
    hist: p.hist ?? [],
    recent: p.recent ?? [],
  };
}

beforeAll(() => {
  try {
    TestBed.initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by another spec in this worker
  }
});

afterEach(() => {
  TestBed.resetTestingModule(); // destroys fixtures → runs ngOnDestroy
  resetTerminalSchedulerForTests();
});

// The real shared components use signal inputs (input.required), which raw
// vitest JIT cannot wire (NG0950 — needs the CLI's signal-input transform).
// Same-selector stubs keep the dev-panel template itself fully exercised.
@Component({ selector: "app-icon", template: "", inputs: ["name", "size", "px", "color"] })
class IconStub {}
@Component({ selector: "app-status-dot", template: "", inputs: ["status"] })
class StatusDotStub {}
@Component({ selector: "app-tool-badge", template: "", inputs: ["tool", "size"] })
class ToolBadgeStub {}

/** Real TestBed render (not a bare `new`): this is what catches a non-callable
 *  `open` — the template invokes `open()` and the constructor needs a proper
 *  injection context for the stats-gate effect. */
function setup(rows: PerfRow[] = []): ComponentFixture<DevPanelComponent> {
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      { provide: PerfStore, useValue: { rows: signal(rows), tick() {}, clear() {} } },
      { provide: AgentRuntimeService, useValue: { agents: signal([]), elapsedFor: () => 0 } },
      { provide: ProjectsStore, useValue: { all: signal([]) } },
    ],
  });
  TestBed.overrideComponent(DevPanelComponent, {
    remove: { imports: [IconComponent, StatusDotComponent, ToolBadgeComponent] },
    add: { imports: [IconStub, StatusDotStub, ToolBadgeStub] },
  });
  const fixture = TestBed.createComponent(DevPanelComponent);
  fixture.detectChanges();
  return fixture;
}

describe("DevPanelComponent open gate", () => {
  it("open() is callable from the template and toggles the panel", () => {
    const fixture = setup();
    const el: HTMLElement = fixture.nativeElement;
    expect(el.querySelector(".dvcon")).toBeNull();

    (el.querySelector(".dvc-fab") as HTMLButtonElement).click();
    fixture.detectChanges();
    expect(fixture.componentInstance.open()).toBe(true);
    expect(el.querySelector(".dvcon")).not.toBeNull();

    (el.querySelector(".dvc-fab") as HTMLButtonElement).click();
    fixture.detectChanges();
    expect(el.querySelector(".dvcon")).toBeNull();
  });

  it("enables scheduler stats while open, disables on close and on destroy", () => {
    const fixture = setup();
    const term = { write: (_d: string, cb?: () => void) => cb?.() };
    const direct = () => terminalSchedulerStats().directWrites;

    // closed → collector off, writes don't count
    writeScheduled("a1", term, "x", { visible: true });
    expect(direct()).toBe(0);

    // open → effect flips the gate on (fresh counters)
    fixture.componentInstance.open.set(true);
    fixture.detectChanges();
    writeScheduled("a1", term, "x", { visible: true });
    expect(direct()).toBe(1);

    // close → gate off again, counter frozen
    fixture.componentInstance.open.set(false);
    fixture.detectChanges();
    writeScheduled("a1", term, "x", { visible: true });
    expect(direct()).toBe(1);

    // destroy with the panel open → explicit ngOnDestroy off-switch
    fixture.componentInstance.open.set(true);
    fixture.detectChanges();
    expect(direct()).toBe(0); // re-enable resets counters
    fixture.destroy();
    writeScheduled("a1", term, "x", { visible: true });
    expect(direct()).toBe(0);
  });
});

describe("DevPanelComponent.copyPerf", () => {
  it("copies aggregates as a JSON envelope, slowest first, with hist/recent dropped and floats rounded", async () => {
    const writeText = vi.fn(() => Promise.resolve());
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });

    const cmp = setup([
      row({ cmd: "fast", calls10s: 40, avgRt: 8.13, avgExec: 6.2, overhead: 1.93, p95Rt: 14, maxRt: 22, errPct: 0 }),
      row({ cmd: "slow", calls10s: 12, avgRt: 142.34, avgExec: 95.1, overhead: 47.24, p95Rt: 210, maxRt: 318, errPct: 0 }),
      row({ cmd: "idle", calls10s: 0, avgRt: null, avgExec: null, overhead: null, p95Rt: null, maxRt: null, errPct: 0 }),
    ]).componentInstance;

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
  });

  it("does not throw and leaves copied=false when the clipboard write rejects", async () => {
    const writeText = vi.fn(() => Promise.reject(new Error("denied")));
    Object.defineProperty(navigator, "clipboard", { value: { writeText }, configurable: true });

    const cmp = setup([row({ cmd: "x", calls10s: 1, avgRt: 5 })]).componentInstance;
    cmp.copyPerf();
    await Promise.resolve();
    expect(cmp.copied()).toBe(false);
  });
});
