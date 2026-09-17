import { Component, provideZonelessChangeDetection } from "@angular/core";
import { ComponentFixture, TestBed } from "@angular/core/testing";
import { BrowserTestingModule, platformBrowserTesting } from "@angular/platform-browser/testing";
import { afterEach, beforeAll, describe, expect, it, vi } from "vitest";
import { Bridge, BRIDGE } from "../data-source/bridge";
import { LibSrcStore } from "../extensions/libsrc.store";
import { IndexStatus, LibSource, LibSrcStatus } from "../models";
import { IconComponent } from "../shared/icon.component";
import { UiStore } from "../ui/ui.store";
import { IndexChipComponent } from "./index-chip.component";
import { IndexStatusStore } from "./index-status.store";

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

const root = (over: Partial<IndexStatus> & { root: string }): IndexStatus => ({
  state: "indexing",
  files: 0,
  done: 0,
  total: 0,
  elapsedMs: 0,
  ...over,
});
const src = (over: Partial<LibSource> & { id: string }): LibSource => ({
  kind: "jdk",
  path: "C:/jdk-21/lib/src.zip",
  label: over.id,
  state: "idle",
  files: 0,
  decls: 0,
  indexedAt: null,
  sizeBytes: 0,
  error: null,
  ...over,
});
const lib = (over: Partial<LibSrcStatus> & { sourceId: string }): LibSrcStatus => ({
  label: over.sourceId,
  kind: "jdk",
  state: "indexing",
  done: 0,
  total: 0,
  decls: 0,
  ...over,
});

interface Setup {
  fixture: ComponentFixture<IndexChipComponent>;
  el: HTMLElement;
  emit: (event: string, payload: unknown) => void;
}

async function setup(sources: LibSource[] = []): Promise<Setup> {
  const handlers: Record<string, Array<(p: unknown) => void>> = {};
  const invoke = vi.fn(async (cmd: string) => (cmd === "libsrc_sources" ? sources : null));
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
    providers: [provideZonelessChangeDetection(), { provide: BRIDGE, useValue: bridge }, { provide: UiStore, useValue: { flash: vi.fn() } }],
  });
  TestBed.overrideComponent(IndexChipComponent, {
    remove: { imports: [IconComponent] },
    add: { imports: [IconStub] },
  });
  TestBed.inject(IndexStatusStore);
  TestBed.inject(LibSrcStore);
  await new Promise((r) => setTimeout(r, 0)); // settle the seed
  const fixture = TestBed.createComponent(IndexChipComponent);
  fixture.detectChanges();
  const emit = (event: string, payload: unknown) => {
    for (const h of handlers[event] ?? []) h(payload);
    fixture.detectChanges();
  };
  return { fixture, el: fixture.nativeElement as HTMLElement, emit };
}

const chip = (s: Setup) => s.el.querySelector<HTMLElement>("[data-testid=index-chip]");
const text = (el: Element | null) => (el?.textContent ?? "").replace(/\s+/g, " ").trim();
const meter = (s: Setup) => (chip(s)?.querySelector<HTMLElement>(".meter > i")?.style.width ?? "");

describe("IndexChip", () => {
  it("renders nothing while nothing indexes", async () => {
    const s = await setup([src({ id: "jdk1", label: "JDK 21", state: "done" })]);
    expect(chip(s)).toBeNull();
  });

  it("roots only: the M2 copy, one root with figures, several as a count", async () => {
    const s = await setup();
    s.emit("symbols://index", root({ root: "a", done: 2341, total: 10020 }));
    expect(text(chip(s))).toBe("symbols · indexing 2,341 / 10,020");
    expect(chip(s)!.dataset["mode"]).toBe("symbols");
    expect(chip(s)!.title).toBe("Building the symbol index");
    expect(meter(s)).toBe("23%");
    s.emit("symbols://index", root({ root: "b", done: 0, total: 100 }));
    expect(text(chip(s))).toBe("symbols · indexing 2 roots");
  });

  it("library only: 'library · indexing <label> · done / total', a count past one source", async () => {
    const s = await setup([src({ id: "jdk1", label: "JDK 21", state: "indexing" })]);
    // before the first status the figures are withheld
    expect(text(chip(s))).toBe("library · indexing JDK 21");
    expect(chip(s)!.dataset["mode"]).toBe("library");
    s.emit("libsrc://status", lib({ sourceId: "jdk1", label: "JDK 21", done: 2341, total: 23010 }));
    expect(text(chip(s))).toBe("library · indexing JDK 21 · 2,341 / 23,010");
    expect(chip(s)!.title).toBe("Indexing library source JDK 21 (C:/jdk-21/lib/src.zip)");
    expect(meter(s)).toBe("10%");
    s.emit("libsrc://status", lib({ sourceId: "m2", label: "Maven", kind: "maven", done: 10, total: 100 }));
    expect(text(chip(s))).toBe("library · indexing 2 sources");
    // done drops the source; the last one leaving hides the chip
    s.emit("libsrc://status", lib({ sourceId: "m2", state: "done", done: 100, total: 100 }));
    expect(text(chip(s))).toBe("library · indexing JDK 21 · 2,341 / 23,010");
    s.emit("libsrc://status", lib({ sourceId: "jdk1", state: "done", done: 23010, total: 23010 }));
    expect(chip(s)).toBeNull();
  });

  it("both at once: 'indexing N root(s) · <label | N sources>' over the summed meter", async () => {
    const s = await setup([src({ id: "jdk1", label: "JDK 21", state: "indexing" })]);
    s.emit("libsrc://status", lib({ sourceId: "jdk1", label: "JDK 21", done: 50, total: 100 }));
    s.emit("symbols://index", root({ root: "a", done: 50, total: 100 }));
    expect(text(chip(s))).toBe("indexing 1 root · JDK 21");
    expect(chip(s)!.dataset["mode"]).toBe("both");
    expect(meter(s)).toBe("50%");
    expect(chip(s)!.title.split("\n")).toEqual(["Building the symbol index", "Indexing library source JDK 21 (C:/jdk-21/lib/src.zip)"]);
    s.emit("symbols://index", root({ root: "b", done: 0, total: 100 }));
    s.emit("libsrc://status", lib({ sourceId: "m2", label: "Maven", kind: "maven", done: 0, total: 100 }));
    expect(text(chip(s))).toBe("indexing 2 roots · 2 sources");
    expect(meter(s)).toBe("25%");
  });

  it("a failed root run tints the chip only while nothing else indexes", async () => {
    const s = await setup();
    s.emit("symbols://index", root({ root: "a", state: "error", error: "grammar missing" }));
    expect(text(chip(s))).toBe("symbols · index failed");
    expect(chip(s)!.classList.contains("error")).toBe(true);
    expect(chip(s)!.title).toBe("grammar missing");
    s.emit("libsrc://status", lib({ sourceId: "jdk1", label: "JDK 21", done: 1, total: 2 }));
    expect(chip(s)!.classList.contains("error")).toBe(false);
    expect(text(chip(s))).toBe("library · indexing JDK 21 · 1 / 2");
  });
});
