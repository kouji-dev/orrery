import { Component, provideZonelessChangeDetection } from "@angular/core";
import { ComponentFixture, TestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { Bridge, BRIDGE } from "../data-source/bridge";
import { SetRowComponent } from "../modals/settings-modal.component";
import { ExtPack, ExtRegistryView } from "../models";
import { SettingsStore } from "../settings/settings.store";
import { IconComponent } from "../shared/icon.component";
import { WorkspaceStore } from "../stores/workspace.store";
import { UiStore } from "../ui/ui.store";
import { ExtensionsModalComponent } from "./extensions-modal.component";
import { ExtensionsStore } from "./extensions.store";

beforeAll(() => {
  try {
    TestBed.initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by another spec in this worker
  }
});

afterEach(() => TestBed.resetTestingModule());

// app-icon uses signal inputs, which raw vitest JIT cannot wire (NG0950) —
// a same-selector stub keeps the modal's own template fully exercised.
@Component({ selector: "app-icon", template: "", inputs: ["name", "size", "px", "color"] })
class IconStub {}

const MB = 1024 * 1024;

function pack(over: Partial<ExtPack> & { id: string }): ExtPack {
  return {
    kind: "grammar",
    name: over.id,
    description: "",
    version: "1.0.0",
    installedVersion: null,
    languages: ["x"],
    sizeBytes: 2.1 * 1024 * 1024,
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

// every row state the design shows (art 02 / 03)
const VIEW: ExtRegistryView = {
  registryUrl: "https://example.test/index.json",
  fetchedAt: Date.now() - 120_000,
  offline: false,
  items: [
    pack({ id: "rust", name: "Rust grammar", installed: true, enabled: true, installedVersion: "0.23.4", version: "0.24.1", state: "installed" }),
    pack({ id: "ts", name: "TypeScript & TSX", languages: ["ts", "tsx", "js", "jsx"], installed: true, enabled: true, installedVersion: "0.21.0", version: "0.21.0", state: "installed" }),
    pack({ id: "java", name: "Java grammar", installed: true, enabled: false, installedVersion: "0.19.2", version: "0.19.2", state: "installed" }),
    pack({ id: "cpp", name: "C / C++ bundle", languages: ["c", "cpp", "h", "hpp", "cc", "cmake"], sizeBytes: 40 * 1024 * 1024, state: "downloading" }),
    pack({ id: "kotlin", name: "Kotlin grammar", state: "available" }),
    pack({ id: "go", name: "Go grammar", installed: true, enabled: true, installedVersion: "0.18.0", version: "0.18.0", state: "pendingRestart" }),
    pack({ id: "zig", name: "Zig grammar", state: "error", error: "sha256 mismatch — archive does not match the registry manifest" }),
    pack({ id: "hs", name: "Haskell grammar", state: "incompatible", compatible: false, minAppVersion: "0.24" }),
    pack({ id: "jdtls", kind: "server", name: "Eclipse JDT Language Server", installed: true, enabled: true, installedVersion: "1.38.0", version: "1.38.0", state: "installed", detection: { status: "configured", path: "/opt/jdtls/bin/jdtls", hint: "needs JDK 17+ — JAVA_HOME not set" } }),
    pack({ id: "tsls", kind: "server", name: "typescript-language-server", installed: true, enabled: true, installedVersion: "4.4.0", version: "4.4.0", state: "installed", detection: { status: "missing", path: null, hint: null } }),
    pack({ id: "clangd", kind: "server", name: "clangd", sizeBytes: 38 * 1024 * 1024, state: "available" }),
    // self-contained packs: a runtime, a server bundled on it, one still to install, one found on PATH only
    pack({ id: "runtime.node", kind: "runtime", name: "Node runtime 22", languages: [], sizeBytes: 46 * 1024 * 1024, installed: true, enabled: true, installedVersion: "22.4.0", version: "22.4.0", state: "installed" }),
    pack({ id: "pyright", kind: "server", name: "Pyright", languages: ["py"], requires: ["runtime.node"], installed: true, enabled: true, installedVersion: "1.1.400", version: "1.1.400", state: "installed", detection: { status: "bundled", path: "C:/packs/pyright/langserver.js", hint: null } }),
    pack({ id: "tsserver", kind: "server", name: "TypeScript language server", languages: ["ts", "tsx"], requires: ["runtime.node"], sizeBytes: 12 * 1024 * 1024, bundledSizeBytes: 58 * 1024 * 1024, state: "available" }),
    pack({ id: "gopls", kind: "server", name: "gopls", languages: ["go"], sizeBytes: 20 * 1024 * 1024, state: "available", detection: { status: "found", path: "/usr/local/bin/gopls", hint: null } }),
  ],
};

interface Setup {
  fixture: ComponentFixture<ExtensionsModalComponent>;
  el: HTMLElement;
  store: ExtensionsStore;
  settings: SettingsStore;
  invoke: ReturnType<typeof vi.fn>;
  emit: (event: string, payload: unknown) => void;
}

async function setup(view: ExtRegistryView = VIEW): Promise<Setup> {
  const handlers: Record<string, Array<(p: unknown) => void>> = {};
  const invoke = vi.fn(async (cmd: string) => {
    if (cmd === "ext_registry_list") return view;
    if (cmd === "libsrc_sources") return [];
    if (cmd === "settings_get") return {};
    return null;
  });
  const bridge: Bridge = {
    invoke: invoke as unknown as Bridge["invoke"],
    on: async (event, handler) => {
      (handlers[event] ||= []).push(handler as (p: unknown) => void);
      return () => {};
    },
    pickDirectory: async () => null,
    pickFile: async () => null,
  };
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      { provide: BRIDGE, useValue: bridge },
      { provide: UiStore, useValue: { flash: vi.fn() } },
      { provide: WorkspaceStore, useValue: { setUpdateResume: vi.fn(), flush: () => Promise.resolve(), ready: () => Promise.resolve() } },
    ],
  });
  TestBed.overrideComponent(ExtensionsModalComponent, {
    remove: { imports: [IconComponent] },
    add: { imports: [IconStub] },
  });
  TestBed.overrideComponent(SetRowComponent, {
    remove: { imports: [IconComponent] },
    add: { imports: [IconStub] },
  });
  const settings = TestBed.inject(SettingsStore);
  await settings.ready();
  const store = TestBed.inject(ExtensionsStore);
  await new Promise((r) => setTimeout(r, 0)); // settle the registry seed
  store.openModal();
  const fixture = TestBed.createComponent(ExtensionsModalComponent);
  fixture.detectChanges();
  const emit = (event: string, payload: unknown) => {
    for (const h of handlers[event] ?? []) h(payload);
    fixture.detectChanges();
  };
  return { fixture, el: fixture.nativeElement as HTMLElement, store, settings, invoke, emit };
}

