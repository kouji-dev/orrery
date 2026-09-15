import { ApplicationRef, provideZonelessChangeDetection, signal } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { Bridge, BRIDGE } from "../data-source/bridge";
import { ExtensionsStore } from "../extensions/extensions.store";
import { AgentsStore } from "../stores/agents.store";
import { LspAutoStartService } from "./lsp-auto-start.service";

type Agent = { id: string; projectId: string; status: string };
type Pack = { id: string; installed: boolean; enabled: boolean; languages: string[] };

describe("LspAutoStartService", () => {
  let invoke: ReturnType<typeof vi.fn>;
  let agents: ReturnType<typeof signal<Agent[]>>;
  let packs: ReturnType<typeof signal<Pack[]>>;
  const tick = () => TestBed.inject(ApplicationRef).tick();
  const pins = () => invoke.mock.calls.filter((c) => c[0] === "lsp_pin_project").map((c) => c[1]);
  const TS = { id: "server.tsls", installed: true, enabled: true, languages: ["typescript"] };

  beforeEach(() => {
    invoke = vi.fn(async () => []);
    agents = signal<Agent[]>([]);
    packs = signal<Pack[]>([TS]);
    const bridge: Bridge = {
      invoke: invoke as unknown as Bridge["invoke"],
      on: async () => () => {},
      pickDirectory: async () => null,
      pickFile: async () => null,
    };
    TestBed.resetTestingModule();
    TestBed.configureTestingModule({
      providers: [
        provideZonelessChangeDetection(),
        { provide: BRIDGE, useValue: bridge },
        { provide: AgentsStore, useValue: { all: agents } },
        { provide: ExtensionsStore, useValue: { servers: packs } },
      ],
    });
    TestBed.inject(LspAutoStartService).start();
    tick();
  });

  afterEach(() => TestBed.resetTestingModule());

  it("pins a project once its first agent runs and unpins it when the last one stops", () => {
    expect(pins()).toEqual([]);
    agents.set([{ id: "a1", projectId: "p1", status: "idle" }]);
    tick();
    expect(pins()).toEqual([]); // idle is not "runs in a project"
    agents.set([{ id: "a1", projectId: "p1", status: "running" }]);
    tick();
    expect(pins()).toEqual([{ id: "p1", pinned: true }]);
    // a second running agent of the same project adds nothing
    agents.set([
      { id: "a1", projectId: "p1", status: "running" },
      { id: "a2", projectId: "p1", status: "running" },
    ]);
    tick();
    expect(pins()).toEqual([{ id: "p1", pinned: true }]);
    // one of them stopping keeps the pin; the last one stopping releases it
    agents.set([
      { id: "a1", projectId: "p1", status: "done" },
      { id: "a2", projectId: "p1", status: "running" },
    ]);
    tick();
    expect(pins()).toEqual([{ id: "p1", pinned: true }]);
    agents.set([
      { id: "a1", projectId: "p1", status: "done" },
      { id: "a2", projectId: "p1", status: "idle" },
    ]);
    tick();
    expect(pins()).toEqual([
      { id: "p1", pinned: true },
      { id: "p1", pinned: false },
    ]);
  });

  it("re-pins the projects at work when the set of enabled server packs changes, and never pins with none", () => {
    packs.set([]);
    agents.set([{ id: "a1", projectId: "p1", status: "running" }]);
    tick();
    expect(pins()).toEqual([]); // no pack could start — no IPC
    packs.set([TS]);
    tick();
    expect(pins()).toEqual([{ id: "p1", pinned: true }]);
    // a pack installed mid-session: the backend re-detects and starts it
    packs.set([TS, { id: "server.pyright", installed: true, enabled: true, languages: ["python"] }]);
    tick();
    expect(pins()).toEqual([
      { id: "p1", pinned: true },
      { id: "p1", pinned: true },
    ]);
    // disabling a pack changes the enabled set → one harmless re-pin (the
    // backend re-detects; a running server is simply re-acquired)
    packs.set([TS, { id: "server.pyright", installed: true, enabled: false, languages: ["python"] }]);
    tick();
    expect(pins()).toHaveLength(3);
    // and disabling every pack releases the project
    packs.set([]);
    tick();
    expect(pins().at(-1)).toEqual({ id: "p1", pinned: false });
  });

  it("pins every project with a running agent, independently", () => {
    agents.set([
      { id: "a1", projectId: "p1", status: "running" },
      { id: "b1", projectId: "p2", status: "running" },
    ]);
    tick();
    expect(pins()).toEqual([
      { id: "p1", pinned: true },
      { id: "p2", pinned: true },
    ]);
    agents.set([
      { id: "a1", projectId: "p1", status: "running" },
      { id: "b1", projectId: "p2", status: "blocked" },
    ]);
    tick();
    expect(pins().at(-1)).toEqual({ id: "p2", pinned: false });
  });
});
