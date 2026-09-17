import { Injector, runInInjectionContext } from "@angular/core";
import { describe, expect, it, vi } from "vitest";
import { Bridge, BRIDGE } from "../data-source/bridge";
import { ExtPack, ExtProgressPayload, ExtRegistryView } from "../models";
import { SettingsStore } from "../settings/settings.store";
import { UiStore } from "../ui/ui.store";
import { ExtensionsStore } from "./extensions.store";

/** A bridge that answers `ext_registry_list` from `view` and records every
 *  `on` handler so a test can push `ext://status` / `ext://progress`. */
class FakeBridge implements Bridge {
  readonly handlers: Record<string, Array<(p: unknown) => void>> = {};
  readonly invoke = vi.fn(async (cmd: string) => {
    if (cmd === "ext_registry_list") {
      if (this.fail) throw new Error("registry unreachable");
      return this.view;
    }
    return undefined;
  }) as unknown as Bridge["invoke"];
  fail = false;
  constructor(public view: ExtRegistryView) {}
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
    return this.picked;
  }
  picked: string | null = null;
}

function pack(over: Partial<ExtPack> & { id: string }): ExtPack {
  return {
    kind: "grammar",
    name: over.id,
    description: "",
    version: "1.0.0",
    installedVersion: null,
    languages: ["x"],
    sizeBytes: 1024 * 1024,
    requires: [],
    bundledSizeBytes: 0,
    installed: false,
    enabled: false,
    compatible: true,
    availableForTarget: true,
    state: "available",
    error: null,
    detection: null,
    ...over,
  };
}

const VIEW: ExtRegistryView = {
  registryUrl: "https://example.test/index.json",
  fetchedAt: 1_000,
  offline: false,
  items: [
    pack({ id: "rust", installed: true, enabled: true, installedVersion: "0.23.4", version: "0.24.1", state: "installed" }),
    pack({ id: "ts", installed: true, enabled: true, installedVersion: "0.21.0", version: "0.21.0", state: "installed" }),
    pack({ id: "cpp", state: "downloading" }),
    pack({ id: "jdtls", kind: "server", installed: true, enabled: true, installedVersion: "1.38.0", version: "1.38.0", state: "installed", detection: { status: "found", path: "/opt/jdtls", hint: null } }),
    // self-contained packs: a runtime and the servers that ride on it
    pack({ id: "runtime.node", kind: "runtime", name: "Node runtime 22", languages: [], installed: true, enabled: true, installedVersion: "22.4.0", version: "22.4.0", state: "installed" }),
    pack({ id: "pyright", kind: "server", name: "Pyright", requires: ["runtime.node"], installed: true, enabled: true, installedVersion: "1.1.400", version: "1.1.400", state: "installed", detection: { status: "bundled", path: "C:/packs/pyright/langserver.js", hint: null } }),
    pack({ id: "tsserver", kind: "server", name: "TypeScript language server", requires: ["runtime.node"], sizeBytes: 12 * 1024 * 1024, bundledSizeBytes: 58 * 1024 * 1024 }),
  ],
};

function make(view: ExtRegistryView = VIEW, opts: { fail?: boolean } = {}) {
  const bridge = new FakeBridge(view);
  bridge.fail = !!opts.fail;
  const flash = vi.fn();
  const relaunch = vi.fn(async () => {});
  const injector = Injector.create({
    providers: [
      { provide: BRIDGE, useValue: bridge },
      { provide: UiStore, useValue: { flash } },
      { provide: SettingsStore, useValue: { relaunch } },
    ],
  });
  const store = runInInjectionContext(injector, () => new ExtensionsStore());
  return { store, bridge, flash, relaunch };
}

const tick = () => new Promise((r) => setTimeout(r, 0));

