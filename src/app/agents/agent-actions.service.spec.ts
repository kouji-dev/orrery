import { Injector } from "@angular/core";
import { describe, it, expect, beforeEach, vi } from "vitest";
import { Agent } from "../models";
import { AgentActionsService } from "./agent-actions.service";
import { AgentRuntimeService } from "./agent-runtime.service";
import { AgentWorkStore } from "./agent-work.store";
import { ConflictStore } from "./conflict.store";
import { NotificationStore } from "../stores/notifications.store";
import { AgentsStore } from "../stores/agents.store";
import { TerminalService } from "../terminal.service";
import { UiStore } from "../ui/ui.store";
import { ProjectActionsService } from "../projects/project-actions.service";

/**
 * The create/remove round trips can take seconds (worktree checkout, folder
 * delete), so the service must show the row state BEFORE the invoke settles:
 * a placeholder row for spawn, a "removing" overlay for delete.
 */
function deferred<T>() {
  let resolve!: (v: T) => void;
  let reject!: (e: unknown) => void;
  const promise = new Promise<T>((res, rej) => { resolve = res; reject = rej; });
  return { promise, resolve, reject };
}

const agent = (over: Partial<Agent> = {}): Agent => ({
  id: "a1", projectId: "p1", tool: "claude", model: "m", name: "alpha", task: "",
  status: "idle", branch: "agent/alpha", worktree: "C:/wt/alpha", base: "main",
  commits: 0, elapsed: 0, progress: 0, pending: [], ...over,
});

describe("AgentActionsService pending transitions", () => {
  let placeholders: Agent[];
  let overlay: Record<string, Partial<Agent>>;
  let spawnDeferred: ReturnType<typeof deferred<Agent>>;
  let removeDeferred: ReturnType<typeof deferred<void>>;
  let svc: AgentActionsService;
  let flashes: string[];

  beforeEach(() => {
    placeholders = [];
    overlay = {};
    flashes = [];
    spawnDeferred = deferred<Agent>();
    removeDeferred = deferred<void>();
    const agentsStore = {
      spawn: vi.fn(() => spawnDeferred.promise),
      remove: vi.fn(() => removeDeferred.promise),
      addPlaceholder: (a: Agent) => placeholders.push(a),
      dropPlaceholder: (id: string) => { placeholders = placeholders.filter((a) => a.id !== id); },
    } as unknown as AgentsStore;
    const runtime = {
      agents: () => [agent()],
      patchRuntime: (id: string, patch: Partial<Agent>) => { overlay[id] = { ...(overlay[id] ?? {}), ...patch }; },
      startProcess: () => {},
      stopProcess: () => {},
      dispose: (id: string) => { delete overlay[id]; },
    } as unknown as AgentRuntimeService;
    const ui = {
      closeSpawn: () => {}, closeDeleteWorktree: () => {}, closeTabsForAgent: () => {},
      openAgent: () => {}, flash: (m: string) => flashes.push(m),
    } as unknown as UiStore;
    const injector = Injector.create({
      providers: [
        { provide: AgentsStore, useValue: agentsStore },
        { provide: AgentRuntimeService, useValue: runtime },
        { provide: UiStore, useValue: ui },
        { provide: ProjectActionsService, useValue: { all: () => [{ id: "p1", name: "proj" }] } },
        { provide: TerminalService, useValue: { hint: () => {} } },
        { provide: NotificationStore, useValue: { clearAgent: () => {} } },
        { provide: AgentWorkStore, useValue: {} },
        { provide: ConflictStore, useValue: { dispose: () => {} } },
        AgentActionsService,
      ],
    });
    svc = injector.get(AgentActionsService);
  });

  const req = { projectId: "p1", branch: "main", toolId: "claude" as const, model: "m", effort: null, name: "new-one", prompt: "do it" };

  it("spawn shows a creating placeholder immediately, under the id it sends", async () => {
    const p = svc.spawn(req);
    expect(placeholders).toHaveLength(1);
    expect(placeholders[0].transition).toBe("creating");
    expect(placeholders[0].name).toBe("new-one");
    const sent = (svc["agentsStore"].spawn as ReturnType<typeof vi.fn>).mock.calls[0][0] as { id: string };
    expect(sent.id).toBe(placeholders[0].id);

    spawnDeferred.resolve(agent({ id: sent.id, name: "new-one" }));
    await p;
    expect(placeholders).toHaveLength(0);
  });

  it("spawn drops the placeholder when the backend fails", async () => {
    const p = svc.spawn(req);
    expect(placeholders).toHaveLength(1);
    spawnDeferred.reject(new Error("worktree add: boom"));
    await p;
    expect(placeholders).toHaveLength(0);
    expect(flashes).toContain("worktree add: boom");
  });

  it("remove marks the row removing at once and clears it on failure", async () => {
    const p = svc.confirmRemoveAgent("a1", true);
    expect(overlay["a1"]?.transition).toBe("removing");
    removeDeferred.reject(new Error("locked"));
    await p;
    expect(overlay["a1"]?.transition).toBeUndefined();
    expect(flashes.some((f) => f.startsWith("delete failed"))).toBe(true);
  });

  it("remove disposes the overlay on success", async () => {
    const p = svc.confirmRemoveAgent("a1");
    expect(overlay["a1"]?.transition).toBe("removing");
    removeDeferred.resolve();
    await p;
    expect(overlay["a1"]).toBeUndefined();
  });
});

/**
 * Editing an existing agent. Before this, tool/model/effort were frozen at
 * spawn and every launch AND resume forwarded them — so the only guarantee
 * worth pinning here is that the edit reaches the record, and that the one case
 * which costs a live process (a provider switch on a RUNNING agent) stops
 * before it updates and starts again after.
 */
