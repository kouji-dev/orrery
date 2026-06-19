import { describe, it, expect } from "vitest";
import { diffToHunks, diffToRows, fileToRows } from "./unified-diff";

describe("diffToHunks", () => {
  it("emits a single + hunk for a new file", () => {
    const h = diffToHunks("", "a\nb\n");
    expect(h.length).toBe(1);
    expect(h[0].lines.map((l) => l.k)).toEqual(["+", "+"]);
    expect(h[0].lines.map((l) => l.n)).toEqual([1, 2]);
    expect(h[0].meta).toBe("@@ -0,0 +1,2 @@");
  });

  it("marks a replaced line as - then + with correct side line numbers", () => {
    const h = diffToHunks("x\nold\nz\n", "x\nnew\nz\n");
    const flat = h.flatMap((x) => x.lines).map((l) => `${l.k} ${l.n}:${l.s}`);
    expect(flat).toEqual(["  1:x", "- 2:old", "+ 2:new", "  3:z"]);
  });

  it("keeps unchanged lines as context within the surrounding window", () => {
    const h = diffToHunks("a\nb\nc\n", "a\nB\nc\n");
    expect(h[0].lines.some((l) => l.k === "-" && l.s === "b")).toBe(true);
    expect(h[0].lines.some((l) => l.k === "+" && l.s === "B")).toBe(true);
  });

  it("emits a single - hunk for a fully deleted file", () => {
    const h = diffToHunks("a\nb\n", "");
    expect(h.length).toBe(1);
    expect(h[0].lines.map((l) => l.k)).toEqual(["-", "-"]);
    expect(h[0].lines.map((l) => l.n)).toEqual([1, 2]);
    expect(h[0].meta).toBe("@@ -1,2 +0,0 @@");
  });
});

describe("row builders", () => {
  it("diffToRows interleaves a hunk separator then code rows with side", () => {
    const rows = diffToRows(diffToHunks("old\n", "new\n"));
    expect(rows[0].type).toBe("hunk");
    const code = rows.filter((r) => r.type === "code") as Extract<typeof rows[number], { type: "code" }>[];
    expect(code.find((r) => r.k === "-")!.side).toBe("old");
    expect(code.find((r) => r.k === "+")!.side).toBe("new");
  });

  it("fileToRows yields one context row per line, side=file", () => {
    const rows = fileToRows("a\nb\n");
    expect(rows.map((r) => (r.type === "code" ? r.n : -1))).toEqual([1, 2]);
    expect(rows.every((r) => r.type === "code" && r.side === "file")).toBe(true);
  });
});
