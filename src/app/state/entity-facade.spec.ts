import { describe, expect, it } from "vitest";
import { Bridge } from "../data-source/bridge";
import { bindFacade } from "./entity-facade";
import { createEntityStore } from "./entity-store";

interface P {
  id: string;
  name: string;
}

// A controllable fake of the Tauri bridge: stub invoke responses + fire events on demand.
class FakeBridge implements Bridge {
  invokeImpl: (command: string, payload?: Record<string, unknown>) => unknown = () => undefined;
  private handlers: Record<string, Array<(p: unknown) => void>> = {};

  async invoke<R>(command: string, payload?: Record<string, unknown>): Promise<R> {
    return this.invokeImpl(command, payload) as R;
  }
  async on<T>(event: string, handler: (p: T) => void): Promise<() => void> {
    (this.handlers[event] ||= []).push(handler as (p: unknown) => void);
    return () => {
      this.handlers[event] = (this.handlers[event] || []).filter((h) => h !== handler);
    };
  }
  async pickDirectory(): Promise<string | null> {
    return null;
  }
  async pickFile(_defaultPath?: string): Promise<string | null> {
    return null;
  }
  emit(event: string, payload: unknown) {
    (this.handlers[event] || []).forEach((h) => h(payload));
  }
}

const cfg = {
  listCommand: "list",
  events: { created: "c://created", updated: "c://updated", deleted: "c://deleted" },
};

describe("entity facade (round-trip data flow)", () => {
  it("load() populates the store from the list command", async () => {
    const store = createEntityStore<P>((p) => p.id);
    const bridge = new FakeBridge();
    bridge.invokeImpl = (cmd) => (cmd === "list" ? [{ id: "a", name: "A" }, { id: "b", name: "B" }] : undefined);
    const facade = bindFacade(store, bridge, cfg);

    await facade.load();
    expect(store.ids()).toEqual(["a", "b"]);
    expect(store.loading()).toBe(false);
  });

  it("a created event upserts into the store (single source of truth)", async () => {
    const store = createEntityStore<P>((p) => p.id);
    const bridge = new FakeBridge();
    const facade = bindFacade(store, bridge, cfg);
    await facade.listen();

    expect(store.all().length).toBe(0);
    bridge.emit("c://created", { id: "x", name: "X" });
    expect(store.all().map((p) => p.id)).toEqual(["x"]);
  });

  it("an updated event patches an existing entity", async () => {
    const store = createEntityStore<P>((p) => p.id);
    const bridge = new FakeBridge();
    const facade = bindFacade(store, bridge, cfg);
    await facade.listen();

    bridge.emit("c://created", { id: "x", name: "X" });
    bridge.emit("c://updated", { id: "x", name: "X2" });
    expect(store.all()[0].name).toBe("X2");
  });

  it("a deleted event removes the entity", async () => {
    const store = createEntityStore<P>((p) => p.id);
    const bridge = new FakeBridge();
    const facade = bindFacade(store, bridge, cfg);
    await facade.listen();

    bridge.emit("c://created", { id: "a", name: "A" });
    bridge.emit("c://created", { id: "b", name: "B" });
    bridge.emit("c://deleted", { id: "a" });
    expect(store.ids()).toEqual(["b"]);
  });

  it("unsubscribe stops further event handling", async () => {
    const store = createEntityStore<P>((p) => p.id);
    const bridge = new FakeBridge();
    const facade = bindFacade(store, bridge, cfg);
    const unsub = await facade.listen();

    bridge.emit("c://created", { id: "a", name: "A" });
    unsub();
    bridge.emit("c://created", { id: "b", name: "B" });
    expect(store.ids()).toEqual(["a"]);
  });

  it("works without an 'updated' event configured", async () => {
    const store = createEntityStore<P>((p) => p.id);
    const bridge = new FakeBridge();
    const facade = bindFacade(store, bridge, {
      listCommand: "list",
      events: { created: "c://created", deleted: "c://deleted" },
    });
    await facade.listen();
    bridge.emit("c://created", { id: "a", name: "A" });
    expect(store.ids()).toEqual(["a"]);
  });
});
