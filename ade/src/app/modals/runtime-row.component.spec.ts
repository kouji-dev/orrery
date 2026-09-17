import { Component, provideZonelessChangeDetection } from "@angular/core";
import { ComponentFixture, TestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { BRIDGE } from "../data-source/bridge";
import { ToolDetection } from "../models";
import { SettingsStore } from "../settings/settings.store";
import { IconComponent } from "../shared/icon.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { RuntimeRowComponent } from "./runtime-row.component";

beforeAll(() => {
  try {
    TestBed.initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    /* already initialized */
  }
});
afterEach(() => TestBed.resetTestingModule());

// Real icon/badge use signal inputs (NG0950 under raw vitest JIT) — stub them.
@Component({ selector: "app-icon", template: "", inputs: ["name", "size", "px", "color"] })
class IconStub {}
@Component({ selector: "app-tool-badge", template: "", inputs: ["tool", "size"] })
class ToolBadgeStub {}

function det(p: Partial<ToolDetection> & { status: ToolDetection["status"] }): ToolDetection {
  return {
    id: "cursor",
    available: p.status === "ok",
    path: null,
    version: null,
    source: null,
    reason: null,
    shim: false,
    ...p,
  };
}

interface Mocks {
  setMap: ReturnType<typeof vi.fn>;
  verifyToolPath: ReturnType<typeof vi.fn>;
  setDetection: ReturnType<typeof vi.fn>;
  refreshDetections: ReturnType<typeof vi.fn>;
  /** The idempotent first-demand sweep the row asks for when it mounts. */
  ensureDetections: ReturnType<typeof vi.fn>;
  pickFile: ReturnType<typeof vi.fn>;
}

function mount(detection: ToolDetection | null, pending = false): {
  cmp: RuntimeRowComponent;
  fixture: ComponentFixture<RuntimeRowComponent>;
  el: HTMLElement;
  m: Mocks;
} {
  const m: Mocks = {
    setMap: vi.fn(),
    verifyToolPath: vi.fn(),
    setDetection: vi.fn(),
    refreshDetections: vi.fn(async () => {}),
    ensureDetections: vi.fn(),
    pickFile: vi.fn(async () => null),
  };
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      { provide: SettingsStore, useValue: { setMap: m.setMap } },
      {
        provide: AgentRuntimeService,
        useValue: {
          detection: () => detection,
          detectionPending: () => pending,
          verifyToolPath: m.verifyToolPath,
          setDetection: m.setDetection,
          refreshDetections: m.refreshDetections,
          ensureDetections: m.ensureDetections,
        },
      },
      { provide: BRIDGE, useValue: { pickFile: m.pickFile } },
    ],
  });
  TestBed.overrideComponent(RuntimeRowComponent, {
    remove: { imports: [IconComponent, ToolBadgeComponent] },
    add: { imports: [IconStub, ToolBadgeStub] },
  });
  const fixture = TestBed.createComponent(RuntimeRowComponent);
  fixture.componentRef.setInput("toolId", "cursor");
  fixture.componentRef.setInput("toolName", "Cursor");
  fixture.detectChanges();
  return { cmp: fixture.componentInstance, fixture, el: fixture.nativeElement, m };
}

