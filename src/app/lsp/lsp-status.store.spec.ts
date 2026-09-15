import { Injector, runInInjectionContext } from "@angular/core";
import { describe, expect, it, vi } from "vitest";
import { Bridge, BRIDGE } from "../data-source/bridge";
import { ExtensionsStore } from "../extensions/extensions.store";
import { LspServer, LspStatus } from "../models";
import { UiStore } from "../ui/ui.store";
import { isLive, isSyncable, LspStatusStore } from "./lsp-status.store";

/** A bridge that answers `lsp_status` (after an optional gate) and records
 *  every `on` handler so a test can push `lsp://status`. */
class FakeBridge implements Bridge {
  readonly handlers: Record<string, Array<(p: unknown) => void>> = {};
  readonly calls: [string, unknown][] = [];
  /** Resolves the pending seed when called (tests race a push against it). */
  release: (() => void) | null = null;
  fail = false;
  constructor(public status: LspStatus, private readonly gate = false) {}
  invoke = vi.fn(async (cmd: string, payload?: Record<string, unknown>) => {
    this.calls.push([cmd, payload]);
    if (cmd === "lsp_status") {
      if (this.fail) throw new Error("no backend");
      if (this.gate) await new Promise<void>((r) => (this.release = r));
      return this.status;
    }
    return null;
  }) as unknown as Bridge["invoke"];
  async on<T>(event: string, handler: (p: T) => void): Promise<() => void> {
    (this.handlers[event] ||= []).push(handler as (p: unknown) => void);
    return () => {
      this.handlers[event] = (this.handlers[event] || []).filter((h) => h !== handler);
    };
  }
  emit(event: string, payload: unknown): void {
    for (const h of this.handlers[event] ?? []) h(payload);
  }
  async pickDirectory(): Promise<string | null> {
    return null;
  }
  async pickFile(): Promise<string | null> {
    return null;
  }
}

export function server(over: Partial<LspServer> & { id: string }): LspServer {
  const [extId, projectId] = over.id.split(":");
  return {
    extId: extId ?? "server.x",
    label: (extId ?? "x").replace(/^server\./, ""),
    language: "java",
    root: "C:/p",
    projectId: projectId ?? "p1",
    projectName: projectId ?? "p1",
    pid: 100,
    state: "ready",
    memBytes: 100 * 1024 * 1024,
    cpu: 1.5,
    restarts: 0,
    startedAt: 1_000,
    lastError: null,
    ...over,
  };
}

const settle = () => new Promise((r) => setTimeout(r, 0));

function make(status: LspStatus = { servers: [] }, opts: { gate?: boolean; fail?: boolean; langs?: Record<string, string[]> } = {}) {
  const bridge = new FakeBridge(status, opts.gate);
  bridge.fail = !!opts.fail;
  const flash = vi.fn();
  const injector = Injector.create({
    providers: [
      { provide: BRIDGE, useValue: bridge },
      { provide: UiStore, useValue: { flash } },
      {
        provide: ExtensionsStore,
        useValue: {
          languagesOfServer: (id: string) => opts.langs?.[id] ?? [],
          servers: () => [{ id: "server.jdtls", name: "Eclipse JDT LS", languages: ["java"] }],
        },
      },
      { provide: LspStatusStore, useClass: LspStatusStore },
    ],
  });
  const store = runInInjectionContext(injector, () => injector.get(LspStatusStore));
  return { store, bridge, flash };
}