const click = (fixture: ComponentFixture<unknown>, target: Element | null) => {
  (target as HTMLElement).click();
  fixture.detectChanges();
};
const navTo = (s: Setup, label: string) =>
  click(s.fixture, Array.from(s.el.querySelectorAll(".set-nav-item")).find((b) => b.textContent?.includes(label)) ?? null);
const row = (s: Setup, id: string) => s.el.querySelector<HTMLElement>(`.ext-row[data-ext-id="${id}"]`);
const srow = (s: Setup, id: string) => s.el.querySelector<HTMLElement>(`.ext-row[data-src-id="${id}"]`);
const btn = (scope: Element | null, label: string) =>
  Array.from(scope?.querySelectorAll(".ext-act .kj-button") ?? []).find((b) => b.textContent?.trim() === label) ?? null;

describe("ExtensionsModal sections render", () => {
  it("opens on Grammars: three nav rows, updates count badge, installed + available groups", async () => {
    const s = await setup();
    expect(s.el.querySelectorAll(".set-nav-item")).toHaveLength(3);
    expect(s.el.querySelector(".set-head .ht")?.textContent).toContain("Grammars");
    expect(s.el.querySelector(".ext-count")?.textContent?.trim()).toBe("1"); // rust 0.23.4 → 0.24.1
    const heads = Array.from(s.el.querySelectorAll(".set-grp-h")).map((h) => h.textContent?.trim());
    expect(heads).toEqual(["Installed · 6", "Available"]);
    // server packs never show up under Grammars
    expect(row(s, "jdtls")).toBeNull();
  });

  it("renders every pack state the design shows", async () => {
    const s = await setup();
    expect(row(s, "rust")?.dataset["state"]).toBe("update");
    expect(row(s, "rust")?.querySelector(".ext-up")?.textContent).toContain("0.23.4 → 0.24.1");
    expect(row(s, "rust")?.textContent).toContain("Update");
    expect(row(s, "ts")?.dataset["state"]).toBe("installed");
    expect(row(s, "ts")?.querySelector(".set-vchip")?.textContent).toContain("v0.21.0");
    expect(row(s, "ts")?.querySelector(".kj-toggle")?.getAttribute("aria-pressed")).toBe("true");
    expect(row(s, "ts")?.textContent).toContain("Uninstall");
    expect(row(s, "java")?.classList.contains("dim")).toBe(true); // installed + disabled
    expect(row(s, "cpp")?.dataset["state"]).toBe("downloading");
    expect(row(s, "cpp")?.querySelector(".ext-bar .kj-progress-bar")).not.toBeNull();
    expect(row(s, "cpp")?.querySelector(".ext-fig")?.textContent).toContain("starting…");
    expect(row(s, "cpp")?.textContent).not.toContain("Cancel");
    expect(row(s, "kotlin")?.dataset["state"]).toBe("available");
    expect(row(s, "kotlin")?.textContent).toContain("Install");
    expect(row(s, "go")?.dataset["state"]).toBe("pendingRestart");
    expect(row(s, "go")?.querySelector(".ext-badge")?.textContent).toContain("restart to activate");
    expect(row(s, "go")?.textContent).toContain("Restart now");
    expect(row(s, "zig")?.classList.contains("err")).toBe(true);
    expect(row(s, "zig")?.querySelector(".ext-line.err")?.textContent).toContain("sha256 mismatch");
    expect(row(s, "zig")?.textContent).toContain("Retry");
    expect(row(s, "hs")?.classList.contains("dim")).toBe(true);
    expect(row(s, "hs")?.textContent).toContain("needs Orrery ≥ 0.24");
    expect(row(s, "hs")?.textContent).not.toContain("Install");
    // language chips cap at four, then "+n"
    expect(Array.from(row(s, "cpp")!.querySelectorAll(".ext-lang")).map((c) => c.textContent)).toEqual(["c", "cpp", "h", "hpp", "+2"]);
    expect(row(s, "cpp")?.querySelector(".ext-meta .mono")?.textContent).toBe("40 MB");
    expect(row(s, "rust")?.querySelector(".ext-meta .mono")?.textContent).toBe("2.1 MB");
  });

  it("progress ticks fill the bar with the design's figure", async () => {
    const s = await setup();
    s.emit("ext://progress", { id: "cpp", downloaded: 12.3 * 1024 * 1024, total: 40 * 1024 * 1024, phase: "download" });
    expect(row(s, "cpp")?.querySelector(".ext-fig")?.textContent).toBe("12.3 / 40 MB");
    expect(row(s, "cpp")?.querySelector("[role=progressbar]")?.getAttribute("aria-valuenow")).toBe("31");
    s.emit("ext://progress", { id: "cpp", downloaded: 40 * 1024 * 1024, total: 40 * 1024 * 1024, phase: "verify" });
    expect(row(s, "cpp")?.querySelector(".ext-fig")?.textContent).toBe("verifying…");
  });

  it("Language servers: idle-shutdown row, detection lines, Locate…, enabled toggle", async () => {
    const s = await setup();
    // system copies count only with the toggle on (the design's found/configured lines)
    s.settings.set({ lspUseSystemServers: true });
    navTo(s, "Language servers");
    expect(s.el.querySelector(".set-head .ht")?.textContent).toContain("Language servers");
    expect(s.el.textContent).toContain("Idle shutdown");
    expect(s.el.querySelector(".set-num-idle input")).not.toBeNull();
    const heads = Array.from(s.el.querySelectorAll(".set-grp-h")).map((h) => h.textContent?.trim());
    expect(heads).toEqual(["Lifecycle", "Enabled · 3", "Available", "Runtimes"]);
    const jdtls = row(s, "jdtls")!;
    expect(jdtls.querySelector(".ext-line .lb")?.textContent).toContain("configured ·");
    expect(jdtls.querySelector(".ext-line .mono")?.textContent).toBe("/opt/jdtls/bin/jdtls");
    expect(jdtls.querySelector(".ext-line.warn")?.textContent).toContain("needs JDK 17+");
    expect(jdtls.querySelector(".kj-toggle")).not.toBeNull();
    const tsls = row(s, "tsls")!;
    expect(tsls.querySelector(".ext-line.warn")?.textContent).toContain("not found — install or");
    expect(tsls.querySelectorAll(".ext-line .kj-button").length).toBeGreaterThanOrEqual(2); // inline + right Locate…
    expect(row(s, "clangd")?.textContent).toContain("Install");
    expect(row(s, "clangd")?.querySelector(".ext-line:not(.ext-bundle)")).toBeNull(); // no detection until installed
    // grammars never show up under servers
    expect(row(s, "rust")).toBeNull();
  });

  it("no Library sources section (M4.1: automatic and invisible); Updates lists the one update", async () => {
    const s = await setup();
    expect(Array.from(s.el.querySelectorAll(".set-nav-item .lb")).map((n) => n.textContent?.trim())).toEqual(["Grammars", "Language servers", "Updates"]);
    expect(s.el.textContent).not.toContain("Library sources");
    expect(s.el.querySelectorAll(".ext-count")).toHaveLength(1); // updates only
    navTo(s, "Updates");
    expect(s.el.querySelector(".set-grp-h")?.textContent?.trim()).toBe("1 update available");
    expect(row(s, "rust")).not.toBeNull();
    expect(row(s, "ts")).toBeNull();
  });

  it("empty registry: grammar + server empty states, Updates says up to date", async () => {
    const s = await setup({ ...VIEW, items: [] });
    expect(s.el.querySelector(".ext-empty .h")?.textContent).toContain("No grammar packs installed");
    expect(s.el.querySelector(".ext-count")).toBeNull();
    navTo(s, "Language servers");
    expect(s.el.querySelector(".ext-empty .h")?.textContent).toContain("No language servers enabled");
    navTo(s, "Updates");
    expect(s.el.querySelector(".ext-empty .h")?.textContent).toContain("Everything is up to date");
  });

  it("footer: registry age + Refresh; offline shows Retry", async () => {
    // fetchedAt is taken NOW, not at module load: a slow full-suite run
    // otherwise drifts the rounded age from 2 to 3 minutes.
    const s = await setup({ ...VIEW, fetchedAt: Date.now() - 100_000 });
    expect(s.el.querySelector(".set-foot .fl")?.textContent).toContain("registry · updated 2 min ago");
    expect(s.el.querySelector(".set-foot .reset-all")?.textContent).toContain("Refresh");
    s.emit("ext://status", { ...VIEW, offline: true });
    expect(s.el.querySelector(".set-foot .ext-off")?.textContent).toContain("registry offline");
    expect(s.el.querySelector(".set-foot .reset-all")?.textContent).toContain("Retry");
  });
});

