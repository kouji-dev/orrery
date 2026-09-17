import { Component, provideZonelessChangeDetection, signal } from "@angular/core";
import { By } from "@angular/platform-browser";
import { SelectComponent, SelectGroup } from "../shared/select.component";
import { ComponentFixture, TestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { AGENT_TOOLS } from "../data";
import { AgentActionsService } from "../agents/agent-actions.service";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ModelCatalogService } from "../agents/model-catalog.service";
import { Settings, ToolDetection, Ticket } from "../models";
import { ProjectActionsService } from "../projects/project-actions.service";
import { settingsDefaults, SettingsStore } from "../settings/settings.store";
import { TicketsStore } from "../stores/tickets.store";
import { IconComponent } from "../shared/icon.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { UiStore } from "../ui/ui.store";
import { AgentToolControlsComponent } from "../shared/agent-tool-controls.component";
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

/** Bodies of every rule in the injected sheets whose selector matches — the
 *  component's own, after Angular's encapsulation attributes are stamped in.
 *  jsdom has no layout, so the two-up rows are asserted off their rule. */
function ruleBody(match: (selector: string) => boolean): string {
  const css = [...document.querySelectorAll("style")].map((s) => s.textContent ?? "").join("\n");
  return [...css.matchAll(/([^{}]+)\{([^{}]*)\}/g)]
    .filter((m) => match(m[1]))
    .map((m) => m[2])
    .join(" ");
}

/** The .spawn-split row holding `marker`. The dialog runs the SAME two-up
 *  recipe for Ticket | Name and Model | effort, so a row has to be named by
 *  what is in it — picking .spawn-split by position silently retargets these
 *  assertions the next time a pair is added above. */
function splitRow(fixture: ComponentFixture<SpawnModalComponent>, marker: string): HTMLElement {
  const rows = [...fixture.nativeElement.querySelectorAll(".spawn-split")] as HTMLElement[];
  return rows.find((r) => r.querySelector(marker))!;
}

interface Setup {
  cmp: SpawnModalComponent;
  fixture: ComponentFixture<SpawnModalComponent>;
  spawn: ReturnType<typeof vi.fn>;
  /** The model-catalog re-probe the modal fires on open (one per probe-backed tool). */
  refreshModels: ReturnType<typeof vi.fn>;
  /** The demand-driven CLI detection sweep the modal asks for on open. */
  ensureDetections: ReturnType<typeof vi.fn>;
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

/** A backend detection with only the fields a case cares about spelled out. */
function DET(p: Partial<ToolDetection> & { id: string; status: ToolDetection["status"] }): ToolDetection {
  return { available: p.status === "ok", path: null, version: null, source: null, reason: null, shim: false, ...p };
}

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
    project?: Partial<typeof PROJECT> & { defaultBranch?: string };
    /** What each model-enumerating CLI reported, per tool id
     *  (pi `--list-models`, `cursor-agent models`). */
    discovered?: Record<string, string[]>;
    /** Full backend detections per tool id — what the tiles read. */
    detections?: Record<string, ToolDetection>;
    /** Tools whose first detection probe hasn't answered yet. */
    pending?: (id: string) => boolean;
    /** The probe's own failure message per tool, when it blew up. */
    probeError?: Record<string, string>;
    /** false = probe still in flight (the picker's "asking…" copy). */
    probed?: boolean;
  } = {},
): Setup {
  const settings = signal<Settings>({ ...settingsDefaults(), ...opts.settings });
  const spawn = vi.fn();
  const refreshModels = vi.fn();
  const ensureDetections = vi.fn();
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      { provide: UiStore, useValue: makeUiStore({ spawnTicketId: opts.spawnTicketId }) },
      { provide: ProjectActionsService, useValue: { all: signal([{ ...PROJECT, ...opts.project }]) } },
      {
        provide: AgentRuntimeService,
        useValue: {
          toolAvailable: opts.available ?? (() => true),
          detection: (id: string) => opts.detections?.[id] ?? null,
          detectionPending: (id: string) => opts.pending?.(id) ?? false,
          ensureDetections,
        },
      },
      // The probe-backed model pickers (pi, cursor) read ModelCatalogService →
      // the Tauri bridge; stub it so these specs stay bridge-free.
      {
        provide: ModelCatalogService,
        useValue: {
          models: (tool: string) => opts.discovered?.[tool] ?? [],
          isProbed: () => opts.probed ?? true,
          error: (tool: string) => opts.probeError?.[tool] ?? null,
          load: () => {},
          refresh: refreshModels,
        },
      },
      { provide: AgentActionsService, useValue: { spawn } },
      { provide: SettingsStore, useValue: { settings } },
      { provide: TicketsStore, useValue: makeTicketsStore(opts.tickets ?? []) },
    ],
  });
  TestBed.overrideComponent(SpawnModalComponent, {
    remove: { imports: [IconComponent, ToolBadgeComponent] },
    add: { imports: [IconStub, ToolBadgeStub] },
  });
  // The tiles / model picker / effort pills moved into the shared control group
  // (app-agent-tool-controls), which this dialog renders — so its own brand
  // badge needs the same stub, or every tile throws NG0950 under JIT.
  TestBed.overrideComponent(AgentToolControlsComponent, {
    remove: { imports: [ToolBadgeComponent] },
    add: { imports: [ToolBadgeStub] },
  });
  const fixture = TestBed.createComponent(SpawnModalComponent);
  fixture.detectChanges();
  return { cmp: fixture.componentInstance, fixture, spawn, refreshModels, ensureDetections };
}

