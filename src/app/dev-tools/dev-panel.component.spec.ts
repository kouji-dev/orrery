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
import { MetricsStore } from "../metrics/metrics.store";
import { TelemetryStore } from "../metrics/telemetry.store";
import { BRIDGE, Commands } from "../data-source/bridge";
import { ProcessNode, ProcessTreeSnapshot, SystemMetrics } from "../models";
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
function setup(rows: PerfRow[] = [], metrics: SystemMetrics | null = null, tree: ProcessTreeSnapshot | null = null): ComponentFixture<DevPanelComponent> {
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      { provide: PerfStore, useValue: { rows: signal(rows), tick() {}, clear() {} } },
      { provide: AgentRuntimeService, useValue: { agents: signal([]), elapsedFor: () => 0 } },
      { provide: ProjectsStore, useValue: { all: signal([]) } },
      { provide: MetricsStore, useValue: { metrics: signal(metrics) } },
      // Resources/Emits tabs poll through the bridge only while visible; the
      // stub serves the given tree snapshot and rejects everything else.
      {
        provide: BRIDGE,
        useValue: {
          invoke: (cmd: string) =>
            cmd === Commands.ProcessTree && tree ? Promise.resolve(tree) : Promise.reject(new Error("stub")),
          on: () => Promise.resolve(() => {}),
        },
      },
      { provide: TelemetryStore, useValue: { traceActive: signal(false), traceReason: signal(null), setTrace() {} } },
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

describe("DevPanelComponent resources tab (merged tree)", () => {
  const SNAP: SystemMetrics = {
    totalCpu: 12.4,
    totalMemBytes: 3 * 2 ** 30,
    sysMemBytes: 16 * 2 ** 30,
    cores: 10,
    procs: [
      { id: "app", label: "Orrery", cpu: 4.2, memBytes: 800 * 2 ** 20 },
      { id: "ag-1", label: "refactor-auth", cpu: 8.2, memBytes: 2200 * 2 ** 20 },
    ],
  };

  const pn = (p: Partial<ProcessNode> & { pid: number; name: string }): ProcessNode => ({
    note: null,
    cpu: 0,
    privBytes: 0,
    rssBytes: 0,
    subtreeCpu: p.cpu ?? 0,
    subtreePrivBytes: p.privBytes ?? 0,
    subtreeProcs: 1,
    detached: false,
    excluded: false,
    children: [],
    ...p,
  });
  const TREE: ProcessTreeSnapshot = {
    roots: [
      {
        id: "app",
        label: "Orrery",
        node: pn({
          pid: 10,
          name: "orrery.exe",
          cpu: 2,
          privBytes: 100 * 2 ** 20,
          subtreeCpu: 4.2,
          subtreePrivBytes: 800 * 2 ** 20,
          subtreeProcs: 2,
          children: [pn({ pid: 11, name: "msedgewebview2.exe", cpu: 2.2, privBytes: 700 * 2 ** 20 })],
        }),
      },
      { id: "ag-1", label: "refactor-auth", node: pn({ pid: 20, name: "node.exe", cpu: 8.2, privBytes: 2200 * 2 ** 20 }) },
    ],
    tsMs: Date.now(),
  };

  async function openResources(fixture: ComponentFixture<DevPanelComponent>): Promise<HTMLElement> {
    const el: HTMLElement = fixture.nativeElement;
    (el.querySelector(".dvc-fab") as HTMLButtonElement).click();
    fixture.detectChanges();
    (Array.from(el.querySelectorAll(".dvc-tab")).find((b) => b.textContent?.includes("Resources")) as HTMLButtonElement).click();
    fixture.detectChanges();
    await new Promise((r) => setTimeout(r)); // drain the pollTree invoke
    fixture.detectChanges();
    return el;
  }

  it("renders gauges plus the tree rooted at an expanded Orrery App row summing everything", async () => {
    const fixture = setup([], SNAP, TREE);
    const el = await openResources(fixture);

    expect(el.querySelectorAll(".dvc-gauge").length).toBe(2);
    expect(el.textContent).toContain("12.4");
    expect(el.textContent).toContain("of 10 cores");

    // no RSS column in the merged table
    const heads = Array.from(el.querySelectorAll(".dvc-tbl thead th")).map((h) => h.textContent?.trim());
    expect(heads).toEqual(["Process", "PID", "CPU", "Private", "Subtree"]);

    // root first + expanded by default: root, orrery.exe, webview child, agent
    const rows = el.querySelectorAll(".dvc-tbl tbody tr");
    expect(rows.length).toBe(4);
    expect(rows[0].textContent).toContain("Orrery App");
    expect(rows[0].textContent).toContain("12.4%"); // 4.2 + 8.2 recursive cpu total
    expect(rows[0].textContent).toContain("2.9 GB"); // 800MB + 2200MB private total
    expect(rows[1].textContent).toContain("orrery.exe");
    expect(rows[2].textContent).toContain("msedgewebview2.exe");
    expect(rows[3].textContent).toContain("node.exe");
  });

  it("collapsing the Orrery App root hides every process row beneath it", async () => {
    const fixture = setup([], SNAP, TREE);
    const el = await openResources(fixture);
    (el.querySelector(".dvc-tbl tbody .dvc-twbtn") as HTMLButtonElement).click();
    fixture.detectChanges();
    const rows = el.querySelectorAll(".dvc-tbl tbody tr");
    expect(rows.length).toBe(1);
    expect(rows[0].textContent).toContain("Orrery App");
  });

  it("shows the sampling empty state before the first tree snapshot", async () => {
    const fixture = setup([], null, null); // bridge rejects → no tree
    const el = await openResources(fixture);
    // .dvc-empty became <kj-empty-state>
    expect(el.querySelector("kj-empty-state")).not.toBeNull();
    expect(el.textContent).toContain("No process tree yet");
  });
});

describe("DevPanelComponent alert badge + ungated perf", () => {
  it("badges the FAB with the count of breaching commands, hidden when clean", () => {
    const clean = setup([row({ cmd: "ok", calls10s: 5, avgRt: 9 })]);
    // the FAB count is <kj-overlay-badge>, which renders .kj-overlay-badge
    // only while it has a value (kjHidden drops the span entirely)
    expect((clean.nativeElement as HTMLElement).querySelector(".kj-overlay-badge")).toBeNull();
    TestBed.resetTestingModule();
    resetTerminalSchedulerForTests();

    const bad = setup([
      row({ cmd: "slow", calls10s: 2, avgRt: 140 }),
      row({ cmd: "erring", calls10s: 3, avgRt: 8, errPct: 1.2 }),
      row({ cmd: "frozen", calls10s: 0, avgRt: 500, stale: true }), // stale rows don't alert
    ]);
    const badge = (bad.nativeElement as HTMLElement).querySelector(".kj-overlay-badge");
    expect(badge?.textContent?.trim()).toBe("2");
  });

  it("perf row expand and the recent-calls feed render without a dev-tier gate", () => {
    const fixture = setup([row({ cmd: "agent_input", calls10s: 4, avgRt: 9, recent: [{ ts: Date.now(), ms: 9, ok: true }] })]);
    const el: HTMLElement = fixture.nativeElement;
    (el.querySelector(".dvc-fab") as HTMLButtonElement).click();
    fixture.detectChanges();

    expect(el.querySelector(".dvc-feed")).not.toBeNull(); // feed: previously dev-only
    (el.querySelector(".dvc-row") as HTMLTableRowElement).click();
    fixture.detectChanges();
    expect(el.querySelector(".dvc-detail")).not.toBeNull(); // expand: previously dev-only
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
