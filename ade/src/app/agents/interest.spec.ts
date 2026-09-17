import { describe, expect, it } from "vitest";
import { deriveInterest, paneLeafViews, VisibleSurfaces } from "./interest";
import { group, leaf } from "../workspace/pane-model";

const base: VisibleSurfaces = {
  paneAgents: [],
  overviewAgentIds: [],
  windowVisible: true,
};

describe("deriveInterest — A0.2 interest-set derivation", () => {
  it("visible terminal panes are stream", () => {
    const entries = deriveInterest({
      ...base,
      paneAgents: [{ agentId: "a", view: "terminal" }],
    });
    expect(entries).toEqual([{ id: "a", mode: "stream" }]);
  });

  it("every terminal pane of a tiled tab streams (not just one focused agent)", () => {
    const entries = deriveInterest({
      ...base,
      paneAgents: [
        { agentId: "a", view: "terminal" },
        { agentId: "b", view: "terminal" },
      ],
    });
    expect(entries).toEqual([
      { id: "a", mode: "stream" },
      { id: "b", mode: "stream" },
    ]);
  });

  it("a diff/file view open on an agent is none — absent from the set", () => {
    const entries = deriveInterest({
      ...base,
      paneAgents: [
        { agentId: "a", view: "diff" },
        { agentId: "b", view: "file" },
      ],
    });
    expect(entries).toEqual([]); // absent = none: renderer bytes/sec = 0
  });

  it("overview mini-preview cards in the viewport are digest", () => {
    const entries = deriveInterest({ ...base, overviewAgentIds: ["b", "a"] });
    expect(entries).toEqual([
      { id: "a", mode: "digest" },
      { id: "b", mode: "digest" },
    ]);
  });

  it("the strongest surface wins: terminal + diff on the same agent = stream", () => {
    const entries = deriveInterest({
      ...base,
      paneAgents: [
        { agentId: "a", view: "diff" },
        { agentId: "a", view: "terminal" },
      ],
    });
    expect(entries).toEqual([{ id: "a", mode: "stream" }]);
  });

  it("digest from the overview never downgrades a streaming terminal", () => {
    const entries = deriveInterest({
      ...base,
      paneAgents: [{ agentId: "a", view: "terminal" }],
      overviewAgentIds: ["a"],
    });
    expect(entries).toEqual([{ id: "a", mode: "stream" }]);
  });

  it("a HIDDEN window demotes stream to digest (an unfocused-but-visible one must NOT)", () => {
    const entries = deriveInterest({
      ...base,
      windowVisible: false,
      paneAgents: [
        { agentId: "a", view: "terminal" },
        { agentId: "b", view: "diff" },
      ],
      overviewAgentIds: [],
    });
    expect(entries).toEqual([{ id: "a", mode: "digest" }]);
  });

  it("unmounted agents never appear (absent = none)", () => {
    const entries = deriveInterest({
      ...base,
      paneAgents: [{ agentId: "a", view: "terminal" }],
    });
    expect(entries.find((e) => e.id === "ghost")).toBeUndefined();
  });

  it("output is sorted by id so identical sets compare equal as JSON", () => {
    const a = deriveInterest({ ...base, overviewAgentIds: ["z", "a", "m"] });
    const b = deriveInterest({ ...base, overviewAgentIds: ["m", "z", "a"] });
    expect(JSON.stringify(a)).toBe(JSON.stringify(b));
  });
});

describe("paneLeafViews", () => {
  it("flattens a split tree into (agentId, view) leaves, skipping unassigned", () => {
    const root = group(
      leaf("a", "terminal"),
      group(leaf("b", "diff"), leaf(null, "terminal"), "h"),
    );
    expect(paneLeafViews(root)).toEqual([
      { agentId: "a", view: "terminal" },
      { agentId: "b", view: "diff" },
    ]);
  });
});