describe("SpawnModal — settings prefill", () => {
  it("no saved defaults: keeps the hardcoded claude / first model / that model's effort", () => {
    const { cmp } = setup();
    expect(cmp.toolId()).toBe("claude");
    expect(cmp.model()).toBe("fable");
    expect(cmp.effort()).toBe("xhigh"); // the fable alias' declared default = Claude Code's own
  });

  it("defaultTool prefills the initial tool (with its model + effort defaults)", () => {
    const { cmp } = setup({ settings: { defaultTool: "codex" } });
    expect(cmp.toolId()).toBe("codex");
    expect(cmp.model()).toBe("gpt-5.6-sol"); // curated first model
    expect(cmp.effort()).toBe("xhigh"); // Sol's declared default
  });

  it("picking a model re-validates the effort against what THAT model accepts", () => {
    const { cmp } = setup();
    cmp.setModel("haiku"); // no effort knob at all
    expect(cmp.effort()).toBeNull();
    cmp.setModel("claude-opus-4-6"); // nothing to keep → the model's default
    expect(cmp.effort()).toBe("high");
    cmp.effort.set("max");
    cmp.setModel("claude-opus-5"); // max is offered → kept
    expect(cmp.effort()).toBe("max");
    cmp.effort.set("xhigh");
    cmp.setModel("claude-sonnet-4-6"); // no xhigh on the 4.6 generation → default
    expect(cmp.effort()).toBe("high");
    cmp.setModel("my-custom-id"); // a custom id gets the TOOL's full list
    expect(cmp.effort()).toBe("high");
  });

  it("per-tool toolModel/toolEffort overrides win over the curated defaults", () => {
    const { cmp } = setup({
      settings: {
        defaultTool: "codex",
        toolModel: { codex: "gpt-5.6-luna" },
        toolEffort: { codex: "low" },
      },
    });
    expect(cmp.model()).toBe("gpt-5.6-luna");
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
      settings: { toolModel: { codex: "gpt-5.6-luna", claude: "haiku" }, toolEffort: { codex: "medium" } },
    });
    expect(cmp.model()).toBe("haiku"); // claude override applies on open
    cmp.setTool("codex");
    expect(cmp.model()).toBe("gpt-5.6-luna");
    expect(cmp.effort()).toBe("medium");
    cmp.setTool("cursor"); // no override → curated default, effort unsupported
    expect(cmp.model()).toBe("composer-2.5");
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
    expect(cmp.model()).toBe("gpt-5.6-sol"); // first curated
    expect(cmp.effort()).toBe("xhigh"); // Sol's own default, not the stale "ultra"
  });

  it("a saved effort the saved MODEL does not accept falls back to that model's default", () => {
    const { cmp } = setup({
      settings: { toolModel: { claude: "claude-opus-4-6" }, toolEffort: { claude: "xhigh" } },
    });
    expect(cmp.model()).toBe("claude-opus-4-6");
    expect(cmp.effort()).toBe("high"); // 4.6 has no xhigh
  });
});

