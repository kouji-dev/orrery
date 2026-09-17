import { Component, provideZonelessChangeDetection, signal } from "@angular/core";
import { ComponentFixture, TestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { AGENT_TOOLS } from "../data";
import { Agent, ToolDetection } from "../models";
import { AgentActionsService } from "../agents/agent-actions.service";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { ModelCatalogService } from "../agents/model-catalog.service";
import { IconComponent } from "../shared/icon.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { AgentToolControlsComponent } from "../shared/agent-tool-controls.component";
import { UiStore } from "../ui/ui.store";
import { EditAgentModalComponent } from "./edit-agent-modal.component";

beforeAll(() => {
  try {
    TestBed.initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by another spec in this worker
  }
});

afterEach(() => TestBed.resetTestingModule());

// signal-input shared components can't be JIT-compiled by raw vitest (NG0950) —
// same-selector stubs keep the dialog's own template fully exercised.
@Component({ selector: "app-icon", template: "", inputs: ["name", "size", "px", "color"] })
class IconStub {}
@Component({ selector: "app-tool-badge", template: "", inputs: ["tool", "size"] })
class ToolBadgeStub {}

function makeAgent(patch: Partial<Agent> = {}): Agent {
  return {
    id: "a1",
    projectId: "p1",
    tool: "claude",
    model: "claude-opus-4-6",
    effort: "high",
    name: "refund-fix",
    task: "",
    status: "idle",
    branch: "agent/refund-fix",
    worktree: "C:/wt/refund-fix",
    base: "main",
    started: true,
    sessionId: "sess-1",
    commits: 0,
    elapsed: 0,
    progress: 0,
    pending: [],
    ...patch,
  };
}

interface Setup {
  cmp: EditAgentModalComponent;
  fixture: ComponentFixture<EditAgentModalComponent>;
  applyAgentEdit: ReturnType<typeof vi.fn>;
  closeEditAgent: ReturnType<typeof vi.fn>;
  agents: ReturnType<typeof signal<Agent[]>>;
  editingAgent: ReturnType<typeof signal<string | null>>;
}

function setup(opts: { agent?: Partial<Agent>; discovered?: Record<string, string[]> } = {}): Setup {
  const ag = makeAgent(opts.agent);
  const agents = signal<Agent[]>([ag]);
  const editingAgent = signal<string | null>(ag.id);
  const applyAgentEdit = vi.fn(() => Promise.resolve());
  const closeEditAgent = vi.fn(() => editingAgent.set(null));
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      {
        provide: UiStore,
        useValue: { editingAgent, openEditAgent: vi.fn(), closeEditAgent, flash: vi.fn() },
      },
      {
        provide: AgentRuntimeService,
        useValue: {
          agents,
          toolAvailable: () => true,
          detection: (): ToolDetection | null => null,
          detectionPending: () => false,
          ensureDetections: vi.fn(),
        },
      },
      {
        provide: ModelCatalogService,
        useValue: {
          models: (tool: string) => opts.discovered?.[tool] ?? [],
          isProbed: () => true,
          error: () => null,
          load: vi.fn(),
          refresh: vi.fn(),
        },
      },
      { provide: AgentActionsService, useValue: { applyAgentEdit } },
    ],
  });
  TestBed.overrideComponent(EditAgentModalComponent, {
    remove: { imports: [IconComponent] },
    add: { imports: [IconStub] },
  });
  TestBed.overrideComponent(AgentToolControlsComponent, {
    remove: { imports: [ToolBadgeComponent] },
    add: { imports: [ToolBadgeStub] },
  });
  const fixture = TestBed.createComponent(EditAgentModalComponent);
  fixture.detectChanges();
  return { cmp: fixture.componentInstance, fixture, applyAgentEdit, closeEditAgent, agents, editingAgent };
}

describe("EditAgentModal — prefill", () => {
  it("seeds the three controls from the RECORD, not from the settings defaults", () => {
    const { cmp } = setup();
    expect(cmp.toolId()).toBe("claude");
    expect(cmp.model()).toBe("claude-opus-4-6");
    expect(cmp.effort()).toBe("high");
    // an untouched agent has nothing to send, so Save is inert
    expect(cmp.dirty()).toBe(false);
    expect(cmp.toolChanged()).toBe(false);
  });

  it("keeps a BYOK model id that is in no curated catalog", () => {
    const { cmp } = setup({
      agent: { tool: "pi", model: "groq/kimi-k2.5", effort: "medium" },
      discovered: { pi: ["anthropic/claude-opus-4-5"] },
    });
    expect(cmp.model()).toBe("groq/kimi-k2.5");
    expect(cmp.effort()).toBe("medium");
    expect(cmp.dirty()).toBe(false);
  });

  it("drops a stored effort the stored MODEL no longer accepts", () => {
    // Opus 4.6 predates `xhigh`; showing it selected would report a level the
    // CLI would reject on the next launch
    const { cmp } = setup({ agent: { model: "claude-opus-4-6", effort: "xhigh" } });
    expect(cmp.effort()).toBe("high"); // that model's own default
  });

  it("clears the effort entirely for a model that has no knob", () => {
    const { cmp } = setup({ agent: { model: "haiku", effort: "high" } });
    expect(cmp.effort()).toBeNull();
  });

  it("renders the SAME shared control group the spawn dialog uses", () => {
    const { fixture } = setup();
    expect(fixture.nativeElement.querySelector("app-agent-tool-controls")).toBeTruthy();
    expect(fixture.nativeElement.querySelectorAll(".tool-tile").length).toBe(AGENT_TOOLS.length);
  });

  it("re-prefills when the dialog is re-pointed at ANOTHER agent without being destroyed", () => {
    const { cmp, fixture, agents, editingAgent } = setup();
    cmp.setTool("codex");
    expect(cmp.dirty()).toBe(true);
    agents.set([makeAgent(), makeAgent({ id: "a2", name: "other", model: "sonnet", effort: "high" })]);
    editingAgent.set("a2");
    fixture.detectChanges();
    expect(cmp.model()).toBe("sonnet");
    expect(cmp.dirty()).toBe(false); // the previous agent's draft did not carry over
  });
});

