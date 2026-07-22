import { describe, it, expect } from "vitest";
import { Text } from "@codemirror/state";
import { Chunk } from "@codemirror/merge";
import { chunkStats } from "./chunk-stats";

function doc(s: string): Text {
  return Text.of(s.split("\n"));
}
function stats(oldText: string, newText: string) {
  const a = doc(oldText);
  const b = doc(newText);
  return chunkStats(Chunk.build(a, b), a, b);
}

describe("chunkStats", () => {
  it("single-line change → 1 hunk, 1/1", () => {
    const s = stats("x\nold\nz", "x\nnew\nz");
    expect(s).toMatchObject({ add: 1, del: 1, hunks: 1, hunk: "@@ -2,1 +2,1 @@" });
  });

  it("pure insertion → add only", () => {
    const s = stats("a\nc", "a\nb\nc");
    expect(s.add).toBe(1);
    expect(s.del).toBe(0);
    expect(s.hunks).toBe(1);
  });

  it("pure deletion → del only", () => {
    const s = stats("a\nb\nc", "a\nc");
    expect(s.add).toBe(0);
    expect(s.del).toBe(1);
  });

  it("added file (empty old) → +N, no −", () => {
    expect(stats("", "a\nb")).toMatchObject({ add: 2, del: 0, hunks: 1, hunk: "@@ -0,0 +1,2 @@" });
  });

  it("deleted file (empty new) → −N, no +", () => {
    expect(stats("a\nb", "")).toMatchObject({ add: 0, del: 2, hunks: 1, hunk: "@@ -1,2 +0,0 @@" });
  });

  it("both empty → zeros", () => {
    expect(stats("", "")).toMatchObject({ add: 0, del: 0, hunks: 0 });
  });

  it("identical → zeros, no hunks", () => {
    expect(stats("a\nb\nc", "a\nb\nc")).toMatchObject({ add: 0, del: 0, hunks: 0 });
  });

  it("two separated edits → 2 hunks", () => {
    const oldT = ["a", "b", "c", "d", "e", "f", "g", "h", "i", "j", "k", "l"].join("\n");
    const newT = ["a", "B", "c", "d", "e", "f", "g", "h", "i", "j", "K", "l"].join("\n");
    const s = stats(oldT, newT);
    expect(s.hunks).toBe(2);
    expect(s.add).toBe(2);
    expect(s.del).toBe(2);
  });

  it("block moved down stays aligned — no giant misplaced hunks (regression)", () => {
    // The old naive LCS cross-matched blank/trivial lines across distant
    // regions, painting content 'deleted at top, re-added at bottom'.
    const block = ["function f() {", "  return 1;", "}"];
    const oldT = [...block, "", "const x = 1;", "const y = 2;"].join("\n");
    const newT = ["const x = 1;", "const y = 2;", "", ...block].join("\n");
    const s = stats(oldT, newT);
    // a move shows as one delete region + one insert region, NOT a rewrite of
    // every line: changed lines must stay well under the full file size
    expect(s.add + s.del).toBeLessThanOrEqual(2 * block.length + 2);
  });

  it("10k-line file with one edit → exact 1/1 fast", () => {
    const lines = Array.from({ length: 10_000 }, (_, i) => `line ${i};`);
    const oldT = lines.join("\n");
    lines[5000] = "line 5000 CHANGED;";
    const newT = lines.join("\n");
    const t0 = performance.now();
    const s = stats(oldT, newT);
    const elapsed = performance.now() - t0;
    expect(s).toMatchObject({ add: 1, del: 1, hunks: 1 });
    expect(elapsed).toBeLessThan(1000); // the old LCS DP froze / OOMed here
  });
});
