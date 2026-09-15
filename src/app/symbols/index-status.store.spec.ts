import { Injector, runInInjectionContext } from "@angular/core";
import { describe, expect, it } from "vitest";
import { Bridge, BRIDGE } from "../data-source/bridge";
import { IndexStatus } from "../models";
import { IndexStatusStore } from "./index-status.store";

class FakeBridge implements Bridge {
  readonly handlers: Record<string, Array<(p: unknown) => void>> = {};
  invoke<R>(): Promise<R> {
    return Promise.reject(new Error("no backend"));
  }
  async on<T>(event: string, handler: (p: T) => void): Promise<() => void> {
    (this.handlers[event] ||= []).push(handler as (p: unknown) => void);
    return () => {
      this.handlers[event] = (this.handlers[event] || []).filter((h) => h !== handler);
    };
  }
  emit(event: string, payload: unknown): void {
    for (const h of this.handlers[event] ?? []) h(payload);
  }
  pickDirectory(): Promise<string | null> {
    return Promise.resolve(null);
  }
  pickFile(): Promise<string | null> {
    return Promise.resolve(null);
  }
}

const status = (over: Partial<IndexStatus> & { root: string }): IndexStatus => ({
  state: "indexing",
  files: 0,
  done: 0,
  total: 0,
  elapsedMs: 0,
  ...over,
});

function make() {
  const bridge = new FakeBridge();
  const injector = Injector.create({ providers: [{ provide: BRIDGE, useValue: bridge }] });
  const store = runInInjectionContext(injector, () => new IndexStatusStore());
  return { store, bridge };
}

const tick = () => new Promise((r) => setTimeout(r, 0));

describe("IndexStatusStore", () => {
  it("starts empty and hidden — nothing is seeded", async () => {
    const { store, bridge } = make();
    await tick();
    expect(bridge.handlers["symbols://index"]).toHaveLength(1);
    expect(store.visible()).toBe(false);
    expect(store.indexing()).toEqual([]);
  });

  it("tracks progress per root and sums it; ready drops the root", async () => {
    const { store, bridge } = make();
    await tick();
    bridge.emit("symbols://index", status({ root: "a", done: 10, total: 100 }));
    expect(store.visible()).toBe(true);
    expect(store.indexing().map((s) => s.root)).toEqual(["a"]);
    expect([store.done(), store.total()]).toEqual([10, 100]);

    bridge.emit("symbols://index", status({ root: "b", done: 5, total: 50 }));
    expect(store.indexing()).toHaveLength(2);
    expect([store.done(), store.total()]).toEqual([15, 150]);

    bridge.emit("symbols://index", status({ root: "a", done: 60, total: 100 }));
    expect([store.done(), store.total()]).toEqual([65, 150]);

    bridge.emit("symbols://index", status({ root: "a", state: "ready", files: 100, done: 100, total: 100 }));
    expect(store.indexing().map((s) => s.root)).toEqual(["b"]);
    bridge.emit("symbols://index", status({ root: "b", state: "ready" }));
    expect(store.visible()).toBe(false);
  });

  it("keeps an errored root visible until its next run, idle drops it", async () => {
    const { store, bridge } = make();
    await tick();
    bridge.emit("symbols://index", status({ root: "a", state: "error", error: "grammar missing" }));
    expect(store.visible()).toBe(true);
    expect(store.indexing()).toEqual([]);
    expect(store.errored()[0].error).toBe("grammar missing");
    bridge.emit("symbols://index", status({ root: "a", state: "indexing", done: 1, total: 2 }));
    expect(store.errored()).toEqual([]);
    expect(store.indexing()).toHaveLength(1);
    bridge.emit("symbols://index", status({ root: "a", state: "idle" }));
    expect(store.visible()).toBe(false);
  });

  it("connect() is re-entrant: the old subscription is dropped first", async () => {
    const { store, bridge } = make();
    await tick();
    store.connect();
    await tick();
    expect(bridge.handlers["symbols://index"]).toHaveLength(1);
    bridge.emit("symbols://index", status({ root: "a" }));
    expect(store.indexing()).toHaveLength(1);
    bridge.emit("symbols://index", { root: "" } as IndexStatus); // junk is ignored
    expect(store.indexing()).toHaveLength(1);
  });
});