describe("EditAgentModal — switching tool", () => {
  it("takes the new tool's defaults: a model id means nothing to another CLI", () => {
    const { cmp } = setup();
    cmp.setTool("codex");
    expect(cmp.model()).toBe("gpt-5.6-sol");
    expect(cmp.effort()).toBe("xhigh");
    expect(cmp.toolChanged()).toBe(true);
  });

  it("switching BACK restores what the agent actually has", () => {
    const { cmp } = setup();
    cmp.setTool("codex");
    cmp.setTool("claude");
    expect(cmp.model()).toBe("claude-opus-4-6");
    expect(cmp.effort()).toBe("high");
    expect(cmp.dirty()).toBe(false); // a stray click through the tiles saves nothing
  });

  it("picking a model re-validates the effort against what THAT model accepts", () => {
    const { cmp } = setup();
    cmp.setModel("haiku");
    expect(cmp.effort()).toBeNull();
    cmp.setModel("claude-opus-5");
    expect(cmp.effort()).toBe("xhigh");
  });
});

describe("EditAgentModal — save", () => {
  it("a model-only edit skips the confirm entirely, even while the agent runs", () => {
    const { cmp, applyAgentEdit } = setup({ agent: { status: "running" } });
    cmp.setModel("claude-opus-5");
    cmp.save();
    expect(cmp.confirming()).toBe(false);
    // Opus 5 still offers the level the agent was on, so the pick is kept
    // rather than reset to that model's own default
    expect(applyAgentEdit).toHaveBeenCalledWith(
      "a1",
      { tool: "claude", model: "claude-opus-5", effort: "high" },
      false,
    );
  });

  it("a tool change on an IDLE agent just updates — nothing is running to lose", () => {
    const { cmp, applyAgentEdit } = setup({ agent: { status: "idle" } });
    cmp.setTool("codex");
    cmp.save();
    expect(cmp.confirming()).toBe(false);
    expect(applyAgentEdit).toHaveBeenCalledWith(
      "a1",
      { tool: "codex", model: "gpt-5.6-sol", effort: "xhigh" },
      false,
    );
  });

  it("a tool change on a RUNNING agent confirms FIRST and applies nothing yet", () => {
    const { cmp, fixture, applyAgentEdit } = setup({ agent: { status: "running" } });
    cmp.setTool("codex");
    cmp.save();
    expect(cmp.confirming()).toBe(true);
    expect(applyAgentEdit).not.toHaveBeenCalled();
    fixture.detectChanges();
    // the copy has to name the blast radius: the live agent is closed and
    // restarted on the new provider
    const warn = fixture.nativeElement.querySelector(".ea-warn") as HTMLElement;
    expect(warn).toBeTruthy();
    expect(warn.textContent).toContain("Codex");
    expect(warn.textContent).toContain("closes the");
    expect(warn.textContent).toContain("running");
  });

  it("confirming the restart sends one update and asks for the restart", () => {
    const { cmp, applyAgentEdit } = setup({ agent: { status: "running" } });
    cmp.setTool("codex");
    cmp.save(); // arms
    cmp.save(); // confirms
    expect(applyAgentEdit).toHaveBeenCalledTimes(1);
    expect(applyAgentEdit).toHaveBeenCalledWith(
      "a1",
      { tool: "codex", model: "gpt-5.6-sol", effort: "xhigh" },
      true,
    );
  });

  it("cancelling the confirm is a TRUE no-op — no update, and the draft is still there", () => {
    const { cmp, applyAgentEdit, closeEditAgent } = setup({ agent: { status: "running" } });
    cmp.setTool("codex");
    cmp.save();
    cmp.confirming.set(false); // the confirm step's Cancel button
    expect(applyAgentEdit).not.toHaveBeenCalled();
    expect(closeEditAgent).not.toHaveBeenCalled(); // back into the form, not out
    expect(cmp.toolId()).toBe("codex");
  });

  it("sends effort as null rather than omitting it when the new tool has no knob", () => {
    const { cmp, applyAgentEdit } = setup();
    cmp.setTool("cursor"); // no effort levels at all
    expect(cmp.effort()).toBeNull();
    cmp.save();
    const patch = applyAgentEdit.mock.calls[0][1] as Record<string, unknown>;
    // an OMITTED key means "leave alone" backend-side, so the claude level
    // would have survived onto a tool that cannot accept it
    expect("effort" in patch).toBe(true);
    expect(patch.effort).toBeNull();
  });

  it("an untouched dialog sends nothing at all", () => {
    const { cmp, applyAgentEdit } = setup();
    cmp.save();
    expect(applyAgentEdit).not.toHaveBeenCalled();
  });

  it("closes itself when the agent it is editing disappears", () => {
    const { fixture, agents, closeEditAgent } = setup();
    agents.set([]);
    fixture.detectChanges();
    expect(closeEditAgent).toHaveBeenCalled();
  });
});