describe("SpawnModal — pi's discovered model catalog", () => {
  const DISCOVERED = ["anthropic/claude-opus-4-5", "openai/gpt-5.1"];

  it("feeds the picker from the CLI probe, not a hardcoded list", async () => {
    const { cmp, fixture } = setup({ discovered: { pi: DISCOVERED } });
    cmp.setTool("pi");
    await fixture.whenStable();
    expect(cmp.discoveredModels()).toEqual(DISCOVERED);
    // the dynamic tool swaps the app-select for the free-text kouji combobox
    expect(fixture.nativeElement.querySelector("kj-combobox")).toBeTruthy();
  });

  it("prefills the first discovered model when no override is stored", () => {
    const { cmp } = setup({ discovered: { pi: DISCOVERED } });
    cmp.setTool("pi");
    expect(cmp.model()).toBe("anthropic/claude-opus-4-5");
  });

  it("keeps a stored BYOK id even though it is in no curated catalog", () => {
    const { cmp } = setup({
      settings: { toolModel: { pi: "groq/kimi-k2.5" } },
      discovered: { pi: DISCOVERED },
    });
    cmp.setTool("pi");
    expect(cmp.model()).toBe("groq/kimi-k2.5");
    // …and it still gets pi's own --thinking levels (nothing narrows them)
    expect(cmp.effortLevels()).toEqual(["off", "minimal", "low", "medium", "high", "xhigh", "max"]);
  });

  it("falls back to the CLI's own default (empty model) when nothing is discovered", () => {
    const { cmp } = setup({ discovered: { pi: [] } });
    cmp.setTool("pi");
    expect(cmp.model()).toBe("");
  });
});

describe("SpawnModal — why the dynamic picker is empty", () => {
  it("probe still in flight: says it is asking, not that there is nothing", () => {
    const { cmp } = setup({ probed: false, discovered: { pi: [] } });
    cmp.setTool("pi");
    expect(cmp.discoveryHint()).toContain("Asking");
  });

  it("pi ran and reported nothing: names the sign-in, in pi's own wording", () => {
    // VERIFIED: `pi --list-models` EXITS 0 and prints "No models available. Use
    // /login to log into a provider via OAuth or API key." when signed out —
    // so an empty list here is an auth gap, never a broken tool.
    const { cmp } = setup({ discovered: { pi: [] } });
    cmp.setTool("pi");
    const hint = cmp.discoveryHint();
    expect(hint).toContain("/login");
    expect(hint).toContain("no provider is signed in");
    expect(hint).not.toContain("isn’t installed");
  });

  it("the probe itself failed: surfaces the captured error, not silence", () => {
    const { cmp } = setup({
      probeError: { pi: "list_tool_models timed out after 20s" },
      detections: { pi: DET({ id: "pi", status: "error", reason: "timed out" }) },
    });
    cmp.setTool("pi");
    expect(cmp.discoveryHint()).toContain("list_tool_models timed out after 20s");
  });

  it("the CLI is not installed: an install gap, not a malfunction", () => {
    const { cmp } = setup({
      probeError: { pi: "program not found" },
      detections: { pi: DET({ id: "pi", status: "missing" }) },
    });
    cmp.setTool("pi");
    expect(cmp.discoveryHint()).toContain("isn’t installed");
    expect(cmp.discoveryHint()).not.toContain("program not found"); // raw spew reads as a bug
  });
});

describe("SpawnModal — tool tile detection states", () => {
  const tiles = (f: ComponentFixture<unknown>) =>
    Array.from((f.nativeElement as HTMLElement).querySelectorAll<HTMLElement>(".tool-tile"));

  it("opening the dialog DEMANDS the sweep — it no longer runs at boot", () => {
    const { ensureDetections } = setup();
    expect(ensureDetections).toHaveBeenCalledTimes(1);
  });

  it("an unprobed tool spins instead of being declared missing", () => {
    const { fixture } = setup({ pending: () => true, available: () => false });
    const all = tiles(fixture);
    expect(all).toHaveLength(AGENT_TOOLS.length);
    expect(all.every((t) => t.querySelector("kj-spinner") !== null)).toBe(true);
    expect(fixture.nativeElement.textContent).toContain("checking");
    expect(fixture.nativeElement.textContent).not.toContain("not found");
  });

  it("probed and absent still says not found — the verdict is allowed once it exists", () => {
    const { fixture } = setup({ available: (id) => id !== "gemini" });
    const gemini = tiles(fixture).find((t) => t.textContent?.includes("Gemini"))!;
    expect(gemini.textContent).toContain("not found");
    expect(gemini.querySelector("kj-spinner")).toBeNull();
  });

  it("probed but unrunnable reads 'can’t run' and carries the reason on hover", () => {
    const { fixture } = setup({
      available: (id) => id !== "gemini",
      detections: {
        gemini: DET({ id: "gemini", status: "error", reason: "couldn’t launch — os error 5" }),
      },
    });
    const gemini = tiles(fixture).find((t) => t.textContent?.includes("Gemini"))!;
    expect(gemini.textContent).toContain("can’t run");
    expect(gemini.querySelector("[title]")?.getAttribute("title")).toContain("os error 5");
  });
});

