import { Component, provideZonelessChangeDetection } from "@angular/core";
import { ComponentFixture, TestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { Bridge, BRIDGE } from "../data-source/bridge";
import { ExtensionsStore } from "../extensions/extensions.store";
import { LspServer, LspStatus } from "../models";
import { IconComponent } from "../shared/icon.component";
import { UiStore } from "../ui/ui.store";
import { LspChipComponent, liveOf, uptimeOf } from "./lsp-chip.component";
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
    // stopped / missing rows still do not count as running
    s.emit({ servers: [server({ id: "server.jdtls:p1", state: "stopped" }), server({ id: "server.gopls:p1", state: "missing" })] });
    expect(text(chip(s)!)).toBe("no servers");
    expect(chip(s)!.classList.contains("none")).toBe(true);
  });

  it("one instance → '<label> · <mem>' with the server icon", async () => {
    const s = await setup({ servers: [server({ id: "server.jdtls:p1" })] });
    const c = chip(s)!;
    expect(text(c)).toBe("jdtls · 812.0 MB");
    expect(c.classList.contains("running")).toBe(true);
    expect(c.querySelector("kj-spinner")).toBeNull();
    expect(c.title).toBe("jdtls · p1");
  });

  it("three instances across two projects → '3 servers · <total>'", async () => {
    const s = await setup({
      servers: [
        server({ id: "server.jdtls:p1", memBytes: 1024 * MB }),
        server({ id: "server.jdtls:p2", projectName: "two", memBytes: 1024 * MB }),
        server({ id: "server.gopls:p1", language: "go", memBytes: 102.4 * MB }),
      ],
    });
    expect(text(chip(s))).toBe("3 servers · 2.1 GB");
    expect(chip(s)!.title.split("\n")).toEqual(["jdtls · p1", "jdtls · two", "gopls · p1"]);
  });

  it("a starting instance swaps the icon for a spinner", async () => {
    const s = await setup({ servers: [server({ id: "server.jdtls:p1", state: "starting", memBytes: 0, startedAt: null })] });
    expect(chip(s)!.classList.contains("starting")).toBe(true);
    expect(chip(s)!.querySelector("kj-spinner")).not.toBeNull();
  });

  it("a crashed instance tints the chip and appends the error count", async () => {
    const s = await setup({
      servers: [server({ id: "server.jdtls:p1" }), server({ id: "server.gopls:p1", language: "go", state: "crashed", memBytes: 0, lastError: "exit 1" })],
    });
    const c = chip(s)!;
    expect(c.classList.contains("error")).toBe(true);
    expect(Array.from(c.querySelectorAll(".mono")).map(text)).toEqual(["2 servers · 812.0 MB", "· 1 error"]);
    s.emit({ servers: [server({ id: "a:p1", state: "crashed", memBytes: 0 }), server({ id: "b:p1", state: "crashed", memBytes: 0 })] });
    expect(text(chip(s))).toContain("2 errors");
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
  it("liveOf maps backend states to the design's LiveBadge", () => {
    expect(liveOf(server({ id: "a:p", state: "starting" }))).toEqual({ tone: "live", label: "starting", spin: true });
    expect(liveOf(server({ id: "a:p", state: "ready" }))).toEqual({ tone: "live", label: "running", spin: false });
    expect(liveOf(server({ id: "a:p", state: "idle" }))).toEqual({ tone: "idle", label: "idle", spin: false });
    expect(liveOf(server({ id: "a:p", state: "crashed" }))).toEqual({ tone: "err", label: "error", spin: false });
  });

  it("uptimeOf counts from startedAt, '—' before the process is up or after a crash", () => {
    expect(uptimeOf(server({ id: "a:p", startedAt: 10_000 }), 75_000)).toBe("1m 5s");
    expect(uptimeOf(server({ id: "a:p", startedAt: null }), 75_000)).toBe("—");
    expect(uptimeOf(server({ id: "a:p", state: "crashed", startedAt: 10_000 }), 75_000)).toBe("—");
  });
});