describe("RuntimeRowComponent", () => {
  it("ok: shows the resolved path chip + Change, no editor", () => {
    const { el } = mount(det({ status: "ok", path: "/usr/local/bin/cursor-agent", version: "2.1.0", source: "path" }));
    expect(el.querySelector(".set-rt-pathchip .pt")?.textContent).toContain("/usr/local/bin/cursor-agent");
    expect(el.querySelector(".set-rt-st.ok")?.textContent).toContain("v2.1.0");
    expect(el.querySelector(".set-rt-input")).toBeNull(); // editor closed
  });

  it("ok + shim install: shows the native-installer hint line", () => {
    const { el } = mount(det({ status: "ok", path: "C:\\npm\\cursor-agent.cmd", source: "path", shim: true }));
    expect(el.querySelector(".set-rt-shim")?.textContent).toContain("native installer");
  });

  it("ok + native install: no shim hint", () => {
    const { el } = mount(det({ status: "ok", path: "/usr/local/bin/cursor-agent", source: "path" }));
    expect(el.querySelector(".set-rt-shim")).toBeNull();
  });

  it("error: shows the reason + locate/verify editor", () => {
    const { el } = mount(
      det({ status: "error", path: "/bad/cursor", source: "path", reason: "found, but exited with code 126" }),
    );
    expect(el.querySelector(".set-rt")?.classList.contains("warn")).toBe(true);
    expect(el.querySelector(".set-rt-reason")?.textContent).toContain("exited with code 126");
    expect(el.querySelector(".set-rt-input")).not.toBeNull(); // editor open
  });

  it("missing: shows the locate editor with 'Use this path'", () => {
    const { el } = mount(det({ status: "missing" }));
    expect(el.querySelector(".set-rt-input")).not.toBeNull();
    expect(el.querySelector('kj-button[kjVariant="default"]')?.textContent).toContain("Use this path");
  });

  it("probe still out: says checking, and offers no locate editor yet", () => {
    // detection() is null both before the sweep answers and when the tool is
    // genuinely absent — only detectionPending() tells the two apart, and
    // prompting for a path for a tool that may well be on PATH is noise.
    const { el } = mount(null, true);
    expect(el.querySelector(".set-rt-st")?.textContent).toContain("checking");
    expect(el.querySelector(".set-rt-st kj-spinner")).not.toBeNull();
    expect(el.textContent).not.toContain("not installed");
    expect(el.querySelector(".set-rt-input")).toBeNull();
  });

  it("verify success: persists toolPath + folds in the detection + closes editor", async () => {
    const { cmp, m } = mount(det({ status: "missing" }));
    m.verifyToolPath.mockResolvedValue(
      det({ status: "ok", path: "/opt/cursor", version: "2.1.0", source: "manual" }),
    );
    cmp.draft.set("/opt/cursor");
    await cmp.verify();
    expect(m.verifyToolPath).toHaveBeenCalledWith("cursor", "/opt/cursor");
    expect(m.setMap).toHaveBeenCalledWith("toolPath", "cursor", "/opt/cursor");
    expect(m.setDetection).toHaveBeenCalled();
    expect(cmp.editing()).toBe(false);
    expect(cmp.fail()).toBeNull();
  });

  it("verify failure: surfaces the reason and does NOT persist", async () => {
    const { cmp, m } = mount(det({ status: "missing" }));
    m.verifyToolPath.mockResolvedValue(det({ status: "error", path: "/opt/cursor", reason: "couldn’t launch — not executable" }));
    cmp.draft.set("/opt/cursor");
    await cmp.verify();
    expect(cmp.fail()).toContain("not executable");
    expect(m.setMap).not.toHaveBeenCalled();
    expect(m.setDetection).not.toHaveBeenCalled();
  });

  it("verify with blank path: validates without calling the backend", async () => {
    const { cmp, m } = mount(det({ status: "missing" }));
    cmp.draft.set("   ");
    await cmp.verify();
    expect(m.verifyToolPath).not.toHaveBeenCalled();
    expect(cmp.fail()).toContain("Enter the full path");
  });

  it("revert: clears the override and FORCES a re-detect, not the idempotent demand", async () => {
    const { cmp, m } = mount(det({ status: "ok", path: "/opt/cursor", source: "manual" }));
    m.ensureDetections.mockClear(); // the mount already spent the first demand
    await cmp.revert();
    expect(m.setMap).toHaveBeenCalledWith("toolPath", "cursor", null);
    expect(m.refreshDetections).toHaveBeenCalled();
    // ensureDetections would no-op here — a reverted path MUST be re-probed
    expect(m.ensureDetections).not.toHaveBeenCalled();
  });

  it("mounting the row demands the sweep — nothing detects at boot any more", () => {
    const { m } = mount(det({ status: "ok", path: "/opt/cursor", source: "path" }));
    expect(m.ensureDetections).toHaveBeenCalledTimes(1);
  });
});
