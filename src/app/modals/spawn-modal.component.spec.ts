import { Component, provideZonelessChangeDetection, signal } from "@angular/core";
import { ComponentFixture, TestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { AgentActionsService } from "../agents/agent-actions.service";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { Settings, Ticket } from "../models";
import { ProjectActionsService } from "../projects/project-actions.service";
import { settingsDefaults, SettingsStore } from "../settings/settings.store";
import { TicketsStore } from "../stores/tickets.store";
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

const PROJECT = {
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

const TICKET_TODO: Ticket = {
  id: "t1",
  title: "Fix the login bug",
  notes: "<p>Steps to reproduce</p>",
  status: "todo",
  projectId: "p1",
  agentId: null,
};

const TICKET_INPROG: Ticket = {
  id: "t2",
  title: "Refactor auth module",
  notes: "<p>See <strong>RFC</strong> doc</p>",
  status: "inprogress",
  projectId: "p1",
  agentId: null,
};

function makeTicketsStore(tickets: Ticket[] = []) {
  const map = new Map(tickets.map((t) => [t.id, t]));
  return {
    all: signal(tickets),
    byId: (id: string) => map.get(id),
  };
}

function makeUiStore(opts: { spawnTicketId?: string | null } = {}) {
  const spawnTicketId = signal<string | null>(opts.spawnTicketId ?? null);
  return {
    spawning: signal({ project: null }),
    closeSpawn: vi.fn(),
    clearSpawnTicket: vi.fn(() => spawnTicketId.set(null)),
    spawnTicketId,
    worktreeRoot: "~/wt",
  };
}

function setup(
  opts: {
    settings?: Partial<Settings>;
    available?: (id: string) => boolean;
    tickets?: Ticket[];
    spawnTicketId?: string | null;
  } = {},
): Setup {
  const settings = signal<Settings>({ ...settingsDefaults(), ...opts.settings });
  const spawn = vi.fn();
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      { provide: UiStore, useValue: makeUiStore({ spawnTicketId: opts.spawnTicketId }) },
      { provide: ProjectActionsService, useValue: { all: signal([PROJECT]) } },
      { provide: AgentRuntimeService, useValue: { toolAvailable: opts.available ?? (() => true) } },
      { provide: AgentActionsService, useValue: { spawn } },
      { provide: SettingsStore, useValue: { settings } },
      { provide: TicketsStore, useValue: makeTicketsStore(opts.tickets ?? []) },
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

describe("SpawnModal — Ticket field", () => {
  it("selecting a ticket prefills the Name but leaves the Initial prompt empty", () => {
    const { cmp } = setup({ tickets: [TICKET_TODO] });
    expect(cmp.ticketId()).toBe("");
    cmp.applyTicket("t1");
    expect(cmp.ticketId()).toBe("t1");
    expect(cmp.name()).toBe("fix-the-login-bug");
    // The Initial prompt is NOT prefilled from the ticket anymore.
    expect(cmp.prompt()).toBe("");
  });

  it("selecting None clears the ticketId but leaves name/prompt unchanged", () => {
    const { cmp } = setup({ tickets: [TICKET_TODO] });
    cmp.applyTicket("t1");
    const nameBefore = cmp.name();
    cmp.applyTicket("");
    expect(cmp.ticketId()).toBe("");
    expect(cmp.name()).toBe(nameBefore); // unchanged
  });

  it("does NOT overwrite name when the user has manually edited it", () => {
    const { cmp } = setup({ tickets: [TICKET_TODO] });
    cmp.onNameInput("my-custom-name");
    cmp.applyTicket("t1");
    expect(cmp.name()).toBe("my-custom-name"); // user input preserved
  });

  it("selecting a ticket never touches the Initial prompt the user typed", () => {
    const { cmp } = setup({ tickets: [TICKET_TODO] });
    cmp.onPromptInput("my custom task description");
    cmp.applyTicket("t1");
    expect(cmp.prompt()).toBe("my custom task description"); // preserved
  });

  it("submit passes ticketId when a ticket is selected", () => {
    const { cmp, spawn } = setup({ tickets: [TICKET_TODO] });
    cmp.applyTicket("t1");
    cmp.submit(true);
    expect(spawn).toHaveBeenCalledWith(
      expect.objectContaining({ ticketId: "t1", start: true }),
    );
  });

  it("composes 'Implement <ticket>, <user prompt>' when a ticket is linked", () => {
    const { cmp, spawn } = setup({ tickets: [TICKET_TODO] });
    cmp.applyTicket("t1");
    cmp.onPromptInput("focus on the OAuth path");
    cmp.branch.set("main");
    cmp.submit(true);
    expect(spawn.mock.calls[0][0].prompt).toBe(
      "Implement Fix the login bug\n\nSteps to reproduce, focus on the OAuth path",
    );
  });

  it("composes just 'Implement <ticket>' (no trailing comma) when the prompt is empty", () => {
    const { cmp, spawn } = setup({ tickets: [TICKET_TODO] });
    cmp.applyTicket("t1");
    cmp.branch.set("main");
    cmp.submit(true);
    expect(spawn.mock.calls[0][0].prompt).toBe("Implement Fix the login bug\n\nSteps to reproduce");
  });

  it("uses just the user prompt when no ticket is linked", () => {
    const { cmp, spawn } = setup({ tickets: [TICKET_TODO] });
    cmp.onNameInput("manual-name");
    cmp.onPromptInput("do the thing");
    cmp.branch.set("main");
    cmp.submit(false);
    expect(spawn.mock.calls[0][0].prompt).toBe("do the thing");
  });

  it("submit omits ticketId when None is selected", () => {
    const { cmp, spawn } = setup({ tickets: [TICKET_TODO] });
    cmp.name.set("some-name");
    cmp.branch.set("main");
    cmp.submit(false);
    const call = spawn.mock.calls[0][0];
    expect(call.ticketId).toBeUndefined();
  });

  it("Dispatch path: spawnTicketId on open defaults the selection and clears the signal", () => {
    const { cmp } = setup({ tickets: [TICKET_TODO], spawnTicketId: "t1" });
    // ngOnInit already ran — ticket should be applied
    expect(cmp.ticketId()).toBe("t1");
    expect(cmp.name()).toBe("fix-the-login-bug");
    // clearSpawnTicket should have been called (signal now null via mock)
    const ui = TestBed.inject(UiStore) as ReturnType<typeof makeUiStore>;
    expect(ui.clearSpawnTicket).toHaveBeenCalled();
    expect(ui.spawnTicketId()).toBeNull();
  });

  it("Dispatch path: the rendered Ticket <select> reflects the dispatched ticket (not None)", () => {
    const { fixture } = setup({ tickets: [TICKET_TODO, TICKET_INPROG], spawnTicketId: "t1" });
    const selects = Array.from(
      (fixture.nativeElement as HTMLElement).querySelectorAll("select"),
    ) as HTMLSelectElement[];
    // the Ticket select is the one whose first option is the "None" sentinel
    const ticketSelect = selects.find((s) => s.options[0]?.textContent?.includes("None"))!;
    expect(ticketSelect).toBeTruthy();
    // Regression: the option must be marked selected as it renders — relying on
    // the <select>'s [value] alone fails because options render after it.
    expect(ticketSelect.value).toBe("t1");
    expect(ticketSelect.selectedOptions[0]?.textContent).toContain("Fix the login bug");
  });

  it("openTicketsTodo lists only todo tickets", () => {
    const { cmp } = setup({ tickets: [TICKET_TODO, TICKET_INPROG] });
    expect(cmp.openTicketsTodo().map((t) => t.id)).toEqual(["t1"]);
  });

  it("openTicketsInProgress lists only inprogress tickets", () => {
    const { cmp } = setup({ tickets: [TICKET_TODO, TICKET_INPROG] });
    expect(cmp.openTicketsInProgress().map((t) => t.id)).toEqual(["t2"]);
  });

  it("strips HTML from the ticket notes in the composed prompt", () => {
    const { cmp, spawn } = setup({ tickets: [TICKET_INPROG] });
    cmp.applyTicket("t2");
    cmp.branch.set("main");
    cmp.submit(true);
    const prompt = spawn.mock.calls[0][0].prompt as string;
    expect(prompt).toContain("Refactor auth module");
    expect(prompt).toContain("See RFC doc"); // HTML stripped
    expect(prompt).not.toContain("<strong>");
  });
});
