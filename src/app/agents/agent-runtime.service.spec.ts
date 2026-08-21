import { provideZonelessChangeDetection, signal } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, beforeEach, describe, expect, it, vi } from "vitest";
import { Agent, Settings } from "../models";
import { AgentDigestEntry, AgentPtyStatusPayload } from "../data-source/bridge";
import { settingsDefaults, SettingsStore } from "../settings/settings.store";
import { AgentsStore } from "../stores/agents.store";
import { NotificationStore } from "../stores/notifications.store";
import { WorkspaceStore } from "../stores/workspace.store";
import { UiStore } from "../ui/ui.store";
import { TerminalService } from "../terminal.service";
import { AgentWorkStore } from "./agent-work.store";
import { AgentRuntimeService } from "./agent-runtime.service";

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

let output: OutputCb;
let exit: (id: string) => void;
let ptyStatus: (p: AgentPtyStatusPayload) => void;
let digest: (entries: AgentDigestEntry[]) => void;
let notifications: { pending: () => never[]; push: ReturnType<typeof vi.fn>; dismissPendingFor: ReturnType<typeof vi.fn> };
let terminals: { write: ReturnType<typeof vi.fn>; exit: ReturnType<typeof vi.fn>; dispose: ReturnType<typeof vi.fn>; size: () => null; syncSize: ReturnType<typeof vi.fn>; onTitle: ReturnType<typeof vi.fn> };

beforeEach(() => {
  vi.useFakeTimers();
});
afterEach(() => {
  TestBed.resetTestingModule();
  vi.useRealTimers();
});

function setup(
  agents: Agent[],
  opts: { settings?: Partial<Settings>; interrupted?: string[] } = {},
): AgentRuntimeService {
  const settings: Settings = { ...settingsDefaults(), ...opts.settings };
  const agentsStore = {
    all: signal(agents),
    ready: () => Promise.resolve(),
    interrupted: vi.fn(() => Promise.resolve(opts.interrupted ?? [])),
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
    onPtyStatus: (cb: (p: AgentPtyStatusPayload) => void) => {
      ptyStatus = cb;
      return Promise.resolve(() => {});
    },
    onDigest: (cb: (entries: AgentDigestEntry[]) => void) => {
      digest = cb;
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
      { provide: SettingsStore, useValue: { settings: signal(settings), ready: () => Promise.resolve(settings) } },
      { provide: WorkspaceStore, useValue: { ready: () => Promise.resolve() } },
      { provide: UiStore, useValue: { activeTab: signal("orchestrator"), paneRoots: signal({}), scopeAgentId: signal(null), flash: vi.fn(), openAgent: vi.fn() } },
      { provide: AgentWorkStore, useValue: { applyScan: vi.fn(), ensureTree: vi.fn(), ensureCommits: vi.fn(), dispose: vi.fn(), dropTotals: vi.fn() } },
    ],
  });
  return TestBed.inject(AgentRuntimeService);
}

/** Let the constructor's fire-and-forget auto-resume promise chain settle
 *  (settings.ready → agents ready → agents_interrupted → start). Microtasks
 *  only — unaffected by the fake timers. */
async function flushMicrotasks(): Promise<void> {
  for (let i = 0; i < 10; i++) await Promise.resolve();
}

describe("AgentRuntimeService — idle tick timer gating", () => {
  it("starts ticking when an agent transitions to running", () => {
    const svc = setup([makeAgent({ id: "a1", tool: "claude", status: "idle" })]);
    const t0 = svc.now();
    vi.advanceTimersByTime(4000); // no timer running yet → now stays parked
    expect(svc.now()).toBe(t0);
    // transition to running
    svc.startProcess("a1");
    const tAfterStart = svc.now();
    vi.advanceTimersByTime(1600); // two ticks should advance the clock
    expect(svc.now()).toBeGreaterThan(tAfterStart);
  });

  it("stops ticking when the last running agent exits", () => {
    const svc = setup([makeAgent({ id: "a1", tool: "claude" })]);
    svc.startProcess("a1");
    vi.advanceTimersByTime(800);
    const tBeforeExit = svc.now();
    exit("a1");
    vi.advanceTimersByTime(4000); // timer must have been cleared — clock stays frozen
    expect(svc.now()).toBe(tBeforeExit);
  });

  it("parks the timer and drops startedAt when start() fails", async () => {
    const svc = setup([makeAgent({ id: "f1", tool: "claude", status: "idle" })]);
    const store = TestBed.inject(AgentsStore) as unknown as {
      start: ReturnType<typeof vi.fn>;
    };
    store.start.mockImplementation(() => Promise.reject({ message: "worktree not found" }));
    svc.startProcess("f1");
    await vi.advanceTimersByTimeAsync(0); // let the rejection's catch run
    const tFrozen = svc.now();
    await vi.advanceTimersByTimeAsync(4000);
    expect(svc.now()).toBe(tFrozen); // timer parked again — no live run remains
    expect(svc.elapsedFor("f1")).toBe(0); // startedAt dropped — nothing counts up
  });

  it("restarts ticking when a new process starts after all have exited", () => {
    const svc = setup([makeAgent({ id: "a1", tool: "claude" })]);
    svc.startProcess("a1");
    vi.advanceTimersByTime(800);
    exit("a1");
    const tFrozen = svc.now();
    vi.advanceTimersByTime(4000); // frozen
    expect(svc.now()).toBe(tFrozen);
    // restart
    svc.startProcess("a1");
    vi.advanceTimersByTime(1600);
    expect(svc.now()).toBeGreaterThan(tFrozen);
  });
});

