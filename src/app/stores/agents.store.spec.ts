import { describe, it, expect, vi } from "vitest";
import { Injector, runInInjectionContext } from "@angular/core";
import { AgentsStore } from "./agents.store";
import { AgentOutputEntry, BRIDGE, Bridge, Commands, Events } from "../data-source/bridge";

// A fake Bridge that records invoke calls and lets a test fire `on` handlers.
function fakeBridge() {
  const invokes: { command: string; payload?: Record<string, unknown> }[] = [];
  const handlers: Record<string, (p: unknown) => void> = {};
  const bridge: Bridge = {
    invoke: vi.fn(<R>(command: string, payload?: Record<string, unknown>) => {
      invokes.push({ command, payload });
      return Promise.resolve(undefined as R);
    }),
    on: vi.fn((event: string, handler: (p: unknown) => void) => {
      handlers[event] = handler;
      return Promise.resolve(() => {});
    }),
    pickDirectory: vi.fn(() => Promise.resolve(null)),
  };
  return { bridge, invokes, handlers };
}

function makeStore(bridge: Bridge): AgentsStore {
  const injector = Injector.create({ providers: [{ provide: BRIDGE, useValue: bridge }] });
  return runInInjectionContext(injector, () => new AgentsStore());
}

describe("AgentsStore session continuation", () => {
  it("start threads the resume flag into the AgentStart invoke payload", () => {
    const { bridge, invokes } = fakeBridge();
    const store = makeStore(bridge);

    store.start("a1", 30, 120, true);
    const call = invokes.find((c) => c.command === Commands.AgentStart);
    expect(call?.payload).toEqual({ id: "a1", rows: 30, cols: 120, resume: true });
  });

  it("start defaults resume to false", () => {
    const { bridge, invokes } = fakeBridge();
    const store = makeStore(bridge);

    store.start("a1");
    const call = invokes.find((c) => c.command === Commands.AgentStart);
    expect(call?.payload).toEqual({ id: "a1", rows: 0, cols: 0, resume: false });
  });
});

describe("AgentsStore interest subscription (A0.2 / A1.2)", () => {
  it("subscribe ships the {entries} payload of runtime_subscribe", () => {
    const { bridge, invokes } = fakeBridge();
    const store = makeStore(bridge);

    void store.subscribe([
      { id: "a1", mode: "stream" },
      { id: "a2", mode: "digest" },
    ]);
    const call = invokes.find((c) => c.command === Commands.RuntimeSubscribe);
    expect(call?.payload).toEqual({
      entries: [
        { id: "a1", mode: "stream" },
        { id: "a2", mode: "digest" },
      ],
    });
  });

  it("snapshot invokes runtime_snapshot with the agent id", () => {
    const { bridge, invokes } = fakeBridge();
    const store = makeStore(bridge);

    void store.snapshot("a1");
    const call = invokes.find((c) => c.command === Commands.RuntimeSnapshot);
    expect(call?.payload).toEqual({ id: "a1" });
  });
});

describe("AgentsStore multiplexed output", () => {
  // The backend mux emits ONE agent://output event per ~16ms frame whose
  // payload is an ARRAY of per-agent entries (not the old single {id, chunk}
  // object) — the subscriber must receive the whole frame.
  it("onOutput delivers the per-agent entries array of one frame", async () => {
    const { bridge, handlers } = fakeBridge();
    const store = makeStore(bridge);

    const frames: AgentOutputEntry[][] = [];
    await store.onOutput((entries) => frames.push(entries));

    handlers[Events.AgentOutput]([
      { id: "a1", chunk: "hello", seq: 5 },
      { id: "a2", chunk: "é", seq: 2 },
    ]);
    handlers[Events.AgentOutput]([{ id: "a1", chunk: " world", seq: 11 }]);

    expect(frames).toEqual([
      [
        { id: "a1", chunk: "hello", seq: 5 },
        { id: "a2", chunk: "é", seq: 2 },
      ],
      [{ id: "a1", chunk: " world", seq: 11 }],
    ]);
  });
});

describe("AgentsStore spawn placeholders (instant sidebar feedback)", () => {
  const placeholder = {
    id: "ph-1", projectId: "p1", tool: "claude", model: "m", name: "new-agent", task: "",
    status: "idle", branch: "", worktree: "", base: "main", commits: 0, elapsed: 0, progress: 0,
    pending: [], transition: "creating",
  } as const;

  it("spawn threads the client-generated id into the AgentSpawn payload", () => {
    const { bridge, invokes } = fakeBridge();
    const store = makeStore(bridge);
    void store.spawn({ id: "ph-1", projectId: "p1", tool: "claude", model: "m", effort: null, name: "n", task: "", base: "main" });
    const call = invokes.find((c) => c.command === Commands.AgentSpawn);
    expect((call?.payload as { req: { id: string } }).req.id).toBe("ph-1");
  });

  it("placeholders show in pendingAgents but never in all()", () => {
    const { bridge } = fakeBridge();
    const store = makeStore(bridge);
    store.addPlaceholder({ ...placeholder });
    expect(store.pendingAgents().map((a) => a.id)).toEqual(["ph-1"]);
    expect(store.all()).toEqual([]);
    store.dropPlaceholder("ph-1");
    expect(store.pendingAgents()).toEqual([]);
  });
});
