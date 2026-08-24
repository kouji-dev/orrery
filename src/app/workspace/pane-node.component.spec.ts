import { Component, provideZonelessChangeDetection } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import { PaneNodeComponent } from "./pane-node.component";
import { AgentActionsService } from "../agents/agent-actions.service";
import { BRIDGE } from "../data-source/bridge";
import { DiagnosticsService } from "../shared/diagnostics.service";
import { EditsStore } from "../stores/edits.store";
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
  // openMenu/contextMenu mirror the real UiStore contract: the tab context
  // menu posts MenuItems into the store; the shared app-context-menu renders it.
  let menuState: { x: number; y: number; items: import("../models").MenuItem[] } | null = null;
  const ui = {
    gitViewFor: () => null,
    setGitView: vi.fn(),
    paneRoots: () => ({}),
    flash: vi.fn(),
    openMenu: (e: MouseEvent, items: import("../models").MenuItem[]) => {
      e.preventDefault();
      menuState = { x: 0, y: 0, items };
    },
    contextMenu: () => menuState,
    closeMenu: () => (menuState = null),
  };
  const bridge = { invoke: vi.fn().mockResolvedValue(undefined) };

  function render(agent: Agent) {
    const f = TestBed.createComponent(PaneNodeComponent);
    f.componentRef.setInput("node", LEAF);
    f.componentRef.setInput("ctx", makeCtx([agent]));
    f.detectChanges();
    return f;
  }
  // <kj-button> carries the title on its HOST (its inner <button> is kouji's).
  // Native tooltips still resolve — the browser walks up for `title` — but a
  // `button[title]` selector no longer matches, so accept either shape and
  // return the element that owns the title and the (click) listener.
  const btn = (f: { nativeElement: HTMLElement }, title: string) =>
    f.nativeElement.querySelector<HTMLElement>(
      `kj-button[title^="${title}"], button[title^="${title}"]`,
    );

  beforeEach(() => {
    vi.clearAllMocks();
    menuState = null;
    TestBed.configureTestingModule({
      imports: [PaneNodeComponent],
      providers: [
        provideZonelessChangeDetection(),
        { provide: AgentActionsService, useValue: actions },
        { provide: DiagnosticsService, useValue: diagnostics },
        { provide: UiStore, useValue: ui },
        { provide: BRIDGE, useValue: bridge },
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

  // ----- dirty file tabs: indicator + guarded close (B1.1) -----

  const FILE_LEAF: PaneLeaf = {
    type: "leaf",
    id: "pane1",
    agentId: "a1",
    view: "file",
    files: ["src/a.ts"],
    activeFile: "src/a.ts",
  };

  function renderFileLeaf(ctx: PaneCtx) {
    const f = TestBed.createComponent(PaneNodeComponent);
    f.componentRef.setInput("node", FILE_LEAF);
    f.componentRef.setInput("ctx", ctx);
    f.detectChanges();
    return f;
  }

  it("marks a tab dirty from the EditsStore and swaps × for the dot", () => {
    const edits = TestBed.inject(EditsStore);
    const f = renderFileLeaf(makeCtx([makeAgent()]));
    expect(f.nativeElement.querySelector(".file-tab.dirty")).toBeNull();
    edits.open("a1", "src/a.ts", "one");
    edits.update("a1", "src/a.ts", "one!");
    f.detectChanges();
    expect(f.nativeElement.querySelector(".file-tab.dirty")).not.toBeNull();
    expect(f.nativeElement.querySelector(".fdot")).not.toBeNull();
  });

  it("closing a clean tab closes immediately and drops the buffer", () => {
    const edits = TestBed.inject(EditsStore);
    edits.open("a1", "src/a.ts", "one");
    const ctx = makeCtx([makeAgent()]);
    const f = renderFileLeaf(ctx);
    f.nativeElement.querySelector<HTMLButtonElement>(".fx")!.click();
    expect(ctx.onFileClose).toHaveBeenCalledWith("pane1", "src/a.ts");
    expect(edits.get("a1", "src/a.ts")).toBeUndefined();
  });

  it("closing a dirty tab opens the confirm dialog instead", () => {
    const edits = TestBed.inject(EditsStore);
    edits.open("a1", "src/a.ts", "one");
    edits.update("a1", "src/a.ts", "one!");
    const ctx = makeCtx([makeAgent()]);
    const f = renderFileLeaf(ctx);
    f.nativeElement.querySelector<HTMLButtonElement>(".fx")!.click();
    f.detectChanges();
    expect(ctx.onFileClose).not.toHaveBeenCalled();
    expect(f.nativeElement.querySelector(".cc-card")).not.toBeNull();
  });

  it("Discard resets the buffer and closes; Cancel keeps everything", () => {
    const edits = TestBed.inject(EditsStore);
    edits.open("a1", "src/a.ts", "one");
    edits.update("a1", "src/a.ts", "one!");
    const ctx = makeCtx([makeAgent()]);
    const f = renderFileLeaf(ctx);
    f.componentInstance.confirmClose.set({ leafId: "pane1", agentId: "a1", paths: ["src/a.ts"], dirty: ["src/a.ts"] });
    f.detectChanges();

    const buttons = [...f.nativeElement.querySelectorAll<HTMLButtonElement>(".cc-actions button")];
    const cancel = buttons.find((b) => b.textContent?.includes("Cancel"))!;
    cancel.click();
    f.detectChanges();
    expect(ctx.onFileClose).not.toHaveBeenCalled();
    expect(f.nativeElement.querySelector(".cc-card")).toBeNull();

    f.componentInstance.confirmClose.set({ leafId: "pane1", agentId: "a1", paths: ["src/a.ts"], dirty: ["src/a.ts"] });
    f.detectChanges();
    const discard = [...f.nativeElement.querySelectorAll<HTMLButtonElement>(".cc-actions button")]
      .find((b) => b.textContent?.includes("Discard"))!;
    discard.click();
    expect(ctx.onFileClose).toHaveBeenCalledWith("pane1", "src/a.ts");
    expect(edits.get("a1", "src/a.ts")).toBeUndefined();
  });

  it("Save writes through the bridge, then closes the tab", async () => {
    const edits = TestBed.inject(EditsStore);
    edits.open("a1", "src/a.ts", "one");
    edits.update("a1", "src/a.ts", "one!");
    const ctx = makeCtx([makeAgent()]);
    const f = renderFileLeaf(ctx);
    f.componentInstance.saveAndClose({ leafId: "pane1", agentId: "a1", paths: ["src/a.ts"], dirty: ["src/a.ts"] });
    // saveAndClose awaits Promise.all over the per-file saves — let the
    // microtask chain fully drain before asserting
    await new Promise((r) => setTimeout(r, 0));
    expect(bridge.invoke).toHaveBeenCalledWith("file_write", {
      id: "a1",
      path: "src/a.ts",
      content: "one!",
    });
    expect(ctx.onFileClose).toHaveBeenCalledWith("pane1", "src/a.ts");
  });

  // ----- file-tab context menu: single + bulk close -----

  const MULTI_LEAF: PaneLeaf = {
    type: "leaf",
    id: "pane1",
    agentId: "a1",
    view: "file",
    files: ["src/a.ts", "src/b.ts", "src/c.ts"],
    activeFile: "src/b.ts",
  };

  function renderMultiLeaf(ctx: PaneCtx) {
    const f = TestBed.createComponent(PaneNodeComponent);
    f.componentRef.setInput("node", MULTI_LEAF);
    f.componentRef.setInput("ctx", ctx);
    f.detectChanges();
    return f;
  }

  /** The tab menu goes through UiStore.openMenu — the items land in the shared
   *  store-driven context menu, not in this component's DOM. */
  function openTabMenu(f: ReturnType<typeof renderMultiLeaf>) {
    const tabs = [...f.nativeElement.querySelectorAll<HTMLElement>(".file-tab")];
    tabs[0].dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true }));
    f.detectChanges();
    return TestBed.inject(UiStore).contextMenu()!.items;
  }

  it("right-clicking a file tab opens the close menu with left/right enablement", () => {
    const ctx = makeCtx([makeAgent()]);
    const f = renderMultiLeaf(ctx);
    const items = openTabMenu(f);
    expect(items.map((i) => i.label)).toEqual([
      "Close",
      "Close All to the Left",
      "Close All to the Right",
      "Close All",
    ]);
    // icon-then-text rule: every item leads with an icon
    expect(items.every((i) => !!i.icon)).toBe(true);
    // first tab: nothing to its left
    expect(items[1].disabled).toBe(true);
    expect(items[2].disabled).toBeFalsy();
  });

  it("Close All to the Right closes the tabs after the anchor, keeping the rest", () => {
    const ctx = makeCtx([makeAgent()]);
    const f = renderMultiLeaf(ctx);
    const right = openTabMenu(f).find((i) => i.label?.includes("Right"))!;
    right.onClick!();
    expect(ctx.onFileClose).toHaveBeenCalledWith("pane1", "src/b.ts");
    expect(ctx.onFileClose).toHaveBeenCalledWith("pane1", "src/c.ts");
    expect(ctx.onFileClose).not.toHaveBeenCalledWith("pane1", "src/a.ts");
  });

  it("Close All with a dirty buffer raises ONE dialog covering the whole set", () => {
    const edits = TestBed.inject(EditsStore);
    edits.open("a1", "src/b.ts", "one");
    edits.update("a1", "src/b.ts", "one!");
    const ctx = makeCtx([makeAgent()]);
    const f = renderMultiLeaf(ctx);
    openTabMenu(f)
      .find((i) => i.label === "Close All")!
      .onClick!();
    f.detectChanges();
    // nothing closed yet; the dialog names only the dirty file
    expect(ctx.onFileClose).not.toHaveBeenCalled();
    const cc = f.componentInstance.confirmClose()!;
    expect(cc.paths).toEqual(["src/a.ts", "src/b.ts", "src/c.ts"]);
    expect(cc.dirty).toEqual(["src/b.ts"]);

    // Discard closes every tab in the set
    f.componentInstance.discardAndClose(cc);
    expect(ctx.onFileClose).toHaveBeenCalledTimes(3);
    expect(edits.get("a1", "src/b.ts")).toBeUndefined();
  });
});
