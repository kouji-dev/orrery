import { Component, provideZonelessChangeDetection, signal } from "@angular/core";
import { ComponentFixture, TestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { AgentActionsService } from "../agents/agent-actions.service";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { Settings } from "../models";
import { ProjectActionsService } from "../projects/project-actions.service";
import { settingsDefaults, SettingsStore } from "../settings/settings.store";
import { IconComponent } from "../shared/icon.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { UiStore } from "../ui/ui.store";
import { SpawnModalComponent } from "./spawn-modal.component";

beforeAll(() => {
  try {
    TestBed.initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by another spec in this worker
  }
});

afterEach(() => TestBed.resetTestingModule());

// signal-input shared components can't be JIT-compiled by raw vitest (NG0950) —
// same-selector stubs keep the modal's own template fully exercised.
@Component({ selector: "app-icon", template: "", inputs: ["name", "size", "px", "color"] })
class IconStub {}
@Component({ selector: "app-tool-badge", template: "", inputs: ["tool", "size"] })
class ToolBadgeStub {}

interface Setup {
  cmp: SpawnModalComponent;
  fixture: ComponentFixture<SpawnModalComponent>;
  spawn: ReturnType<typeof vi.fn>;
}

function setup(opts: { settings?: Partial<Settings>; available?: (id: string) => boolean } = {}): Setup {
  const settings = signal<Settings>({ ...settingsDefaults(), ...opts.settings });
  const project = {
    id: "p1",
    name: "Proj",
    path: "C:/proj",
    icon: "",
    color: "",
    folderExists: true,
    hasGit: true,
    branches: ["main", "dev"],
    head: "abc1234",
  };
  const spawn = vi.fn();
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      { provide: UiStore, useValue: { spawning: signal({ project: null }), closeSpawn: vi.fn(), worktreeRoot: "~/wt" } },
      { provide: ProjectActionsService, useValue: { all: signal([project]) } },
      { provide: AgentRuntimeService, useValue: { toolAvailable: opts.available ?? (() => true) } },
      { provide: AgentActionsService, useValue: { spawn } },
      { provide: SettingsStore, useValue: { settings } },
    ],
  });
  TestBed.overrideComponent(SpawnModalComponent, {
    remove: { imports: [IconComponent, ToolBadgeComponent] },
    add: { imports: [IconStub, ToolBadgeStub] },
  });
  const fixture = TestBed.createComponent(SpawnModalComponent);
  fixture.detectChanges();
  return { cmp: fixture.componentInstance, fixture, spawn };
}

describe("SpawnModal — settings prefill", () => {
  it("no saved defaults: keeps the hardcoded claude / first model / no effort", () => {
    const { cmp } = setup();
    expect(cmp.toolId()).toBe("claude");
    expect(cmp.model()).toBe("opus");
    expect(cmp.effort()).toBeNull();
  });

  it("defaultTool prefills the initial tool (with its model + effort defaults)", () => {
    const { cmp } = setup({ settings: { defaultTool: "codex" } });
    expect(cmp.toolId()).toBe("codex");
    expect(cmp.model()).toBe("gpt-5.1-codex"); // curated first model
    expect(cmp.effort()).toBe("high"); // hardcoded effort default
  });

  it("per-tool toolModel/toolEffort overrides win over the curated defaults", () => {
    const { cmp } = setup({
      settings: {
        defaultTool: "codex",
        toolModel: { codex: "gpt-5.1-codex-mini" },
        toolEffort: { codex: "low" },
      },
    });
    expect(cmp.model()).toBe("gpt-5.1-codex-mini");
    expect(cmp.effort()).toBe("low");
  });

  it("defaultTool that is NOT detected falls back to the hardcoded default", () => {
    const { cmp } = setup({
      settings: { defaultTool: "gemini" },
      available: (id) => id !== "gemini",
    });
    expect(cmp.toolId()).toBe("claude");
  });

  it("unknown saved defaultTool falls back to the hardcoded default", () => {
    const { cmp } = setup({ settings: { defaultTool: "no-such-tool" } });
    expect(cmp.toolId()).toBe("claude");
  });

  it("switching tool applies THAT tool's settings defaults", () => {
    const { cmp } = setup({
      settings: { toolModel: { codex: "gpt-5.1-codex-mini", claude: "haiku" }, toolEffort: { codex: "medium" } },
    });
    expect(cmp.model()).toBe("haiku"); // claude override applies on open
    cmp.setTool("codex");
    expect(cmp.model()).toBe("gpt-5.1-codex-mini");
    expect(cmp.effort()).toBe("medium");
    cmp.setTool("cursor"); // no override → curated default, effort unsupported
    expect(cmp.model()).toBe("composer-1");
    expect(cmp.effort()).toBeNull();
  });

  it("a STALE persisted model/effort (no longer curated) falls back to the defaults", () => {
    const { cmp } = setup({
      settings: {
        defaultTool: "codex",
        toolModel: { codex: "gpt-4-classic" },
        toolEffort: { codex: "ultra" },
      },
    });
    expect(cmp.model()).toBe("gpt-5.1-codex"); // first curated
    expect(cmp.effort()).toBe("high");
  });
});
