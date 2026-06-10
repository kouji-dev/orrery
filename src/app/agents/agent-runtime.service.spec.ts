import { provideZonelessChangeDetection, signal } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { Agent } from "../models";
import { appendPtyTail } from "../utils";
import { AgentsStore } from "../stores/agents.store";
import { NotificationStore } from "../stores/notifications.store";
import { UiStore } from "../ui/ui.store";
import { TerminalService } from "../terminal.service";
import { AgentWorkStore } from "./agent-work.store";
import { AgentRuntimeService } from "./agent-runtime.service";

// Spy-wrap appendPtyTail so the specs can prove WHEN folding happens (never on
// push for hook-driven tools; lazily on read for the exit/gemini paths). The
// wrapper delegates to the real implementation — behavior is unchanged.
vi.mock("../utils", async (importOriginal) => {
  const actual = await importOriginal<typeof import("../utils")>();
  return { ...actual, appendPtyTail: vi.fn(actual.appendPtyTail) };
});
const foldSpy = vi.mocked(appendPtyTail);

function makeAgent(p: Partial<Agent> & { id: string; tool: Agent["tool"] }): Agent {
  return {
    projectId: "p1",
    model: "m",
    name: p.id,
    task: "",
    status: "running",
    branch: "b",
    worktree: "w",
    base: "main",
    commits: 0,
    elapsed: 0,
    progress: 0,
    pending: [],
    ...p,
  } as Agent;
}

type OutputCb = (entries: { id: string; chunk: string; seq?: number }[]) => void;

beforeAll(() => {
  try {
    TestBed.initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by another spec in this worker
  }
});

describe("AgentRuntimeService — lazy PTY tail folding", () => {
  let output: OutputCb;
  let exit: (id: string) => void;
  let notifications: { pending: () => never[]; push: ReturnType<typeof vi.fn>; dismissPendingFor: ReturnType<typeof vi.fn> };
  let terminals: { write: ReturnType<typeof vi.fn>; exit: ReturnType<typeof vi.fn>; dispose: ReturnType<typeof vi.fn>; size: () => null; syncSize: ReturnType<typeof vi.fn>; onTitle: ReturnType<typeof vi.fn> };

  beforeEach(() => {
    vi.useFakeTimers();
    foldSpy.mockClear();
  });
  afterEach(() => {
    TestBed.resetTestingModule();
    vi.useRealTimers();
  });

  function setup(agents: Agent[]): AgentRuntimeService {
    const agentsStore = {
      all: signal(agents),
      detectTools: () => Promise.resolve([]),
      watch: () => Promise.resolve(),
      onScan: () => new Promise<() => void>(() => {}),
      onPermission: () => Promise.resolve(() => {}),
      onStatus: () => Promise.resolve(() => {}),
      onActivity: () => Promise.resolve(() => {}),
      onOutput: (cb: OutputCb) => {
        output = cb;
        return Promise.resolve(() => {});
      },
      onExit: (cb: (id: string) => void) => {
        exit = cb;
        return Promise.resolve(() => {});
      },
      start: vi.fn(() => Promise.resolve()),
      stop: vi.fn(() => Promise.resolve()),
      update: vi.fn(() => Promise.resolve()),
    };
    notifications = { pending: () => [], push: vi.fn(() => null), dismissPendingFor: vi.fn() };
    terminals = { write: vi.fn(), exit: vi.fn(), dispose: vi.fn(), size: () => null, syncSize: vi.fn(), onTitle: vi.fn() };
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        AgentRuntimeService,
        { provide: AgentsStore, useValue: agentsStore },
        { provide: NotificationStore, useValue: notifications },
        { provide: TerminalService, useValue: terminals },
        { provide: UiStore, useValue: { activeTab: signal("orchestrator"), paneRoots: signal({}), scopeAgentId: signal(null), flash: vi.fn() } },
        { provide: AgentWorkStore, useValue: { applyScan: vi.fn(), ensureTree: vi.fn(), ensureCommits: vi.fn(), dispose: vi.fn() } },
      ],
    });
    return TestBed.inject(AgentRuntimeService);
  }

  it("hook-driven tool: streaming never folds; exit folds once for the notification detail", () => {
    const svc = setup([makeAgent({ id: "a1", tool: "claude" })]);
    output([{ id: "a1", chunk: "line one\r\n" }]);
    output([{ id: "a1", chunk: "line two\r\n" }]);
    vi.advanceTimersByTime(5000); // many liveness ticks — the hook path never reads the tail
    expect(foldSpy).not.toHaveBeenCalled();
    expect(terminals.write).toHaveBeenCalledTimes(2); // xterm still gets the raw stream

    exit("a1");
    expect(foldSpy).toHaveBeenCalledTimes(2); // both buffered chunks, folded exactly once
    expect(notifications.push).toHaveBeenCalledWith(
      expect.objectContaining({ kind: "done", detail: "line one\nline two" }),
    );
    // post-exit tail still carries the exited marker (parity with the old sys line)
    expect(svc.promptTail("a1")).toBe("line one\nline two\n▪ process exited");
  });

  it("gemini: tail folds lazily on the heuristic read, caches until new output, raises the same prompt", () => {
    setup([makeAgent({ id: "g1", tool: "gemini" })]);
    output([{ id: "g1", chunk: "thinking...\r\n" }]);
    output([{ id: "g1", chunk: "Apply this patch?\r\n" }]);

    vi.advanceTimersByTime(800); // tick 1: output is recent → working → tail not read
    expect(foldSpy).not.toHaveBeenCalled();

    vi.advanceTimersByTime(800); // tick 2: idle → heuristic reads the tail → folds now
    expect(foldSpy).toHaveBeenCalledTimes(2);
    expect(notifications.push).toHaveBeenCalledTimes(1);
    expect(notifications.push).toHaveBeenCalledWith(
      expect.objectContaining({
        kind: "question",
        title: "g1 has a question",
        detail: "thinking...\nApply this patch?",
      }),
    );

    vi.advanceTimersByTime(800); // tick 3: no new output → cached fold, no re-raise
    expect(foldSpy).toHaveBeenCalledTimes(2);
    expect(notifications.push).toHaveBeenCalledTimes(1);

    output([{ id: "g1", chunk: "more\r\n" }]); // new raw chunk → dirty
    vi.advanceTimersByTime(1600); // working tick, then idle tick re-folds the new chunk only
    expect(foldSpy).toHaveBeenCalledTimes(3);
  });

  it("dispose clears the raw buffer — no fold of removed agents' output", () => {
    const svc = setup([makeAgent({ id: "a1", tool: "claude" })]);
    output([{ id: "a1", chunk: "secret scrollback\r\n" }]);
    svc.dispose("a1");
    expect(svc.promptTail("a1")).toBe("");
    expect(foldSpy).not.toHaveBeenCalled();
    expect(terminals.dispose).toHaveBeenCalledWith("a1");
  });
});