describe("LspStatusStore ordering", () => {
  it("subscribes to lsp://status BEFORE seeding from lsp_status", async () => {
    const { store, bridge } = make({ servers: [server({ id: "server.jdtls:p1" })] });
    // the subscription exists as soon as the constructor ran…
    await Promise.resolve();
    expect(bridge.handlers["lsp://status"]?.length).toBe(1);
    await settle();
    // …and the seed landed after it
    expect(bridge.calls.map(([c]) => c)).toEqual(["lsp_status"]);
    expect(store.servers().map((s) => s.id)).toEqual(["server.jdtls:p1"]);
  });

  it("a push that lands while the seed is in flight wins over the seed", async () => {
    const { store, bridge } = make({ servers: [server({ id: "server.jdtls:p1", state: "stopped" })] }, { gate: true });
    await settle();
    bridge.emit("lsp://status", { servers: [server({ id: "server.jdtls:p1", state: "ready" })] });
    bridge.release?.();
    await settle();
    expect(store.servers()[0].state).toBe("ready");
  });

  it("a failing seed leaves the store empty; a later push still fills it", async () => {
    const { store, bridge } = make({ servers: [] }, { fail: true });
    await settle();
    expect(store.servers()).toEqual([]);
    bridge.emit("lsp://status", { servers: [server({ id: "server.gopls:p2" })] });
    expect(store.servers().length).toBe(1);
  });

  it("every push REPLACES the list (no merge) and tolerates a malformed one", async () => {
    const { store, bridge } = make({ servers: [server({ id: "a:p1" }), server({ id: "b:p1" })] });
    await settle();
    bridge.emit("lsp://status", { servers: [server({ id: "b:p1" })] });
    expect(store.servers().map((s) => s.id)).toEqual(["b:p1"]);
    bridge.emit("lsp://status", {});
    expect(store.servers()).toEqual([]);
  });

  it("connect() drops the earlier subscription and re-seeds", async () => {
    const { store, bridge } = make({ servers: [] });
    await settle();
    bridge.status = { servers: [server({ id: "a:p1" })] };
    store.connect();
    await settle();
    expect(bridge.handlers["lsp://status"]?.length).toBe(1);
    expect(store.servers().length).toBe(1);
  });
});

describe("LspStatusStore aggregates", () => {
  const MB = 1024 * 1024;
  const list = [
    server({ id: "server.jdtls:p1", state: "ready", memBytes: 800 * MB }),
    server({ id: "server.jdtls:p2", projectName: "two", state: "starting", memBytes: 0, startedAt: null, pid: null }),
    server({ id: "server.gopls:p1", language: "go", state: "idle", memBytes: 200 * MB }),
    server({ id: "server.rust-analyzer:p1", language: "rust", state: "crashed", memBytes: 0, lastError: "boom" }),
    server({ id: "server.pyright:p1", language: "python", state: "stopped", memBytes: 0 }),
    server({ id: "server.tsls:p1", language: "javascript", state: "missing", memBytes: 0 }),
  ];

  it("running excludes stopped/missing; totals, starting, errors follow", async () => {
    const { store } = make({ servers: list });
    await settle();
    expect(store.running().map((s) => s.id)).toEqual(["server.jdtls:p1", "server.jdtls:p2", "server.gopls:p1", "server.rust-analyzer:p1"]);
    expect(store.any()).toBe(true);
    expect(store.totalMem()).toBe(1000 * MB);
    expect(store.starting().map((s) => s.id)).toEqual(["server.jdtls:p2"]);
    expect(store.errors().map((s) => s.id)).toEqual(["server.rust-analyzer:p1"]);
    expect(store.allIdle()).toBe(false);
    // error beats starting
    expect(store.aggregate()).toBe("error");
  });

  it("aggregate: starting beats running; idle only when all idle; empty = no chip", async () => {
    const { store, bridge } = make({ servers: [] });
    await settle();
    expect(store.any()).toBe(false);
    bridge.emit("lsp://status", { servers: [server({ id: "a:p1", state: "ready" }), server({ id: "b:p1", state: "starting" })] });
    expect(store.aggregate()).toBe("starting");
    bridge.emit("lsp://status", { servers: [server({ id: "a:p1", state: "ready" }), server({ id: "b:p1", state: "idle" })] });
    expect(store.aggregate()).toBe("running");
    bridge.emit("lsp://status", { servers: [server({ id: "a:p1", state: "idle" }), server({ id: "b:p1", state: "idle" })] });
    expect(store.aggregate()).toBe("idle");
    expect(store.allIdle()).toBe(true);
  });

  it("byProject groups live rows in first-seen order with the project name", async () => {
    const { store } = make({ servers: list });
    await settle();
    const g = store.byProject();
    expect(g.map((x) => [x.projectId, x.projectName, x.rows.length])).toEqual([
      ["p1", "p1", 3],
      ["p2", "two", 1],
    ]);
  });

  it("instancesOf / liveFor / labelFor", async () => {
    const { store } = make({ servers: list }, { langs: { "server.tsls": ["javascript", "typescript"], "server.jdtls": ["java"] } });
    await settle();
    expect(store.instancesOf("server.jdtls").map((s) => s.projectId)).toEqual(["p1", "p2"]);
    // starting + ready + idle are syncable; crashed / stopped are not
    expect(store.liveFor("p1", "java")?.id).toBe("server.jdtls:p1");
    expect(store.liveFor("p2", "java")?.id).toBe("server.jdtls:p2");
    expect(store.liveFor("p1", "go")?.id).toBe("server.gopls:p1");
    expect(store.liveFor("p1", "rust")).toBeUndefined();
    expect(store.liveFor("p1", "python")).toBeUndefined();
    expect(store.liveFor("p1", "")).toBeUndefined();
    // the label the NavHint uses: the instance's label, else the pack name
    expect(store.labelFor("p1", "java")).toBe("jdtls");
    expect(store.labelFor("p9", "java")).toBe("jdtls");
    expect(store.labelFor("p1", "kotlin")).toBe("language server");
  });

  it("isLive / isSyncable predicates", () => {
    expect(isLive(server({ id: "a:p", state: "crashed" }))).toBe(true);
    expect(isLive(server({ id: "a:p", state: "stopped" }))).toBe(false);
    expect(isSyncable(server({ id: "a:p", state: "crashed" }))).toBe(false);
    expect(isSyncable(server({ id: "a:p", state: "idle" }))).toBe(true);
  });
});

