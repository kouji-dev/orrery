import { Component, provideZonelessChangeDetection } from "@angular/core";
import { ComponentFixture, TestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { Bridge, BRIDGE } from "../data-source/bridge";
import { ExtensionsStore } from "../extensions/extensions.store";
import { LspServer, LspStatus } from "../models";
import { IconComponent } from "../shared/icon.component";
import { UiStore } from "../ui/ui.store";
import { LspChipComponent, liveOf, shortLabel, uptimeOf } from "./lsp-chip.component";
import { LspStatusStore } from "./lsp-status.store";

beforeAll(() => {
  try {
    TestBed.initTestEnvironment(BrowserTestingModule, platformBrowserTesting());
  } catch {
    // already initialized by another spec in this worker
  }
});

afterEach(() => TestBed.resetTestingModule());

// app-icon uses signal inputs, which raw vitest JIT cannot wire (NG0950)
@Component({ selector: "app-icon", template: "", inputs: ["name", "size", "px", "color", "cls"] })
class IconStub {}

const MB = 1024 * 1024;
function server(over: Partial<LspServer> & { id: string }): LspServer {
  const [extId, projectId] = over.id.split(":");
  return {
    extId,
    label: extId.replace(/^server\./, ""),
    language: "java",
    root: "C:/p",
    projectId,
    projectName: projectId,
    pid: 100,
    state: "ready",
    memBytes: 812 * MB,
    cpu: 2.5,
    restarts: 0,
    startedAt: Date.now() - 65_000,
    lastError: null,
    ...over,
  };
}

interface Setup {
  fixture: ComponentFixture<LspChipComponent>;
  el: HTMLElement;
  store: LspStatusStore;
  invoke: ReturnType<typeof vi.fn>;
  emit: (status: LspStatus) => void;
  openModal: ReturnType<typeof vi.fn>;
}

async function setup(initial: LspStatus = { servers: [] }): Promise<Setup> {
  const handlers: Array<(p: unknown) => void> = [];
  const invoke = vi.fn(async (cmd: string) => (cmd === "lsp_status" ? initial : null));
  const bridge: Bridge = {
    invoke: invoke as unknown as Bridge["invoke"],
    on: async (event, handler) => {
      if (event === "lsp://status") handlers.push(handler as (p: unknown) => void);
      return () => {};
    },
    pickDirectory: async () => null,
    pickFile: async () => null,
  };
  const openModal = vi.fn();
  TestBed.configureTestingModule({
    providers: [
      provideZonelessChangeDetection(),
      { provide: BRIDGE, useValue: bridge },
      { provide: UiStore, useValue: { flash: vi.fn() } },
      { provide: ExtensionsStore, useValue: { openModal, languagesOfServer: () => [], servers: () => [] } },
    ],
  });
  TestBed.overrideComponent(LspChipComponent, {
    remove: { imports: [IconComponent] },
    add: { imports: [IconStub] },
  });
  const store = TestBed.inject(LspStatusStore);
  await new Promise((r) => setTimeout(r, 0)); // settle the seed
  const fixture = TestBed.createComponent(LspChipComponent);
  fixture.detectChanges();
  const emit = (status: LspStatus) => {
    for (const h of handlers) h(status);
    fixture.detectChanges();
  };
  return { fixture, el: fixture.nativeElement as HTMLElement, store, invoke, emit, openModal };
}

const chip = (s: Setup) => s.el.querySelector<HTMLElement>("[data-testid=lsp-chip]");
const text = (el: Element | null) => (el?.textContent ?? "").replace(/\s+/g, " ").trim();

describe("LspChip", () => {
  it("stays visible as 'no servers' while none runs — the marker is permanent", async () => {
    const s = await setup();
    // a marker that vanished would hide the states worth seeing (a crash, a
    // server that never came up), so it renders in its quietest form instead
    expect(text(chip(s)!)).toBe("no servers");
    expect(chip(s)!.classList.contains("none")).toBe(true);
    // a stopped row does not count as running and stays quiet…
    s.emit({ servers: [server({ id: "server.jdtls:p1", state: "stopped", memBytes: 0 })] });
    expect(text(chip(s)!)).toBe("no servers");
    expect(chip(s)!.classList.contains("none")).toBe(true);
    // …but a server that could not be LAUNCHED is an error to see, not a gap
    s.emit({ servers: [server({ id: "server.gopls:p1", state: "missing", memBytes: 0, lastError: "gopls not found" })] });
    expect(text(chip(s)!)).toBe("no servers");
    expect(chip(s)!.classList.contains("error")).toBe(true);
    expect(chip(s)!.classList.contains("none")).toBe(false);
  });

  it("one instance → 'servers' with the server icon; the name and memory stay in the tooltip / popover", async () => {
    const s = await setup({ servers: [server({ id: "server.jdtls:p1" })] });
    const c = chip(s)!;
    expect(text(c)).toBe("servers");
    expect(c.classList.contains("running")).toBe(true);
    expect(c.querySelector("kj-spinner")).toBeNull();
    expect(c.title).toBe("jdtls · p1");
  });

  it("three instances across two projects → still just 'servers'; the tooltip lists them", async () => {
    const s = await setup({
      servers: [
        server({ id: "server.jdtls:p1", memBytes: 1024 * MB }),
        server({ id: "server.jdtls:p2", projectName: "two", memBytes: 1024 * MB }),
        server({ id: "server.gopls:p1", language: "go", memBytes: 102.4 * MB }),
      ],
    });
    expect(text(chip(s))).toBe("servers");
    expect(chip(s)!.title.split("\n")).toEqual(["jdtls · p1", "jdtls · two", "gopls · p1"]);
  });

  it("a starting instance swaps the icon for a spinner", async () => {
    const s = await setup({ servers: [server({ id: "server.jdtls:p1", state: "starting", memBytes: 0, startedAt: null })] });
    expect(chip(s)!.classList.contains("starting")).toBe(true);
    expect(chip(s)!.querySelector("kj-spinner")).not.toBeNull();
  });

  it("a crashed instance tints the chip without adding text; all crashed → 'no servers' in the error tint", async () => {
    const s = await setup({
      servers: [server({ id: "server.jdtls:p1" }), server({ id: "server.gopls:p1", language: "go", state: "crashed", memBytes: 0, lastError: "exit 1" })],
    });
    const c = chip(s)!;
    expect(c.classList.contains("error")).toBe(true);
    expect(text(c)).toBe("servers");
    s.emit({ servers: [server({ id: "a:p1", state: "crashed", memBytes: 0 }), server({ id: "b:p1", state: "crashed", memBytes: 0 })] });
    expect(text(chip(s))).toBe("no servers");
    expect(chip(s)!.classList.contains("error")).toBe(true);
  });

  it("dims when every instance idles; falls back to 'no servers' when the list empties", async () => {
    const s = await setup({ servers: [server({ id: "server.jdtls:p1", state: "idle" })] });
    expect(chip(s)!.classList.contains("idle")).toBe(true);
    s.emit({ servers: [] });
    expect(chip(s)!.classList.contains("none")).toBe(true);
    expect(text(chip(s)!)).toBe("no servers");
  });
});

describe("LspChip helpers", () => {
  it("shortLabel: known packs get their tool name, long labels fall back to the pack id, short ones stay", () => {
    expect(shortLabel({ extId: "server.typescript-language-server", label: "TypeScript language server" })).toBe("ts-ls");
    expect(shortLabel({ extId: "server.jdtls", label: "Eclipse JDT Language Server" })).toBe("jdtls");
    expect(shortLabel({ extId: "server.clangd", label: "clangd" })).toBe("clangd");
    expect(shortLabel({ extId: "server.clangd", label: "The clangd C/C++ language server" })).toBe("clangd");
  });

  it("the row shows the short name, two-decimal memory and two-decimal cpu; the chip and tooltip do not carry them", async () => {
    const s = await setup({
      servers: [server({ id: "server.typescript-language-server:p1", label: "TypeScript language server", memBytes: 604.6 * MB, cpu: 6.351164 })],
    });
    expect(text(chip(s))).toBe("servers");
    expect(chip(s)!.title).toBe("TypeScript language server · p1");
    const head = s.el.querySelector<HTMLElement>(".lsp-pop-h .tot")!;
    expect(text(head)).toBe("1 · 604.60 MB");
    const row = s.el.querySelector<HTMLElement>(".lsp-row")!;
    expect(text(row.querySelector(".nm"))).toBe("ts-ls");
    expect(text(row.querySelector(".fig"))).toBe("604.60 MB");
    expect(text(row.querySelector(".fig.sub"))).toMatch(/^6\.35% · /);
    expect(row.querySelector<HTMLElement>(".lsp-row-m")!.title).toContain("cpu 6.35%");
  });

  it("liveOf maps backend states to the design's LiveBadge", () => {
    expect(liveOf(server({ id: "a:p", state: "starting" }))).toEqual({ tone: "live", label: "starting", spin: true });
    expect(liveOf(server({ id: "a:p", state: "ready" }))).toEqual({ tone: "live", label: "running", spin: false });
    expect(liveOf(server({ id: "a:p", state: "idle" }))).toEqual({ tone: "idle", label: "idle", spin: false });
    expect(liveOf(server({ id: "a:p", state: "crashed" }))).toEqual({ tone: "err", label: "error", spin: false });
    expect(liveOf(server({ id: "a:p", state: "missing" }))).toEqual({ tone: "err", label: "not found", spin: false });
    expect(liveOf(server({ id: "a:p", state: "stopped" }))).toEqual({ tone: "idle", label: "stopped", spin: false });
  });

  it("uptimeOf counts from startedAt, '—' before the process is up or after a crash", () => {
    expect(uptimeOf(server({ id: "a:p", startedAt: 10_000 }), 75_000)).toBe("1m 5s");
    expect(uptimeOf(server({ id: "a:p", startedAt: null }), 75_000)).toBe("—");
    expect(uptimeOf(server({ id: "a:p", state: "crashed", startedAt: 10_000 }), 75_000)).toBe("—");
  });
});