describe("SpawnModal — cursor's probed pool vs. its curated fallback", () => {
  it("the probe wins when the account reported a pool", () => {
    const { cmp } = setup({ discovered: { cursor: ["composer-3", "grok-5"] } });
    cmp.setTool("cursor");
    expect(cmp.modelChoices().map((m) => m.id)).toEqual(["composer-3", "grok-5"]);
    expect(cmp.model()).toBe("composer-3");
  });

  it("an EMPTY probe (signed out / no models) falls back to the curated list", () => {
    // `cursor-agent models` prints "No models available for this account." and
    // exits 0 — the picker must not go empty.
    const { cmp } = setup({ discovered: { cursor: [] } });
    cmp.setTool("cursor");
    expect(cmp.modelChoices().length).toBeGreaterThan(0);
    expect(cmp.model()).toBe("composer-2.5"); // the curated default
  });

  it("re-probes every model-enumerating tool on open, and only those", () => {
    const { refreshModels } = setup();
    expect(refreshModels.mock.calls.map((c) => c[0]).sort()).toEqual(["cursor", "pi"]);
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

  /* The Ticket field is an <app-select> now, not a native <select>: the To do /
     In progress split that <optgroup> carried rides in as option GROUPS. Under
     raw vitest JIT the control's signal inputs are inert, so what is asserted
     here is the model it is fed; that the trigger then paints the ticket TITLE
     rather than its id is asserted in e2e (spawn-dialog.spec.ts). */
  it("Dispatch path: the Ticket options carry the dispatched ticket under its status group", () => {
    const { cmp } = setup({ tickets: [TICKET_TODO, TICKET_INPROG], spawnTicketId: "t1" });
    const opts = cmp.ticketOptions();
    // the "None" escape hatch stays loose at the top, before any group. Its
    // value is a sentinel, not "" — kouji paints its placeholder over an empty
    // value, which would have printed "Select…" where the row's label belongs.
    expect(opts[0]).toEqual({ value: "__none__", label: "None — start from scratch" });
    const groups = opts.filter((o) => "options" in o) as SelectGroup[];
    expect(groups.map((g) => g.label)).toEqual(["To do", "In progress"]);
    // the dispatched id resolves to a LABEL — the half that printed a raw id
    expect(groups[0].options).toContainEqual({ value: "t1", label: "Fix the login bug" });
    expect(cmp.ticketId()).toBe("t1");
  });

  it("contributes no group for a status with no open tickets", () => {
    const { cmp } = setup({ tickets: [TICKET_TODO] });
    const groups = cmp.ticketOptions().filter((o) => "options" in o) as SelectGroup[];
    expect(groups.map((g) => g.label)).toEqual(["To do"]);
  });

  it("openTicketsTodo lists only todo tickets", () => {
    const { cmp } = setup({ tickets: [TICKET_TODO, TICKET_INPROG] });
    expect(cmp.openTicketsTodo().map((t) => t.id)).toEqual(["t1"]);
  });

  it("openTicketsInProgress lists only inprogress tickets", () => {
    const { cmp } = setup({ tickets: [TICKET_TODO, TICKET_INPROG] });
    expect(cmp.openTicketsInProgress().map((t) => t.id)).toEqual(["t2"]);
  });

  it("switching to a ticket's project re-derives the branch default", () => {
    const { cmp } = setup({
      tickets: [TICKET_TODO],
      project: { branches: ["aaa", "main", "dev"], defaultBranch: "main" },
    });
    cmp.branch.set("dev"); // user had picked something else
    cmp.applyTicket("t1"); // t1 targets p1 → reset to that project's default
    expect(cmp.branch()).toBe("main");
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

describe("SpawnModal — source branch", () => {
  /** The branch picker is an <app-select> (a kouji listbox, not a native
   *  <select>). Two constraints shape this helper:
   *   - signal INPUTS are not wired under raw vitest JIT, so the control cannot
   *     be identified by its options, nor can its trigger label be asserted —
   *     that half of the old test now belongs in e2e, where the real control runs.
   *   - its OUTPUT works fine, so emitting valueChange still exercises the real
   *     binding path: control -> (valueChange) -> branch.set -> submit payload.
   *  Identified by template order (project, branch, ticket, model), asserted
   *  below so a template reshuffle fails loudly instead of silently testing the
   *  wrong one. */
  function branchPicker(fixture: ComponentFixture<SpawnModalComponent>) {
    const selects = fixture.debugElement.queryAll(By.directive(SelectComponent));
    expect(
      selects,
      "spawn modal renders project + branch + ticket + model pickers",
    ).toHaveLength(4);
    return selects[1].componentInstance as SelectComponent;
  }

  it("defaults to the repo's defaultBranch, not the first branch in the list", () => {
    const { cmp } = setup({
      project: { branches: ["agent/aaa", "dev", "main"], defaultBranch: "main" },
    });
    expect(cmp.branch()).toBe("main");
  });

  it("falls back to the first branch when no defaultBranch resolves", () => {
    const { cmp } = setup({ project: { branches: ["dev", "trunk"], defaultBranch: undefined } });
    expect(cmp.branch()).toBe("dev");
  });

  it("binds the branch picker to the resolved default branch", () => {
    const branches = ["agent/aaa", "dev", "main"];
    const { fixture, cmp } = setup({ project: { branches, defaultBranch: "main" } });
    expect(branchPicker(fixture)).toBeTruthy();
    expect(cmp.branch()).toBe("main");
  });

  it("a user's selection via the rendered picker survives to submit (regression)", () => {
    const branches = ["agent/aaa", "dev", "main"];
    const { fixture, cmp, spawn } = setup({ project: { branches, defaultBranch: "main" } });
    expect(branchPicker(fixture)).toBeTruthy(); // the picker is rendered and bound
    // NOTE: the DOM-driven half of this regression now lives in e2e
    // (spawn-dialog.spec.ts). Under raw vitest JIT the modal's bindings to
    // <app-select> are inert — neither its inputs nor its outputs are wired —
    // so a click on the real control cannot be simulated here. What IS still
    // worth guarding at this level is the second half: a non-default branch
    // must survive into the spawn payload rather than being re-defaulted.
    cmp.branch.set("dev");
    fixture.detectChanges();
    expect(cmp.branch()).toBe("dev");
    cmp.name.set("agent-x");
    cmp.submit(true);
    expect(spawn).toHaveBeenCalledWith(expect.objectContaining({ branch: "dev" }));
  });

  it("switching project resets the branch to THAT project's default", () => {
    const { cmp } = setup({
      project: { branches: ["agent/aaa", "dev", "main"], defaultBranch: "main" },
    });
    cmp.branch.set("dev");
    cmp.setProject("p1"); // re-selecting resolves the (single) test project again
    expect(cmp.branch()).toBe("main");
  });
});

describe("SpawnModal — agent picker row", () => {
  /** The component's own emulated-encapsulation sheet, as Angular injected it. */
  function spawnToolsRule(): string {
    const css = [...document.querySelectorAll("style")].map((s) => s.textContent ?? "").join("\n");
    return /\.spawn-tools[^{}]*\{([^}]*)\}/.exec(css)?.[1] ?? "";
  }

  it("renders every tool as one tile in a single horizontally scrolling row", () => {
    const { fixture } = setup();
    const row = fixture.nativeElement.querySelector(".spawn-tools") as HTMLElement;
    expect(row).toBeTruthy();
    expect(row.querySelectorAll(".tool-tile").length).toBe(AGENT_TOOLS.length);
    // jsdom has no layout, so the guarantee is read off the rule itself: the
    // row overflows sideways instead of dividing the fixed 540px dialog by an
    // ever-growing N, and the tracks keep a legible floor while it does.
    const rule = spawnToolsRule();
    expect(rule).toMatch(/overflow-x:\s*auto/);
    expect(rule).toMatch(/grid-auto-columns:\s*minmax\(var\(--tile-floor/);
    expect(rule).not.toMatch(/minmax\(\s*0/); // a zero floor is what let tiles squeeze
    // the app scrollbar must stay visible — it is the only cue that more
    // agents exist off-screen
    expect(row.classList.contains("scroll-hide")).toBe(false);
  });
});

describe("SpawnModal — agent tile order", () => {
  /** The tile labels in the order the row actually renders them. */
  function tileOrder(fixture: ComponentFixture<SpawnModalComponent>): string[] {
    return [...fixture.nativeElement.querySelectorAll(".tool-tile .tn")].map(
      (el) => (el as HTMLElement).textContent?.trim() ?? "",
    );
  }
  const ALL = AGENT_TOOLS.map((t) => t.id); // claude, codex, cursor, gemini, pi

  it("puts the runnable agents first once the detection sweep has settled", () => {
    const { cmp, fixture } = setup({ available: (id) => id === "gemini" || id === "pi" });
    expect(cmp.tools().map((t) => t.id)).toEqual(["gemini", "pi", "claude", "codex", "cursor"]);
    expect(tileOrder(fixture)).toEqual(["Gemini", "Pi", "Claude Code", "Codex", "Cursor"]);
  });

  it("keeps the declaration order INSIDE each group — the partition is stable", () => {
    const { cmp } = setup({ available: (id) => id !== "codex" && id !== "gemini" });
    // claude/cursor/pi are declared in that order and stay in it; so do the two
    // demoted ones. Nothing here depends on a comparator's tie-breaking.
    expect(cmp.tools().map((t) => t.id)).toEqual(["claude", "cursor", "pi", "codex", "gemini"]);
  });

  it("does not promote a tool whose probe errored, even if it reads as available", () => {
    const { cmp } = setup({
      available: () => true,
      detections: { claude: DET({ id: "claude", status: "error", reason: "exec format error" }) },
    });
    // the tile says "can’t run" — leading with it would offer the one agent
    // that cannot spawn
    expect(cmp.tools().map((t) => t.id)).toEqual(["codex", "cursor", "gemini", "pi", "claude"]);
  });

  it("holds the declaration order while ANY probe is still out", () => {
    // pi is the only runnable one, but codex has no verdict yet: reordering now
    // would move tiles under the cursor once per probe as the sweep lands.
    const { cmp, fixture } = setup({ available: (id) => id === "pi", pending: (id) => id === "codex" });
    expect(cmp.tools().map((t) => t.id)).toEqual(ALL);
    expect(tileOrder(fixture)).toEqual(AGENT_TOOLS.map((t) => t.name));
  });

  it("never drops a tool, and never moves the selection", () => {
    const { cmp } = setup({ settings: { defaultTool: "gemini" }, available: (id) => id !== "claude" });
    expect(cmp.toolId()).toBe("gemini"); // initialTool() untouched by the order
    cmp.setTool("claude"); // a demoted tile is still selectable
    expect(cmp.toolId()).toBe("claude");
    expect(cmp.tools().map((t) => t.id)).toEqual(["codex", "cursor", "gemini", "pi", "claude"]);
    expect([...cmp.tools()].sort()).toHaveLength(ALL.length);
  });
});

describe("SpawnModal — ticket + name row", () => {
  /** The pair, found by the Name field's input group. */
  const row = (fixture: ComponentFixture<SpawnModalComponent>) => splitRow(fixture, ".spawn-name");

  it("pairs Ticket and Name on one row, as two columns of the SAME wrapper", () => {
    const { fixture } = setup({ tickets: [TICKET_TODO] });
    const cols = [...row(fixture).children] as HTMLElement[];
    // Both halves are kj-field. The Ticket half was a bare <div> + .field-label,
    // a wrapper one type step and one gap away from the kj-field beside it — so
    // paired, the two labels sat off each other's baseline and the two controls
    // started at different heights. One wrapper is what makes them agree.
    expect(cols.map((c) => c.tagName.toLowerCase())).toEqual(["kj-field", "kj-field"]);
    expect(cols.every((c) => c.classList.contains("spawn-field"))).toBe(true);
    expect(row(fixture).querySelector(".field-label")).toBeNull();
    expect(cols[0].querySelectorAll("app-select").length).toBe(1); // Ticket's picker
    expect(cols[1].querySelector(".spawn-name")).toBeTruthy(); // Name's input group
  });

  it("floors both columns and lets the pair wrap instead of squeezing either", () => {
    setup();
    // jsdom has no layout: the guarantee is read off the shared two-up rule.
    // min-width:0 is the load-bearing part here — a flex item defaults to
    // min-width:auto, so a long ticket title or worktree name would widen its
    // column past the card rather than being clipped.
    const split = ruleBody((s) => s.includes(".spawn-split"));
    expect(split).toMatch(/display:\s*flex/);
    expect(split).toMatch(/flex-wrap:\s*wrap/);
    expect(split).toMatch(/min-width:\s*0/);
    expect(split).toMatch(/flex:\s*1\s+1\s+22ch/);
  });

  it("keeps the ticket link driving the Name field from inside the row", () => {
    const { cmp, fixture } = setup({ tickets: [TICKET_TODO] });
    expect(row(fixture).querySelector(".spawn-linked")).toBeNull(); // idle: no line at all
    cmp.applyTicket("t1");
    fixture.detectChanges();
    expect(cmp.name()).toBe("fix-the-login-bug"); // the prefill still crosses the pair
    const linked = row(fixture).querySelector(".spawn-linked");
    expect(linked).toBeTruthy();
    expect(linked?.textContent).toContain("Name linked");
  });
});

describe("SpawnModal — model + effort row", () => {
  it("lets the effort tray wrap to its own full-width row instead of starving Model", () => {
    // pi is the worst pairing: SEVEN effort levels against the longest model ids
    const { cmp, fixture } = setup({
      settings: { defaultTool: "pi" },
      discovered: { pi: ["anthropic/claude-sonnet-4-5-20250929"] },
    });
    expect(cmp.effortLevels()).toHaveLength(7);
    const row = splitRow(fixture, ".spawn-effort");
    expect(row).toBeTruthy();
    expect(row.querySelectorAll(".spawn-field").length).toBe(2); // Model + effort
    expect(row.querySelector(".spawn-effort")).toBeTruthy();
    // jsdom has no layout, so the guarantee is read off the rule itself. The
    // row WRAPS: a tray wider than the space left drops to a line of its own
    // (where it grows to the full width) instead of pushing the Model field
    // below its own basis. min-width:0 is what stops that push — a flex item's
    // min-width defaults to auto = min-content, and the nowrap kj pill tray's
    // min-content is the sum of every pill.
    const split = ruleBody((s) => s.includes(".spawn-split"));
    expect(split).toMatch(/display:\s*flex/);
    expect(split).toMatch(/flex-wrap:\s*wrap/);
    expect(split).toMatch(/min-width:\s*0/);
    // both bases are content-sized, never a hard half — that is what makes the
    // break depend on the level count without any TS branching on it
    expect(split).toMatch(/flex:\s*1\s+1\s+22ch/);
    expect(split).toMatch(/flex-basis:\s*auto/);
  });

  it("wraps the effort pills rather than shrinking or scrolling them", () => {
    setup({ settings: { defaultTool: "pi" } });
    const seg = ruleBody((s) => s.includes(".spawn-seg"));
    expect(seg).toMatch(/flex-wrap:\s*wrap/);
    // flex:1 is a 0 basis, which never overflows — the line would never break
    // and the pills would silently clip to stubs again
    expect(seg).toMatch(/flex:\s*1\s+1\s+auto/);
    // a scroller would hide levels behind a gesture and can park the SELECTED
    // pill off-screen, leaving the field showing no answer at all
    expect(seg).not.toMatch(/overflow-x/);
  });

  it("keeps the dialog on the app's density-scaled width convention", () => {
    setup();
    // widened 540 -> 600 so claude's five and codex's four levels still share
    // the row with Model; kouji has no dialog-width knob, the class is it
    expect(ruleBody((s) => s.includes(".kj-dialog"))).toMatch(
      /width:\s*round\(calc\(600px \* var\(--density\)\), 1px\)/,
    );
  });
});