describe("LspStatusStore actions", () => {
  it("stop / restart send extId + projectId and mark the instance busy meanwhile", async () => {
    const s = server({ id: "server.jdtls:p1" });
    const { store, bridge } = make({ servers: [s] });
    await settle();
    const p = store.stop(s);
    expect(store.busy().has(s.id)).toBe(true);
    await p;
    expect(store.busy().has(s.id)).toBe(false);
    await store.restart(s);
    expect(bridge.calls.slice(1)).toEqual([
      ["lsp_stop", { extId: "server.jdtls", projectId: "p1" }],
      ["lsp_restart", { extId: "server.jdtls", projectId: "p1" }],
    ]);
  });

  it("stopAll sends lsp_stop_all; stopPack stops each instance of one pack", async () => {
    const { store, bridge } = make({ servers: [server({ id: "server.jdtls:p1" }), server({ id: "server.jdtls:p2" }), server({ id: "server.gopls:p1" })] });
    await settle();
    await store.stopAll();
    await store.stopPack("server.jdtls");
    expect(bridge.calls.slice(1)).toEqual([
      ["lsp_stop_all", {}],
      ["lsp_stop", { extId: "server.jdtls", projectId: "p1" }],
      ["lsp_stop", { extId: "server.jdtls", projectId: "p2" }],
    ]);
  });

  it("a rejected action flashes and clears busy", async () => {
    const s = server({ id: "server.jdtls:p1" });
    const { store, bridge, flash } = make({ servers: [s] });
    await settle();
    (bridge.invoke as unknown as ReturnType<typeof vi.fn>).mockRejectedValueOnce(new Error("gone"));
    await store.stop(s);
    expect(flash).toHaveBeenCalledWith("stop failed: gone");
    expect(store.busy().size).toBe(0);
  });
});