describe("ExtensionsModal · self-contained server packs (runtimes ride along)", () => {
  // the gaps between the pieces are CSS `gap`, not text — normalise around the dots
  const flat = (el: Element | null | undefined) => el?.textContent?.replace(/\s+/g, " ").replace(/\s*·\s*/g, " · ").trim();

  it("a server still to install says what the bundle includes and prices the Install button", async () => {
    const s = await setup();
    navTo(s, "Language servers");
    const ts = row(s, "tsserver")!;
    expect(ts.dataset["state"]).toBe("available");
    expect(flat(ts.querySelector(".ext-bundle"))).toBe("bundled · includes Node runtime · 58 MB");
    expect(btn(ts, "Install · 58 MB")).not.toBeNull();
    // a server that ships alone: just its size
    expect(flat(row(s, "clangd")?.querySelector(".ext-bundle"))).toBe("bundled · 38 MB");
    expect(btn(row(s, "clangd"), "Install · 38 MB")).not.toBeNull();
    // grammars keep the plain label
    navTo(s, "Grammars");
    expect(btn(row(s, "kotlin"), "Install")).not.toBeNull();
  });

  it("an installed bundled server shows its version with the launch path on hover; no Locate…", async () => {
    const s = await setup();
    navTo(s, "Language servers");
    const py = row(s, "pyright")!;
    expect(py.dataset["state"]).toBe("installed");
    expect(py.querySelector(".ext-bundle")).toBeNull();
    const line = py.querySelector(".ext-line")!;
    expect(flat(line)).toBe("bundled · v1.1.400");
    expect(line.querySelector(".mono")?.getAttribute("title")).toBe("C:/packs/pyright/langserver.js");
    expect(py.textContent).not.toContain("Locate…");
    expect(py.querySelector(".kj-toggle")).not.toBeNull();
  });

  it("a system copy is ignored until 'Use system servers' is on; the toggle writes the setting", async () => {
    const s = await setup();
    navTo(s, "Language servers");
    const tgl = s.el.querySelector<HTMLElement>('.kj-toggle[aria-label="Use system servers"]')!;
    expect(tgl).not.toBeNull();
    expect(tgl.getAttribute("aria-pressed")).toBe("false");
    expect(s.el.textContent).toContain("Fall back to servers found on PATH");
    const go = row(s, "gopls")!;
    expect(go.querySelector(".ext-line.mute")?.textContent).toContain("system copy ignored — enable ‘Use system servers’");
    expect(go.querySelector(".ext-line.mute")?.getAttribute("title")).toBe("/usr/local/bin/gopls");
    expect(Array.from(go.querySelectorAll(".ext-line .lb")).map((l) => l.textContent?.trim())).toEqual(["bundled ·"]);
    // jdtls's manually configured path is an explicit choice: shown (and
    // honoured by the backend) even while the toggle is off
    expect(row(s, "jdtls")?.querySelector(".ext-line.mute")).toBeNull();
    expect(Array.from(row(s, "jdtls")!.querySelectorAll(".ext-line .lb")).map((l) => l.textContent?.trim())).toContain("configured ·");
    click(s.fixture, tgl);
    expect(s.settings.settings().lspUseSystemServers).toBe(true);
    expect(go.querySelector(".ext-line.mute")).toBeNull();
    expect(Array.from(go.querySelectorAll(".ext-line .lb")).map((l) => l.textContent?.trim())).toEqual(["bundled ·", "found ·"]);
    expect(go.querySelector(".ext-line .mono:not(.tnum)")?.textContent).toBe("/usr/local/bin/gopls");
    expect(go.textContent).toContain("Locate…");
    expect(row(s, "jdtls")?.querySelector(".ext-line .lb")?.textContent).toContain("configured ·");
  });

  it("Runtimes group: auto-managed row, no Enabled toggle, Uninstall locked with the reason", async () => {
    const s = await setup();
    navTo(s, "Language servers");
    const grp = s.el.querySelector('[data-testid="ext-runtimes"]')!;
    expect(grp.querySelector(".set-grp-h")?.textContent?.trim()).toBe("Runtimes");
    const node = grp.querySelector<HTMLElement>('.ext-row[data-ext-id="runtime.node"]')!;
    expect(node.dataset["state"]).toBe("installed");
    expect(node.querySelector(".set-vchip")?.textContent).toContain("v22.4.0");
    expect(flat(node.querySelector(".ext-meta"))).toBe("auto-managed · used by Pyright · 46 MB");
    expect(node.querySelector(".kj-toggle")).toBeNull();
    const lock = node.querySelector(".ext-lock")!;
    expect(lock.getAttribute("title")).toBe("required by Pyright");
    expect(lock.querySelector(".kj-button")?.getAttribute("aria-disabled")).toBe("true");
    expect(node.querySelector("kj-confirm-popup")).toBeNull();
    // nav foot counts it as a pack (6 grammars + 1 runtime)
    expect(s.el.querySelector(".set-nav-foot")?.textContent).toContain("7 packs");
    // the last server needing it goes → Uninstall unlocks (confirm popup back)
    s.emit("ext://status", { ...VIEW, items: VIEW.items.map((p) => (p.id === "pyright" ? { ...p, installed: false, state: "available" as const, detection: null } : p)) });
    expect(node.querySelector(".ext-lock")).toBeNull();
    expect(flat(node.querySelector(".ext-meta"))).toBe("auto-managed · 46 MB");
    expect(btn(node, "Uninstall")).not.toBeNull();
    click(s.fixture, btn(node, "Uninstall"));
    const confirm = Array.from(document.querySelectorAll(".ext-confirm")).find((c) => c.textContent?.includes("Uninstall Node runtime 22?"));
    expect(confirm?.textContent).toContain("No installed server needs this runtime");
    // a runtime not yet on disk offers a plain Install
    s.emit("ext://status", { ...VIEW, items: VIEW.items.map((p) => (p.id === "runtime.node" ? { ...p, installed: false, installedVersion: null, state: "available" as const } : p)) });
    expect(btn(node, "Install")).not.toBeNull();
    expect(s.el.querySelector(".set-nav-foot")?.textContent).toContain("6 packs");
  });

  it("a multi-step install names the step in the bar; Updates lists a runtime update", async () => {
    const s = await setup();
    navTo(s, "Language servers");
    s.emit("ext://status", { ...VIEW, items: VIEW.items.map((p) => (p.id === "tsserver" ? { ...p, state: "downloading" as const } : p)) });
    const ts = row(s, "tsserver")!;
    expect(ts.dataset["state"]).toBe("downloading");
    expect(ts.querySelector(".ext-bundle")).toBeNull();
    expect(ts.querySelector(".ext-fig")?.textContent).toBe("starting…");
    s.emit("ext://progress", { id: "tsserver", dependency: "runtime.node", stepIndex: 1, stepCount: 2, downloaded: 12.3 * MB, total: 32 * MB, phase: "download" });
    expect(ts.querySelector(".ext-fig")?.textContent).toBe("1/2 · Node runtime 22 · 12.3 / 32 MB");
    expect(ts.querySelector("[role=progressbar]")?.getAttribute("aria-valuenow")).toBe("38");
    s.emit("ext://progress", { id: "tsserver", dependency: null, stepIndex: 2, stepCount: 2, downloaded: 3.1 * MB, total: 12 * MB, phase: "download" });
    expect(ts.querySelector(".ext-fig")?.textContent).toBe("2/2 · TypeScript language server · 3.1 / 12 MB");
    s.emit("ext://progress", { id: "tsserver", dependency: null, stepIndex: 2, stepCount: 2, downloaded: 12 * MB, total: 12 * MB, phase: "unpack" });
    expect(ts.querySelector(".ext-fig")?.textContent).toBe("2/2 · TypeScript language server · unpacking…");
    // a runtime update shows under Updates with the version arrow
    s.emit("ext://status", { ...VIEW, items: VIEW.items.map((p) => (p.id === "runtime.node" ? { ...p, version: "22.6.0" } : p)) });
    expect(s.el.querySelector(".ext-count")?.textContent?.trim()).toBe("2");
    navTo(s, "Updates");
    expect(s.el.querySelector(".set-grp-h")?.textContent?.trim()).toBe("2 updates available");
    const node = row(s, "runtime.node")!;
    expect(node.querySelector(".ext-up")?.textContent).toContain("22.4.0 → 22.6.0");
    expect(btn(node, "Update")).not.toBeNull();
    expect(node.querySelector(".kj-toggle")).toBeNull();
  });
});