describe("ExtensionsStore seed + feeds", () => {
  it("seeds from the cached registry list and splits grammars / servers", async () => {
    const { store, bridge } = make();
    await tick();
    expect(bridge.invoke).toHaveBeenCalledWith("ext_registry_list", { refresh: false });
    expect(store.packs()).toHaveLength(7);
    expect(store.grammars().map((p) => p.id)).toEqual(["rust", "ts", "cpp"]);
    expect(store.servers().map((p) => p.id)).toEqual(["jdtls", "pyright", "tsserver"]);
    expect(store.runtimes().map((p) => p.id)).toEqual(["runtime.node"]);
    expect(store.registryState()).toBe("ok");
    expect(store.error()).toBeNull();
  });

  it("a failed seed reports offline; a later status clears it", async () => {
    const { store, bridge } = make(VIEW, { fail: true });
    await tick();
    expect(store.view()).toBeNull();
    expect(store.registryState()).toBe("offline");
    bridge.emit("ext://status", VIEW);
    expect(store.registryState()).toBe("ok");
    expect(store.packs()).toHaveLength(7);
  });

  it("ext://status REPLACES the view (no merging)", async () => {
    const { store, bridge } = make();
    await tick();
    bridge.emit("ext://status", { ...VIEW, items: [pack({ id: "only" })] });
    expect(store.packs().map((p) => p.id)).toEqual(["only"]);
  });

  it("keys progress by pack id and drops it once a status says the download ended", async () => {
    const { store, bridge } = make();
    await tick();
    const p1: ExtProgressPayload = { id: "cpp", downloaded: 10, total: 40, phase: "download", dependency: null, stepIndex: 1, stepCount: 1 };
    const p2: ExtProgressPayload = { id: "cpp", downloaded: 20, total: 40, phase: "download", dependency: null, stepIndex: 1, stepCount: 1 };
    bridge.emit("ext://progress", p1);
    bridge.emit("ext://progress", p2);
    expect(store.progress()["cpp"]).toEqual(p2);
    // still downloading per the next status → kept
    bridge.emit("ext://status", VIEW);
    expect(store.progress()["cpp"]).toEqual(p2);
    // installed now → cleared
    bridge.emit("ext://status", {
      ...VIEW,
      items: VIEW.items.map((p) => (p.id === "cpp" ? { ...p, state: "installed" as const, installed: true } : p)),
    });
    expect(store.progress()["cpp"]).toBeUndefined();
  });

  it("a dependency download streams under the REQUESTED server's id and survives while the server downloads", async () => {
    const { store, bridge } = make();
    await tick();
    const downloading = { ...VIEW, items: VIEW.items.map((p) => (p.id === "tsserver" ? { ...p, state: "downloading" as const } : p)) };
    bridge.emit("ext://status", downloading);
    const dep: ExtProgressPayload = { id: "tsserver", downloaded: 5, total: 32, phase: "download", dependency: "runtime.node", stepIndex: 1, stepCount: 2 };
    bridge.emit("ext://progress", dep);
    expect(store.progress()["tsserver"]).toEqual(dep);
    // the dependency's row is not `downloading` in this status → its copy is dropped on the next snapshot
    bridge.emit("ext://status", downloading);
    expect(store.progress()["tsserver"]?.dependency).toBe("runtime.node");
    expect(store.progress()["runtime.node"]).toBeUndefined();
    // the server lands → its progress goes with it
    bridge.emit("ext://status", VIEW);
    expect(store.progress()["tsserver"]).toBeUndefined();
  });

  it("a dependency's ticks also fill the dependency's OWN row while the backend marks it downloading", async () => {
    const { store, bridge } = make();
    await tick();
    const both = {
      ...VIEW,
      items: [
        ...VIEW.items.map((p) => (p.id === "tsserver" ? { ...p, state: "downloading" as const } : p)),
        pack({ id: "runtime.node", kind: "runtime", state: "downloading" }),
      ],
    };
    bridge.emit("ext://status", both);
    const dep: ExtProgressPayload = { id: "tsserver", downloaded: 5, total: 32, phase: "download", dependency: "runtime.node", stepIndex: 1, stepCount: 2 };
    bridge.emit("ext://progress", dep);
    // the runtime row reads as a plain single-step download of its own
    expect(store.progress()["runtime.node"]).toEqual({ ...dep, id: "runtime.node", dependency: null, stepIndex: 1, stepCount: 1 });
    expect(store.progress()["tsserver"]).toEqual(dep);
    // the runtime lands (installed) while the server keeps downloading → only the runtime's copy goes
    bridge.emit("ext://status", {
      ...both,
      items: both.items.map((p) => (p.id === "runtime.node" ? { ...p, state: "installed" as const, installed: true } : p)),
    });
    expect(store.progress()["runtime.node"]).toBeUndefined();
    expect(store.progress()["tsserver"]).toEqual(dep);
  });

  it("connect() drops the earlier subscriptions and re-seeds", async () => {
    const { store, bridge } = make();
    await tick();
    expect(bridge.handlers["ext://status"]).toHaveLength(1);
    store.connect();
    await tick();
    expect(bridge.handlers["ext://status"]).toHaveLength(1);
    expect(bridge.invoke).toHaveBeenCalledTimes(2);
  });
});

