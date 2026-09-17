import { provideZonelessChangeDetection } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { afterEach, describe, expect, it, vi } from "vitest";
import { Bridge, BRIDGE, Commands } from "../data-source/bridge";
import { ModelCatalogService } from "./model-catalog.service";

function setup(invoke: Bridge["invoke"]) {
  const bridge: Bridge = {
    invoke,
    on: async () => () => {},
    pickDirectory: async () => null,
    pickFile: async () => null,
  };
  TestBed.configureTestingModule({
    providers: [provideZonelessChangeDetection(), { provide: BRIDGE, useValue: bridge }],
  });
  return TestBed.inject(ModelCatalogService);
}

afterEach(() => TestBed.resetTestingModule());

describe("ModelCatalogService", () => {
  it("probes the CLI once and caches what it reported", async () => {
    const invoke = vi.fn(async () => ["anthropic/claude-opus-4-5", "openai/gpt-5.1"]);
    const svc = setup(invoke as unknown as Bridge["invoke"]);

    expect(svc.models("pi")).toEqual([]); // nothing until the probe lands
    expect(svc.isProbed("pi")).toBe(false);

    svc.load("pi");
    svc.load("pi"); // de-duped while in flight
    await vi.waitFor(() => expect(svc.isProbed("pi")).toBe(true));

    expect(invoke).toHaveBeenCalledTimes(1);
    expect(invoke).toHaveBeenCalledWith(Commands.ListToolModels, { id: "pi" });
    expect(svc.models("pi")).toEqual(["anthropic/claude-opus-4-5", "openai/gpt-5.1"]);

    svc.load("pi"); // already probed — no second shell-out
    expect(invoke).toHaveBeenCalledTimes(1);
  });

  it("keeps the last known list while a re-probe is in flight (stale-while-revalidate)", async () => {
    let release!: (v: string[]) => void;
    const invoke = vi
      .fn()
      .mockResolvedValueOnce(["composer-2.5", "auto"])
      .mockImplementationOnce(() => new Promise<string[]>((r) => (release = r)));
    const svc = setup(invoke as unknown as Bridge["invoke"]);

    svc.load("cursor");
    await vi.waitFor(() => expect(svc.models("cursor")).toEqual(["composer-2.5", "auto"]));

    // second open → second probe, and the OLD list stays visible meanwhile
    svc.refresh("cursor");
    expect(invoke).toHaveBeenCalledTimes(2);
    expect(svc.isProbed("cursor")).toBe(false); // "asking the CLI…" copy
    expect(svc.models("cursor")).toEqual(["composer-2.5", "auto"]); // no flicker

    release(["composer-3"]);
    await vi.waitFor(() => expect(svc.models("cursor")).toEqual(["composer-3"]));
    expect(svc.isProbed("cursor")).toBe(true);
  });

  it("a failed re-probe leaves the previously known list intact", async () => {
    const invoke = vi
      .fn()
      .mockResolvedValueOnce(["composer-2.5"])
      .mockRejectedValueOnce(new Error("network"));
    const svc = setup(invoke as unknown as Bridge["invoke"]);

    svc.load("cursor");
    await vi.waitFor(() => expect(svc.models("cursor")).toEqual(["composer-2.5"]));
    svc.refresh("cursor");
    await vi.waitFor(() => expect(svc.isProbed("cursor")).toBe(true));
    expect(svc.models("cursor")).toEqual(["composer-2.5"]);
  });

  it("an empty-but-successful probe is recorded as truth (→ curated fallback)", async () => {
    // `cursor-agent models` prints "No models available for this account." and
    // EXITS 0 — the empty list is the honest answer, not a failure.
    const invoke = vi.fn(async () => [] as string[]);
    const svc = setup(invoke as unknown as Bridge["invoke"]);
    svc.load("cursor");
    await vi.waitFor(() => expect(svc.isProbed("cursor")).toBe(true));
    expect(svc.models("cursor")).toEqual([]);
  });

  it("caches an empty list when the probe fails — never invents ids", async () => {
    const invoke = vi.fn(async () => {
      throw new Error("`pi` is not installed");
    });
    const svc = setup(invoke as unknown as Bridge["invoke"]);

    svc.load("pi");
    await vi.waitFor(() => expect(svc.isProbed("pi")).toBe(true));
    expect(svc.models("pi")).toEqual([]);
  });

  it("refresh re-probes a tool whose keys or install just changed", async () => {
    let models = ["openai/gpt-5.1"];
    const invoke = vi.fn(async () => models);
    const svc = setup(invoke as unknown as Bridge["invoke"]);

    svc.load("pi");
    await vi.waitFor(() => expect(svc.models("pi")).toEqual(["openai/gpt-5.1"]));

    models = ["openai/gpt-5.1", "google/gemini-3-pro"];
    svc.refresh("pi");
    await vi.waitFor(() => expect(svc.models("pi")).toHaveLength(2));
    expect(invoke).toHaveBeenCalledTimes(2);
  });
});
