import { provideZonelessChangeDetection } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Bridge, BRIDGE, BridgeError } from "../data-source/bridge";
import { UiStore } from "../ui/ui.store";
import { ScrollStateService } from "../workspace/scroll-state.service";
import { WORKSPACE_SAVE_DEBOUNCE_MS, WorkspaceDoc, WorkspaceStore } from "./workspace.store";

const LS_KEY = "orrery.workspace";
const LEGACY_RESUME_KEY = "orrery:update-restore";

function docFixture(partial: Partial<WorkspaceDoc> = {}): Partial<WorkspaceDoc> {
  return {
    v: 2,
    tabs: [
      { id: "orchestrator", kind: "orchestrator" },
      { id: "backlog", kind: "backlog" },
      { id: "tab4", kind: "agent" },
    ],
    activeTab: "tab4",
    scopeAgentId: "a1",
    paneRoots: { tab4: { type: "leaf", id: "pane7", agentId: "a1", view: "terminal" } },
    gitViews: {},
    diffSelections: { a1: "src/x.ts" },
    diffListWidth: 420,
    scroll: { plain: { "a1:docs/x.md": 640 }, view: {}, diff: {} },
    updateResume: null,
    ...partial,
  };
}

describe("WorkspaceStore", () => {
  let invoke: ReturnType<typeof vi.fn>;

  function make(getResult: unknown | (() => unknown)): WorkspaceStore {
    invoke = vi.fn(async (cmd: string) => {
      if (cmd === "workspace_get") {
        return typeof getResult === "function" ? (getResult as () => unknown)() : getResult;
      }
      return undefined;
    });
    const bridge: Bridge = {
      invoke: invoke as Bridge["invoke"],
      on: async () => () => {},
      pickDirectory: async () => null,
    };
    TestBed.configureTestingModule({
      providers: [provideZonelessChangeDetection(), { provide: BRIDGE, useValue: bridge }],
    });
    return TestBed.inject(WorkspaceStore);
  }

  const setCalls = () => invoke.mock.calls.filter(([cmd]) => cmd === "workspace_set");

  beforeEach(() => localStorage.clear());
  afterEach(() => vi.useRealTimers());

  it("hydrates UiStore + scroll from the backend doc before ready() resolves", async () => {
    const store = make(docFixture());
    await store.ready();
    const ui = TestBed.inject(UiStore);
    expect(ui.tabs().map((t) => t.id)).toEqual(["orchestrator", "backlog", "tab4"]);
    expect(ui.activeTab()).toBe("tab4");
    expect(ui.scopeAgentId()).toBe("a1");
    expect(ui.diffSelections()["a1"]).toBe("src/x.ts");
    expect(ui.diffListWidth()).toBe(420);
    expect(TestBed.inject(ScrollStateService).getPlain("a1", "docs/x.md")).toBe(640);
  });

  it("drains a fresh updateResume into UiStore one-shot; stale is ignored", async () => {
    const store = make(docFixture({ updateResume: { at: Date.now(), resume: ["a1"] } }));
    await store.ready();
    expect(TestBed.inject(UiStore).updateResumeIds).toEqual(["a1"]);
    // the drained list never persists back
    await store.flush();
    const last = setCalls().at(-1)![1] as { doc: WorkspaceDoc };
    expect(last.doc.updateResume).toBeNull();
  });

  it("ignores a stale updateResume (failed install that never relaunched)", async () => {
    const store = make(docFixture({ updateResume: { at: Date.now() - 16 * 60_000, resume: ["a1"] } }));
    await store.ready();
    expect(TestBed.inject(UiStore).updateResumeIds).toEqual([]);
  });

  it("migrates the legacy localStorage v1 doc + resume key on first backend load", async () => {
    localStorage.setItem(
      LS_KEY,
      JSON.stringify({
        v: 1,
        tabs: [
          { id: "orchestrator", kind: "orchestrator" },
          { id: "backlog", kind: "backlog" },
          { id: "tab2", kind: "agent" },
        ],
        activeTab: "tab2",
        scopeAgentId: "a9",
        paneRoots: { tab2: { type: "leaf", id: "pane1", agentId: "a9", view: "terminal" } },
      }),
    );
    localStorage.setItem(LEGACY_RESUME_KEY, JSON.stringify({ v: 1, at: Date.now(), resume: ["a9"] }));
    const store = make(null); // backend: never saved
    await store.ready();
    const ui = TestBed.inject(UiStore);
    expect(ui.activeTab()).toBe("tab2");
    expect(ui.updateResumeIds).toEqual(["a9"]);
    // webview copies are consumed — SQLite is the source of truth now
    expect(localStorage.getItem(LS_KEY)).toBeNull();
    expect(localStorage.getItem(LEGACY_RESUME_KEY)).toBeNull();
    await store.flush();
    expect((setCalls().at(-1)![1] as { doc: WorkspaceDoc }).doc.v).toBe(2);
  });

  it("falls back to localStorage round-trips when the backend is absent", async () => {
    localStorage.setItem(LS_KEY, JSON.stringify(docFixture({ activeTab: "tab4" })));
    const store = make(() => {
      throw new BridgeError("unknown", "no tauri");
    });
    await store.ready();
    expect(TestBed.inject(UiStore).activeTab()).toBe("tab4");
    await store.flush();
    expect(setCalls()).toHaveLength(0, "no backend writes in fallback mode");
    const written = JSON.parse(localStorage.getItem(LS_KEY)!);
    expect(written.v).toBe(2);
  });

  it("debounces persistence: rapid changes coalesce into one workspace_set", async () => {
    const store = make(docFixture());
    await store.ready();
    vi.useFakeTimers();
    const ui = TestBed.inject(UiStore);
    ui.activeTab.set("orchestrator");
    TestBed.tick(); // run the persist effect
    ui.activeTab.set("backlog");
    TestBed.tick();
    expect(setCalls()).toHaveLength(0);
    vi.advanceTimersByTime(WORKSPACE_SAVE_DEBOUNCE_MS + 10);
    expect(setCalls()).toHaveLength(1);
    expect((setCalls()[0][1] as { doc: WorkspaceDoc }).doc.activeTab).toBe("backlog");
  });

  it("scroll saves trigger persistence via the rev signal", async () => {
    const store = make(docFixture());
    await store.ready();
    vi.useFakeTimers();
    TestBed.inject(ScrollStateService).savePlain("a1", "docs/y.md", 320);
    TestBed.tick();
    vi.advanceTimersByTime(WORKSPACE_SAVE_DEBOUNCE_MS + 10);
    const last = setCalls().at(-1)![1] as { doc: WorkspaceDoc };
    expect(last.doc.scroll.plain["a1:docs/y.md"]).toBe(320);
  });
});