describe("ExtensionsStore computed", () => {
  it("updates = installed packs whose registry version moved; dot follows updates or a pending restart", async () => {
    const { store, bridge } = make();
    await tick();
    expect(store.updates().map((p) => p.id)).toEqual(["rust"]);
    expect(store.updateCount()).toBe(1);
    expect(store.dot()).toBe(true);
    bridge.emit("ext://status", { ...VIEW, items: [pack({ id: "ts", installed: true, installedVersion: "1.0.0" })] });
    expect(store.updateCount()).toBe(0);
    expect(store.dot()).toBe(false);
    bridge.emit("ext://status", { ...VIEW, items: [pack({ id: "go", installed: true, installedVersion: "1.0.0", state: "pendingRestart" })] });
    expect(store.restartPending()).toBe(true);
    expect(store.dot()).toBe(true);
  });

  it("updates include runtime packs", async () => {
    const { store, bridge } = make();
    await tick();
    bridge.emit("ext://status", { ...VIEW, items: [pack({ id: "runtime.java", kind: "runtime", installed: true, installedVersion: "21.0.1", version: "21.0.3" })] });
    expect(store.updates().map((p) => p.id)).toEqual(["runtime.java"]);
    expect(store.dot()).toBe(true);
  });

  it("requiredBy names the INSTALLED packs that list the runtime; nameOf resolves an id", async () => {
    const { store, bridge } = make();
    await tick();
    // tsserver requires it too but is not installed → not a lock
    expect(store.requiredBy("runtime.node")).toEqual(["Pyright"]);
    expect(store.requiredBy("runtime.java")).toEqual([]);
    expect(store.nameOf("runtime.node")).toBe("Node runtime 22");
    expect(store.nameOf("nope")).toBe("nope");
    bridge.emit("ext://status", { ...VIEW, items: VIEW.items.map((p) => (p.id === "pyright" ? { ...p, installed: false } : p)) });
    expect(store.requiredBy("runtime.node")).toEqual([]);
  });

  it("registry offline flag from the backend shows in the footer state", async () => {
    const { store } = make({ ...VIEW, offline: true });
    await tick();
    expect(store.registryState()).toBe("offline");
  });
});

describe("ExtensionsStore actions", () => {
  it("install / uninstall / setEnabled / setPath invoke the ext_* commands", async () => {
    const { store, bridge } = make();
    await tick();
    await store.install("kotlin");
    expect(bridge.invoke).toHaveBeenCalledWith("ext_install", { id: "kotlin" });
    await store.uninstall("ts");
    expect(bridge.invoke).toHaveBeenCalledWith("ext_uninstall", { id: "ts" });
    await store.setEnabled("ts", false);
    expect(bridge.invoke).toHaveBeenCalledWith("ext_set_enabled", { id: "ts", enabled: false });
    await store.setPath("jdtls", "/usr/local/bin/jdtls");
    expect(bridge.invoke).toHaveBeenCalledWith("ext_set_path", { id: "jdtls", path: "/usr/local/bin/jdtls" });
    expect(store.busy().size).toBe(0);
  });

  it("locate: picker → ext_set_path; a cancelled picker sends nothing", async () => {
    const { store, bridge } = make();
    await tick();
    await store.locate("jdtls");
    expect(bridge.invoke).not.toHaveBeenCalledWith("ext_set_path", expect.anything());
    bridge.picked = "C:/tools/jdtls.exe";
    await store.locate("jdtls");
    expect(bridge.invoke).toHaveBeenCalledWith("ext_set_path", { id: "jdtls", path: "C:/tools/jdtls.exe" });
  });

  it("uninstalling a runtime a server still needs surfaces the backend's reason", async () => {
    const { store, bridge, flash } = make();
    await tick();
    (bridge.invoke as unknown as ReturnType<typeof vi.fn>).mockImplementationOnce(async () => {
      throw new Error("required by server.pyright");
    });
    await store.uninstall("runtime.node");
    expect(bridge.invoke).toHaveBeenCalledWith("ext_uninstall", { id: "runtime.node" });
    expect(flash).toHaveBeenCalledWith("uninstall failed: required by server.pyright");
    expect(store.busy().has("runtime.node")).toBe(false);
  });

  it("a rejected command flashes and clears busy", async () => {
    const { store, bridge, flash } = make();
    await tick();
    (bridge.invoke as unknown as ReturnType<typeof vi.fn>).mockImplementationOnce(async () => {
      throw new Error("sha256 mismatch");
    });
    await store.install("zig");
    expect(flash).toHaveBeenCalledWith("install failed: sha256 mismatch");
    expect(store.busy().has("zig")).toBe(false);
  });

  it("refresh() re-lists with refresh:true and flags refreshing meanwhile", async () => {
    const { store, bridge } = make();
    await tick();
    const p = store.refresh();
    expect(store.registryState()).toBe("refreshing");
    await p;
    expect(bridge.invoke).toHaveBeenCalledWith("ext_registry_list", { refresh: true });
    expect(store.registryState()).toBe("ok");
  });

  it("restartNow delegates to the settings relaunch path", async () => {
    const { store, relaunch } = make();
    await tick();
    await store.restartNow();
    expect(relaunch).toHaveBeenCalled();
  });

  it("openModal picks the section; closeModal clears the flag", () => {
    const { store } = make();
    store.openModal("updates");
    expect(store.open()).toBe(true);
    expect(store.section()).toBe("updates");
    store.closeModal();
    expect(store.open()).toBe(false);
  });
});
