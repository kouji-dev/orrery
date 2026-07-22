import { Component, provideZonelessChangeDetection } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { PaneNodeComponent } from "./pane-node.component";
import { AgentActionsService } from "../agents/agent-actions.service";
import { DiagnosticsService } from "../shared/diagnostics.service";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { TerminalComponent } from "./terminal.component";
import { DiffViewComponent } from "./diff-view.component";
import { FileViewComponent } from "./file-view.component";
import { AgentGitViewComponent } from "./git/agent-git-view.component";
import { Agent } from "../models";
import { PaneCtx, PaneLeaf } from "./pane-model";

// Stubs — signal input.required components fail under JIT, and the real pane
// bodies (xterm, codemirror) are irrelevant to the header buttons under test.
@Component({ selector: "app-icon", template: "", inputs: ["name", "size", "px", "color"] })
class IconStub {}
@Component({ selector: "app-tool-badge", template: "", inputs: ["tool", "size"] })
class ToolBadgeStub {}
@Component({ selector: "app-terminal", template: "", inputs: ["agent"] })
class TerminalStub {}
@Component({ selector: "app-diff-view", template: "", inputs: ["agent"] })
class DiffViewStub {}
@Component({ selector: "app-file-view", template: "", inputs: ["agent", "path"] })
class FileViewStub {}
@Component({ selector: "app-agent-git-view", template: "", inputs: ["agent", "gitView"], outputs: ["close"] })
class AgentGitViewStub {}

function makeAgent(patch: Partial<Agent> = {}): Agent {
  return {
    id: "a1",
    projectId: "p1",
    tool: "claude",
    model: "opus",
    name: "refund-fix",
    task: "",
    status: "idle",
    branch: "agent/a1",
    worktree: "C:/wt/a1",
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

function makeCtx(agents: Agent[]): PaneCtx {
  return {
    agents: () => agents,
    projects: () => [],
    focusId: () => null,
    canClose: () => false,
    onSplit: vi.fn(),
    onClose: vi.fn(),
    onAgent: vi.fn(),
    onView: vi.fn(),
    onFileSelect: vi.fn(),
    onFileClose: vi.fn(),
    onRatio: vi.fn(),
    onFocus: vi.fn(),
    dropTarget: () => null,
    onDropOver: vi.fn(),
    onPaneDrop: vi.fn(),
  };
}

const LEAF: PaneLeaf = { type: "leaf", id: "pane1", agentId: "a1", view: "terminal" };

describe("PaneNodeComponent header actions", () => {
  const actions = { act: vi.fn(), toggleRun: vi.fn() };
  const diagnostics = { openWorktree: vi.fn(), openLog: vi.fn() };
  const ui = { gitViewFor: () => null, setGitView: vi.fn() };

  function render(agent: Agent) {
    const f = TestBed.createComponent(PaneNodeComponent);
    f.componentRef.setInput("node", LEAF);
    f.componentRef.setInput("ctx", makeCtx([agent]));
    f.detectChanges();
    return f;
  }
  const btn = (f: { nativeElement: HTMLElement }, title: string) =>
    f.nativeElement.querySelector<HTMLButtonElement>(`button[title^="${title}"]`);

  beforeEach(() => {
    vi.clearAllMocks();
    TestBed.configureTestingModule({
      imports: [PaneNodeComponent],
      providers: [
        provideZonelessChangeDetection(),
        { provide: AgentActionsService, useValue: actions },
        { provide: DiagnosticsService, useValue: diagnostics },
        { provide: UiStore, useValue: ui },
      ],
    });
    TestBed.overrideComponent(PaneNodeComponent, {
      remove: { imports: [IconComponent, ToolBadgeComponent, TerminalComponent, DiffViewComponent, FileViewComponent, AgentGitViewComponent] },
      add: { imports: [IconStub, ToolBadgeStub, TerminalStub, DiffViewStub, FileViewStub, AgentGitViewStub] },
    });
  });
  afterEach(() => TestBed.resetTestingModule());

  it("shows the reveal-in-explorer button before play/pause and opens the worktree", () => {
    const f = render(makeAgent());
    const reveal = btn(f, "Open worktree folder")!;
    expect(reveal).not.toBeNull();

    // sits in the header's action cluster, before the play/pause button
    const play = btn(f, "Resume agent")!;
    expect(reveal.compareDocumentPosition(play) & Node.DOCUMENT_POSITION_FOLLOWING).toBeTruthy();

    reveal.click();
    expect(diagnostics.openWorktree).toHaveBeenCalledWith("C:/wt/a1");
  });

  it("hides the reveal button when the agent has no worktree", () => {
    const f = render(makeAgent({ worktree: "" }));
    expect(btn(f, "Open worktree folder")).toBeNull();
  });

  it("shows Continue when a session is captured and resumes it (distinct from play)", () => {
    const f = render(makeAgent());
    const cont = btn(f, "Continue last session")!;
    expect(cont).not.toBeNull();
    expect(cont.title).toContain("sess-1");

    cont.click();
    expect(actions.act).toHaveBeenCalledWith("a1", "continueSession");
    expect(actions.toggleRun).not.toHaveBeenCalled();

    // play stays a fresh start/resume, not a session continue
    btn(f, "Resume agent")!.click();
    expect(actions.toggleRun).toHaveBeenCalled();
  });

  it("hides Continue while running or without a captured session", () => {
    const running = render(makeAgent({ status: "running" }));
    expect(btn(running, "Continue last session")).toBeNull();

    const noSession = render(makeAgent({ sessionId: undefined }));
    expect(btn(noSession, "Continue last session")).toBeNull();
  });
});