describe("AgentRuntimeService — shared clock & stable agents identity", () => {
  it("agents() keeps array AND object identities across clock ticks (zero churn from the clock)", () => {
    const svc = setup([makeAgent({ id: "a1", tool: "claude" })]);
    svc.startProcess("a1");
    vi.advanceTimersByTime(800); // settle tick: no output yet → working flips true→false once
    const before = svc.agents();
    vi.advanceTimersByTime(4000); // five more ticks — nothing real changes
    const after = svc.agents();
    expect(after).toBe(before); // same array identity
    expect(after[0]).toBe(before[0]); // same merged-object identity
    expect(after[0].elapsed).toBe(0); // the clock no longer patches elapsed into runtime state
  });

  it("now() ticks while an agent is running", () => {
    const svc = setup([makeAgent({ id: "r1", tool: "claude" })]); // status: running
    const t0 = svc.now();
    vi.advanceTimersByTime(3200);
    expect(svc.now()).toBeGreaterThan(t0);
  });

  it("now() stays parked when no agent is running", () => {
    const svc = setup([makeAgent({ id: "i1", tool: "claude", status: "idle" })]);
    const t0 = svc.now();
    vi.advanceTimersByTime(3200);
    expect(svc.now()).toBe(t0);
  });

  it("elapsedFor() derives live seconds from the clock, freezes at exit, clears on dispose", () => {
    const svc = setup([makeAgent({ id: "a1", tool: "claude" })]);
    expect(svc.elapsedFor("a1")).toBe(0); // never started
    svc.startProcess("a1");
    vi.advanceTimersByTime(5000); // last clock tick at 4800ms → round(4.8) = 5
    expect(svc.elapsedFor("a1")).toBe(5);
    exit("a1");
    vi.advanceTimersByTime(3000);
    expect(svc.elapsedFor("a1")).toBe(5); // frozen at the exit value
    svc.dispose("a1");
    expect(svc.elapsedFor("a1")).toBe(0);
  });
});

