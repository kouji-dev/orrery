import { describe, it, expect, vi } from "vitest";
import { Injector, runInInjectionContext } from "@angular/core";
import { AgentsStore } from "./agents.store";
import { BRIDGE, Bridge, Commands } from "../data-source/bridge";

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