describe("ExtensionsModal actions reach the store", () => {
  it("Install invokes ext_install for that pack", async () => {
    const s = await setup();
    const btn = Array.from(row(s, "kotlin")!.querySelectorAll(".ext-act .kj-button")).find((b) => b.textContent?.includes("Install"));
    click(s.fixture, btn ?? null);
    expect(s.invoke).toHaveBeenCalledWith("ext_install", { id: "kotlin" });
  });

  it("the Enabled toggle invokes ext_set_enabled with the new value", async () => {
    const s = await setup();
    click(s.fixture, row(s, "ts")!.querySelector(".kj-toggle"));
    expect(s.invoke).toHaveBeenCalledWith("ext_set_enabled", { id: "ts", enabled: false });
  });

  it("idle shutdown writes settings.lspIdleMinutes, clamped to 1..120", async () => {
    const s = await setup();
    navTo(s, "Language servers");
    s.fixture.componentInstance.setIdle(25);
    expect(s.settings.settings().lspIdleMinutes).toBe(25);
    s.fixture.componentInstance.setIdle(500);
    expect(s.settings.settings().lspIdleMinutes).toBe(120);
  });

  it("nav switch updates the store section; Done closes it", async () => {
    const s = await setup();
    navTo(s, "Updates");
    expect(s.store.section()).toBe("updates");
    click(s.fixture, Array.from(s.el.querySelectorAll(".set-foot .kj-button")).find((b) => b.textContent?.includes("Done")) ?? null);
    expect(s.store.open()).toBe(false);
  });
});