describe("AgentRuntimeService — Rust PTY heuristics (A0.3, agent://pty-status)", () => {
  const ev = (
    id: string,
    p: Partial<AgentPtyStatusPayload> = {},
  ): AgentPtyStatusPayload => ({
    id,
    working: false,
    needsInput: false,
    permission: false,
    detail: "",
    ...p,
  });

  it("streams raw output (with seq) to xterm; exit uses the pty-status tail as detail", () => {
    const svc = setup([makeAgent({ id: "a1", tool: "claude" })]);
    output([{ id: "a1", chunk: "line one\r\n", seq: 10 }]);
    output([{ id: "a1", chunk: "line two\r\n", seq: 20 }]);
    expect(terminals.write).toHaveBeenCalledTimes(2);
    expect(terminals.write).toHaveBeenLastCalledWith("a1", "line two\r\n", 20);

    exit("a1");
    // no pty-status ever arrived (hook tool) → the generic detail
    expect(notifications.push).toHaveBeenCalledWith(
      expect.objectContaining({
        kind: "done",
        detail: "Process ended — review or merge its work.",
      }),
    );
    expect(svc.promptTail("a1")).toBe("");
  });

  it("gemini: a needs-input transition raises ONE question with the backend detail", () => {
    setup([makeAgent({ id: "g1", tool: "gemini" })]);
    // Backend heuristics: working while streaming — no notification.
    ptyStatus(ev("g1", { working: true, detail: "thinking..." }));
    expect(notifications.push).not.toHaveBeenCalled();

    // Transition: quiet + trailing question → needsInput (open question).
    ptyStatus(
      ev("g1", { needsInput: true, detail: "thinking...\nApply this patch?" }),
    );
    expect(notifications.push).toHaveBeenCalledTimes(1);
    expect(notifications.push).toHaveBeenCalledWith(
      expect.objectContaining({
        kind: "question",
        title: "g1 has a question",
        detail: "thinking...\nApply this patch?",
      }),
    );

    // Same state re-pushed (backend dedups, but belt-and-braces): no re-raise.
    ptyStatus(
      ev("g1", { needsInput: true, detail: "thinking...\nApply this patch?" }),
    );
    expect(notifications.push).toHaveBeenCalledTimes(1);

    // Input answered → needsInput falls → pending prompts dismissed.
    ptyStatus(ev("g1", { working: true, detail: "continuing" }));
    expect(notifications.dismissPendingFor).toHaveBeenCalledWith("g1", [
      "permission",
      "question",
    ]);
  });

  it("gemini: a permission-classified prompt raises kind=permission", () => {
    setup([makeAgent({ id: "g1", tool: "gemini" })]);
    ptyStatus(
      ev("g1", {
        needsInput: true,
        permission: true,
        detail: "Run `rm -rf dist`? [y/N]",
      }),
    );
    expect(notifications.push).toHaveBeenCalledWith(
      expect.objectContaining({
        kind: "permission",
        title: "g1 needs permission",
        detail: "Run `rm -rf dist`? [y/N]",
      }),
    );
  });

  it("hook-driven tools never double-raise from pty-status (backend hooks own them)", () => {
    setup([makeAgent({ id: "a1", tool: "claude" })]);
    ptyStatus(ev("a1", { needsInput: true, detail: "Proceed? (y/n)" }));
    expect(notifications.push).not.toHaveBeenCalled();
  });

  it("pty-status for a removed agent is ignored (no state leak)", () => {
    const svc = setup([makeAgent({ id: "g1", tool: "gemini" })]);
    ptyStatus(ev("ghost", { needsInput: true, detail: "?" }));
    expect(notifications.push).not.toHaveBeenCalled();
    expect(svc.promptTail("ghost")).toBe("");
  });

  it("dispose clears the heuristics state", () => {
    const svc = setup([makeAgent({ id: "g1", tool: "gemini" })]);
    ptyStatus(ev("g1", { working: true, detail: "context" }));
    expect(svc.promptTail("g1")).toBe("context");
    svc.dispose("g1");
    expect(svc.promptTail("g1")).toBe("");
    expect(terminals.dispose).toHaveBeenCalledWith("g1");
  });
});

describe("AgentRuntimeService — digest lines (A0.2, agent://digest)", () => {
  it("stores the latest digest lines per agent for the mini-previews", () => {
    const svc = setup([makeAgent({ id: "a1", tool: "claude" })]);
    digest([{ id: "a1", lines: ["one", "two"], seq: 10 }]);
    expect(svc.digestFor("a1")).toEqual(["one", "two"]);
    digest([{ id: "a1", lines: ["three"], seq: 20 }]);
    expect(svc.digestFor("a1")).toEqual(["three"]); // last write wins
  });

  it("ignores digests for unknown agents and clears on dispose", () => {
    const svc = setup([makeAgent({ id: "a1", tool: "claude" })]);
    digest([{ id: "ghost", lines: ["x"], seq: 1 }]);
    expect(svc.digestFor("ghost")).toEqual([]);
    digest([{ id: "a1", lines: ["x"], seq: 1 }]);
    svc.dispose("a1");
    expect(svc.digestFor("a1")).toEqual([]);
  });
});

describe("AgentRuntimeService — settings auto-resume", () => {
  it("autoResume ON: relaunches interrupted agents that have a session, with resume:true", async () => {
    setup(
      [
        makeAgent({ id: "a1", tool: "claude", status: "idle", sessionId: "s-1" }),
        makeAgent({ id: "a2", tool: "claude", status: "idle" }), // no captured session — skipped
      ],
      { settings: { autoResume: true }, interrupted: ["a1", "a2", "ghost"] }, // ghost: removed since
    );
    await flushMicrotasks();
    const store = TestBed.inject(AgentsStore) as unknown as {
      start: ReturnType<typeof vi.fn>;
      interrupted: ReturnType<typeof vi.fn>;
    };
    expect(store.interrupted).toHaveBeenCalledTimes(1);
    expect(store.start).toHaveBeenCalledTimes(1);
    expect(store.start).toHaveBeenCalledWith("a1", 0, 0, true);
  });

  it("autoResume OFF: never drains the backend's interrupted snapshot", async () => {
    setup([makeAgent({ id: "a1", tool: "claude", status: "idle", sessionId: "s-1" })], {
      settings: { autoResume: false },
      interrupted: ["a1"],
    });
    await flushMicrotasks();
    const store = TestBed.inject(AgentsStore) as unknown as {
      start: ReturnType<typeof vi.fn>;
      interrupted: ReturnType<typeof vi.fn>;
    };
    expect(store.interrupted).not.toHaveBeenCalled();
    expect(store.start).not.toHaveBeenCalled();
  });
});
