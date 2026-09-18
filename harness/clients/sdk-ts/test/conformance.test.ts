/**
 * The same fixture files the Rust SDK runs, the same assertions.
 *
 * If this file and `clients/sdk-rs/tests/conformance.rs` ever disagree, one of
 * the five clients is drawing something the others are not — and it will be the
 * one nobody is looking at.
 */

import { describe, expect, it } from "vitest";

import { conformance, SurfaceStore, type Frame } from "../src/index.js";

const scenarios = conformance.loadAll();

describe("the conformance fixtures", () => {
  it("has the whole set, exactly", () => {
    expect(scenarios.length).toBe(conformance.SCENARIO_COUNT);
    for (const scenario of scenarios) {
      expect(
        scenario.steps.some((s) => s.kind === "expect"),
        `${scenario.name} asserts nothing`,
      ).toBe(true);
    }
  });

  for (const scenario of scenarios) {
    it(`${scenario.name} produces the expected state at every checkpoint`, () => {
      for (const { index, actual, expected } of conformance.checkpoints(scenario)) {
        expect(actual, `${scenario.name} checkpoint ${index}`).toEqual(expected);
      }
    });
  }
});

describe("the store", () => {
  const find = (name: string) => scenarios.find((s) => s.name === name)!;

  it("turns text deltas into one markdown surface", () => {
    const store = conformance.replay(find("streaming-markdown"));
    const surface = store.turn("turn-3")!.surfaces.find((s) => s.id === "msg-1")!;
    expect(surface.kind.t).toBe("markdown");
    if (surface.kind.t !== "markdown") throw new Error("unreachable");
    expect(surface.kind.complete).toBe(true);
    expect(surface.kind.value.startsWith("Here is the fix:")).toBe(true);
    expect(surface.kind.value.split("```").length - 1).toBe(2);
    expect(surface.status).toBe("done");
  });

  it("detects a gap by arithmetic", () => {
    const store = conformance.replay(find("seq-gap"));
    expect(store.gaps()).toEqual([{ expected: 4, got: 6 }]);
  });

  it("does not mistake a merged frame for a gap", () => {
    const store = new SurfaceStore();
    store.apply({
      seq: 1,
      type: "TEXT_MESSAGE_START",
      messageId: "m",
      role: "assistant",
    } as Frame);
    const changes = store.apply({
      seq: 9,
      merged_from: 2,
      type: "TEXT_MESSAGE_CONTENT",
      messageId: "m",
      delta: "eight frames in one",
    } as Frame);
    expect(changes.some((c) => c.kind === "gap-detected")).toBe(false);
    expect(store.gaps()).toEqual([]);
  });

  it("ignores unknown events rather than rejecting them", () => {
    // `streaming-markdown` carries an event type this build has never heard of,
    // and `custom-with-fallback` a `Custom` name it has no handler for. Neither
    // may show up as a lost frame.
    expect(conformance.replay(find("streaming-markdown")).gaps()).toEqual([]);
    expect(conformance.replay(find("custom-with-fallback")).gaps()).toEqual([]);
  });

  it("keeps a custom surface's payload and fallback side by side", () => {
    const store = conformance.replay(find("custom-with-fallback"));
    const surface = store.turn("turn-10")!.surfaces.find((s) => s.id === "dag-1")!;
    expect(surface.kind.t).toBe("custom");
    if (surface.kind.t !== "custom") throw new Error("unreachable");
    expect(surface.kind.kind).toBe("buildgraph.dag");
    expect(surface.kind.fallback.kind.t).toBe("text");
  });

  it("closes a cancelled turn's message as cancelled, never as done", () => {
    const store = conformance.replay(find("cancel-midturn"));
    const turn = store.turn("turn-9")!;
    expect(turn.cancelled).toBe(true);
    expect(turn.surfaces[0]!.status).toBe("cancelled");
  });

  it("lands replayed surfaces in the detached turn", () => {
    const store = conformance.replay(find("reattach-since"));
    expect(store.turn("(detached)")).toBeDefined();
    expect(store.gaps()).toEqual([]);
  });
});
