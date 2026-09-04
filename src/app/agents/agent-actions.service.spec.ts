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
