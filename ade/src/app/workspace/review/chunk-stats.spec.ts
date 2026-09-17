import { describe, expect, it } from "vitest";

import { lineChangeStats, LineChangeLite } from "./chunk-stats";

// Monaco ILineChange convention: END 0 = "no lines on this side", START then
// names the line BEFORE the change point.
const change = (
  oStart: number,
  oEnd: number,
  mStart: number,
  mEnd: number,
): LineChangeLite => ({
  originalStartLineNumber: oStart,
  originalEndLineNumber: oEnd,
  modifiedStartLineNumber: mStart,
  modifiedEndLineNumber: mEnd,
});

describe("lineChangeStats", () => {
  it("counts a single-line modification as +1 −1", () => {
    const s = lineChangeStats([change(2, 2, 2, 2)], "a\nb\nc", "a\nB\nc");
    expect(s).toEqual({ add: 1, del: 1, hunks: 1, hunk: "@@ -2,1 +2,1 @@" });
  });

  it("counts pure insertions (original side empty on the change)", () => {
    const s = lineChangeStats([change(1, 0, 2, 3)], "a", "a\nx\ny");
    expect(s).toEqual({ add: 2, del: 0, hunks: 1, hunk: "@@ -1,0 +2,2 @@" });
  });

  it("counts pure deletions (modified side empty on the change)", () => {
    const s = lineChangeStats([change(2, 3, 1, 0)], "a\nx\ny", "a");
    expect(s).toEqual({ add: 0, del: 2, hunks: 1, hunk: "@@ -2,2 +1,0 @@" });
  });

  it("sums multiple hunks and labels the first", () => {
    const s = lineChangeStats(
      [change(1, 1, 1, 2), change(5, 6, 7, 7)],
      "a\nb\nc\nd\ne\nf",
      "A\nA2\nb\nc\nd\nE\n",
    );
    expect(s.add).toBe(3);
    expect(s.del).toBe(3);
    expect(s.hunks).toBe(2);
    expect(s.hunk).toBe("@@ -1,1 +1,2 @@");
  });

  it("added file: exact +N with no change walk", () => {
    const s = lineChangeStats([], "", "one\ntwo\nthree");
    expect(s).toEqual({ add: 3, del: 0, hunks: 1, hunk: "@@ -0,0 +1,3 @@" });
  });

  it("deleted file: exact −N with no change walk", () => {
    const s = lineChangeStats([], "one\ntwo", "");
    expect(s).toEqual({ add: 0, del: 2, hunks: 1, hunk: "@@ -1,2 +0,0 @@" });
  });

  it("both sides empty / no changes", () => {
    expect(lineChangeStats([], "", "")).toEqual({ add: 0, del: 0, hunks: 0, hunk: "@@ -0,0 +0,0 @@" });
    expect(lineChangeStats([], "same", "same")).toEqual({ add: 0, del: 0, hunks: 0, hunk: "@@ -0,0 +0,0 @@" });
  });
});
