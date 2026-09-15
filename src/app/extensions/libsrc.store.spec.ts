import { Injector, runInInjectionContext } from "@angular/core";
import { describe, expect, it, vi } from "vitest";
import { Bridge, BRIDGE } from "../data-source/bridge";
import { LibSource, LibSrcStatus } from "../models";
import { UiStore } from "../ui/ui.store";
import { LibSrcStore } from "./libsrc.store";

/** A bridge whose `libsrc_sources` answer is settled by the test (so the
 *  subscribe-then-seed ordering can be exercised), every other command
 *  resolving or rejecting as configured. */
class FakeBridge implements Bridge {
  readonly handlers: Record<string, Array<(p: unknown) => void>> = {};
  readonly calls: Array<[string, unknown]> = [];
  sources: LibSource[] = [];
  /** Deferred seed: the test resolves it with `releaseSeed()`. */
  private seedResolve: ((v: LibSource[]) => void) | null = null;
  deferSeed = false;
  failing = new Set<string>();

  invoke<R>(cmd: string, payload?: Record<string, unknown>): Promise<R> {
    this.calls.push([cmd, payload]);
    if (this.failing.has(cmd)) return Promise.reject(new Error(`${cmd} failed`));
    if (cmd === "libsrc_sources") {
      if (this.deferSeed) return new Promise<LibSource[]>((r) => (this.seedResolve = r)) as Promise<R>;
      return Promise.resolve(this.sources as unknown as R);
    }
    return Promise.resolve(null as R);
  }
  releaseSeed(): void {
    this.seedResolve?.(this.sources);
    this.seedResolve = null;
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

const MB = 1024 * 1024;
const src = (over: Partial<LibSource> & { id: string }): LibSource => ({
  kind: "jdk",
  path: "C:/jdk-21/lib/src.zip",
  label: over.id,
  state: "idle",
  files: 0,
  decls: 0,
  indexedAt: null,
  sizeBytes: 0,
  projectId: null,
  projectName: null,
  artifacts: 1,
  missing: 0,
  skipped: 0,
  error: null,
  ...over,
});
const status = (over: Partial<LibSrcStatus> & { sourceId: string }): LibSrcStatus => ({
  label: over.sourceId,
  kind: "jdk",
  state: "indexing",
  done: 0,
  total: 0,
  decls: 0,
  ...over,
});

const SOURCES: LibSource[] = [
  src({ id: "jdk1", label: "JDK 21", state: "done", files: 23010, decls: 201_340, sizeBytes: 25 * MB, indexedAt: 1 }),
  src({ id: "m2", kind: "maven", label: "Maven · shop", path: "C:/w/shop/pom.xml", state: "indexing", projectId: "p1", projectName: "shop", artifacts: 12, missing: 3 }),
  src({ id: "cargo", kind: "cargo", label: "Cargo · orrery", path: "C:/w/orrery/src-tauri/Cargo.lock", projectId: "p2", projectName: "orrery", artifacts: 647, skipped: 2 }),
];

function make(sources: LibSource[] = SOURCES, deferSeed = false, failing: string[] = []) {
  const bridge = new FakeBridge();
  bridge.sources = sources;
  bridge.deferSeed = deferSeed;
  for (const cmd of failing) bridge.failing.add(cmd);
  const flash = vi.fn();
  const injector = Injector.create({
    providers: [
      { provide: BRIDGE, useValue: bridge },
      { provide: UiStore, useValue: { flash } },
    ],
  });
  const store = runInInjectionContext(injector, () => new LibSrcStore());
  return { store, bridge, flash };
}

const tick = () => new Promise((r) => setTimeout(r, 0));
const byId = (store: LibSrcStore, id: string) => store.sources().find((s) => s.id === id);

describe("LibSrcStore seeding", () => {
  it("subscribes to libsrc://status, then seeds from libsrc_sources", async () => {
    const { store, bridge } = make();
    // the subscription is registered before the seed answers
    expect(bridge.handlers["libsrc://status"]).toHaveLength(1);
    expect(bridge.calls[0][0]).toBe("libsrc_sources");
    await tick();
    expect(store.seeded()).toBe(true);
    expect(store.sources().map((s) => s.id)).toEqual(["jdk1", "m2", "cargo"]);
    expect(store.indexing().map((s) => s.id)).toEqual(["m2"]);
    expect(store.any()).toBe(true);
    expect(store.error()).toBeNull();
  });

  it("a status that lands before the seed is folded into the seed row (and shows a stub meanwhile)", async () => {
    const { store, bridge } = make(SOURCES, true);
    bridge.emit("libsrc://status", status({ sourceId: "m2", label: "Maven", kind: "maven", done: 2341, total: 23010, decls: 900 }));
    // a stub row keeps the footer chip honest before the list arrives
    expect(store.indexing().map((s) => s.label)).toEqual(["Maven"]);
    expect([store.done(), store.total()]).toEqual([2341, 23010]);
    bridge.releaseSeed();
    await tick();
    expect(store.sources().map((s) => s.id)).toEqual(["jdk1", "m2", "cargo"]);
    const m2 = byId(store, "m2")!;
    expect(m2.path).toBe("C:/w/shop/pom.xml"); // the seed's row…
    expect([m2.done, m2.total, m2.decls]).toEqual([2341, 23010, 900]); // …with the early progress
  });

  it("a rejected seed leaves the list empty and records the error", async () => {
    const { store } = make(SOURCES, false, ["libsrc_sources"]);
    await tick();
    expect(store.seeded()).toBe(true);
    expect(store.sources()).toEqual([]);
    expect(store.error()).toContain("libsrc_sources failed");
    expect(store.any()).toBe(false);
  });

  it("connect() is re-entrant: the old subscription is dropped first", async () => {
    const { store, bridge } = make();
    await tick();
    store.connect();
    await tick();
    expect(bridge.handlers["libsrc://status"]).toHaveLength(1);
    bridge.emit("libsrc://status", { sourceId: "" } as LibSrcStatus); // junk is ignored
    expect(store.sources()).toHaveLength(3);
  });

  it("refresh() replaces the list but keeps the progress of a source still indexing", async () => {
    const { store, bridge } = make();
    await tick();
    bridge.emit("libsrc://status", status({ sourceId: "m2", done: 5000, total: 23010 }));
    bridge.sources = [SOURCES[1]];
    await store.refresh();
    expect(store.sources().map((s) => s.id)).toEqual(["m2"]);
    expect([byId(store, "m2")!.done, byId(store, "m2")!.total]).toEqual([5000, 23010]);
  });
});

describe("LibSrcStore progress merge", () => {
  it("merges state / done / total / decls into the matching source only", async () => {
    const { store, bridge } = make();
    await tick();
    bridge.emit("libsrc://status", status({ sourceId: "m2", done: 2341, total: 23010, decls: 12_000 }));
    const m2 = byId(store, "m2")!;
    expect([m2.state, m2.done, m2.total, m2.decls]).toEqual(["indexing", 2341, 23010, 12_000]);
    expect(byId(store, "jdk1")!.state).toBe("done");
    expect([store.done(), store.total()]).toEqual([2341, 23010]);

    bridge.emit("libsrc://status", status({ sourceId: "m2", state: "done", done: 23010, total: 23010, decls: 180_000 }));
    const done = byId(store, "m2")!;
    expect(done.state).toBe("done");
    expect(done.files).toBe(23010);
    expect(done.decls).toBe(180_000);
    expect(done.indexedAt).not.toBeNull();
    expect(store.any()).toBe(false);
  });

  it("error and cancelled statuses land on the row; an unknown finished source is ignored", async () => {
    const { store, bridge } = make();
    await tick();
    bridge.emit("libsrc://status", status({ sourceId: "m2", state: "error", error: "permission denied" }));
    expect(byId(store, "m2")!.state).toBe("error");
    expect(byId(store, "m2")!.error).toBe("permission denied");
    bridge.emit("libsrc://status", status({ sourceId: "cargo", state: "indexing", done: 1, total: 10 }));
    bridge.emit("libsrc://status", status({ sourceId: "cargo", state: "cancelled", done: 4, total: 10 }));
    expect(byId(store, "cargo")!.state).toBe("cancelled");
    bridge.emit("libsrc://status", status({ sourceId: "ghost", state: "done" }));
    expect(store.sources()).toHaveLength(3);
    // …but an unknown INDEXING source is listed so the chip can show it
    bridge.emit("libsrc://status", status({ sourceId: "late", label: "Gradle", kind: "maven", done: 1, total: 2 }));
    expect(byId(store, "late")).toMatchObject({ label: "Gradle", kind: "maven", state: "indexing", path: "", projectId: null, artifacts: 0 });
  });
});

describe("LibSrcStore actions", () => {
  it("reindex flips the row to indexing at once and invokes libsrc_reindex", async () => {
    const { store, bridge } = make();
    await tick();
    const p = store.reindex("jdk1");
    expect(byId(store, "jdk1")).toMatchObject({ state: "indexing", done: 0, total: 0, error: null });
    expect(store.busy().has("jdk1")).toBe(true);
    await p;
    expect(bridge.calls).toContainEqual(["libsrc_reindex", { sourceId: "jdk1" }]);
    expect(store.busy().has("jdk1")).toBe(false);
  });

  it("cancel invokes libsrc_cancel; remove drops the row once the backend agreed", async () => {
    const { store, bridge } = make();
    await tick();
    await store.cancel("m2");
    expect(bridge.calls).toContainEqual(["libsrc_cancel", { sourceId: "m2" }]);
    await store.remove("cargo");
    expect(bridge.calls).toContainEqual(["libsrc_remove", { sourceId: "cargo" }]);
    expect(store.sources().map((s) => s.id)).toEqual(["jdk1", "m2"]);
  });

  it("a failed command flashes and keeps the row; a busy row ignores a second click", async () => {
    const { store, bridge, flash } = make();
    await tick();
    bridge.failing.add("libsrc_remove");
    await store.remove("cargo");
    expect(flash).toHaveBeenCalledWith("remove failed: libsrc_remove failed");
    expect(store.sources()).toHaveLength(3);
    const a = store.cancel("m2");
    const b = store.cancel("m2");
    await Promise.all([a, b]);
    expect(bridge.calls.filter(([c]) => c === "libsrc_cancel")).toHaveLength(1);
  });

  it("rescan() invokes libsrc_rescan and seeds from its answer; busy under '*' meanwhile; a failure flashes", async () => {
    const { store, bridge, flash } = make();
    await tick();
    bridge.invoke = ((cmd: string, payload?: Record<string, unknown>) => {
      bridge.calls.push([cmd, payload]);
      if (bridge.failing.has(cmd)) return Promise.reject(new Error(`${cmd} failed`));
      if (cmd === "libsrc_rescan") return Promise.resolve([SOURCES[0], src({ id: "cargo2", kind: "cargo", label: "Cargo · new", projectId: "p3", projectName: "new", artifacts: 3 })]);
      return Promise.resolve(null);
    }) as FakeBridge["invoke"];
    const p = store.rescan();
    expect(store.busy().has("*")).toBe(true);
    await p;
    expect(bridge.calls).toContainEqual(["libsrc_rescan", {}]);
    expect(store.sources().map((s) => s.id)).toEqual(["jdk1", "cargo2"]);
    expect(store.busy().has("*")).toBe(false);
    bridge.failing.add("libsrc_rescan");
    await store.rescan();
    expect(flash).toHaveBeenCalledWith("re-scan failed: libsrc_rescan failed");
    expect(store.sources()).toHaveLength(2);
  });

  it("ensure(rootId) asks once per root and swallows a backend without the command", async () => {
    const { store, bridge, flash } = make();
    await tick();
    bridge.failing.add("libsrc_ensure");
    store.ensure("root-a");
    store.ensure("root-a");
    await tick();
    expect(bridge.calls.filter(([c]) => c === "libsrc_ensure")).toEqual([["libsrc_ensure", { id: "root-a" }]]);
    expect(flash).not.toHaveBeenCalled();
    // the failure un-marks the root so a later editor open retries
    bridge.failing.delete("libsrc_ensure");
    store.ensure("root-a");
    await tick();
    expect(bridge.calls.filter(([c]) => c === "libsrc_ensure")).toHaveLength(2);
    store.ensure("root-a");
    expect(bridge.calls.filter(([c]) => c === "libsrc_ensure")).toHaveLength(2);
  });
});