describe("AgentActionsService — applyAgentEdit", () => {
  let svc: AgentActionsService;
  let flashes: string[];
  let calls: string[];
  let update: ReturnType<typeof vi.fn>;
  let startProcess: ReturnType<typeof vi.fn>;
  let stopProcess: ReturnType<typeof vi.fn>;
  let closeEditAgent: ReturnType<typeof vi.fn>;
  let openEditAgent: ReturnType<typeof vi.fn>;
  let current: Agent;

  function build(ag: Agent, updateImpl?: () => Promise<Agent>) {
    current = ag;
    flashes = [];
    calls = [];
    update = vi.fn(() => {
      calls.push("update");
      return updateImpl ? updateImpl() : Promise.resolve(ag);
    });
    startProcess = vi.fn(() => calls.push("start"));
    // the real stopProcess resolves the backend round-trip; applyAgentEdit
    // AWAITS it, so the stub has to be a promise or the ordering assertion
    // below would pass by accident
    stopProcess = vi.fn(() => {
      calls.push("stop");
      return Promise.resolve();
    });
    closeEditAgent = vi.fn();
    openEditAgent = vi.fn();
    const injector = Injector.create({
      providers: [
        { provide: AgentsStore, useValue: { update } as unknown as AgentsStore },
        {
          provide: AgentRuntimeService,
          useValue: { agents: () => [current], startProcess, stopProcess } as unknown as AgentRuntimeService,
        },
        {
          provide: UiStore,
          useValue: { closeEditAgent, openEditAgent, flash: (m: string) => flashes.push(m) } as unknown as UiStore,
        },
        { provide: ProjectActionsService, useValue: { all: () => [{ id: "p1", name: "proj", branch: "main" }] } },
        { provide: TerminalService, useValue: {} },
        { provide: NotificationStore, useValue: {} },
        { provide: AgentWorkStore, useValue: { changesFor: () => ({ data: [] }) } },
        { provide: ConflictStore, useValue: {} },
        AgentActionsService,
      ],
    });
    svc = injector.get(AgentActionsService);
  }

  const patch = { tool: "codex" as const, model: "gpt-5.6-sol", effort: "high" };

  it("sends tool, model and effort in ONE update — the backend resets the last two on a tool change", async () => {
    build(agent({ tool: "claude", model: "opus", effort: "xhigh" }));
    await svc.applyAgentEdit("a1", patch);
    expect(update).toHaveBeenCalledTimes(1);
    expect(update).toHaveBeenCalledWith("a1", patch);
  });

  it("sends effort explicitly as null so a knobless tool does not inherit the old level", async () => {
    build(agent({ tool: "claude", model: "opus", effort: "xhigh" }));
    await svc.applyAgentEdit("a1", { tool: "cursor", model: "composer-2.5", effort: null });
    const sent = update.mock.calls[0][1] as Record<string, unknown>;
    expect("effort" in sent).toBe(true); // an OMITTED key means "leave alone"
    expect(sent.effort).toBeNull();
  });

  it("an idle agent is only updated — nothing is stopped or started", async () => {
    build(agent({ status: "idle" }));
    await svc.applyAgentEdit("a1", patch);
    expect(calls).toEqual(["update"]);
    expect(stopProcess).not.toHaveBeenCalled();
    expect(startProcess).not.toHaveBeenCalled();
  });

  it("restart stops, updates, then starts — in that order, and never resumes", async () => {
    build(agent({ status: "running", started: true, sessionId: "sess-1" }));
    await svc.applyAgentEdit("a1", patch, true);
    expect(calls).toEqual(["stop", "update", "start"]);
    // a fresh launch, not a resume: the update drops the session id with the
    // tool it belonged to, so resuming would hand the new CLI a foreign id
    expect(startProcess).toHaveBeenCalledWith("a1");
    expect(flashes.some((f) => f.includes("restarted"))).toBe(true);
  });

  it("a failed restart leaves the agent stopped and says so instead of starting it anyway", async () => {
    build(agent({ status: "running" }), () => Promise.reject(new Error("tool unknown")));
    await svc.applyAgentEdit("a1", patch, true);
    expect(calls).toEqual(["stop", "update"]);
    expect(startProcess).not.toHaveBeenCalled();
    expect(flashes).toContain("tool unknown");
    expect(flashes.some((f) => f.includes("is stopped"))).toBe(true);
  });

  it("a model-only edit on a RUNNING agent says when it lands, rather than implying it already did", async () => {
    build(agent({ status: "running", tool: "claude", model: "opus" }));
    await svc.applyAgentEdit("a1", { tool: "claude", model: "sonnet", effort: "high" });
    expect(calls).toEqual(["update"]); // the live process is untouched
    expect(flashes.some((f) => f.includes("applies on next start"))).toBe(true);
  });

  it("the agent context menu carries an Edit agent item that opens the dialog", () => {
    build(agent());
    const items = svc.agentMenu("a1");
    const edit = items.find((i) => i.label === "Edit agent")!;
    expect(edit).toBeTruthy();
    // project rule: every context-menu item leads with an app-icon then its text
    expect(edit.icon).toBeTruthy();
    edit.onClick!();
    expect(openEditAgent).toHaveBeenCalledWith("a1");
    // …and it is not the branch-rename item, which now carries its own glyph so
    // the two pencils can't be told apart only by reading their labels
    expect(items.find((i) => i.label === "Rename branch")?.icon).not.toBe(edit.icon);
  });

  it("offers the edit for a RUNNING agent too — the provider switch is the point", () => {
    build(agent({ status: "running" }));
    expect(svc.agentMenu("a1").some((i) => i.label === "Edit agent" && !i.disabled)).toBe(true);
  });
});
