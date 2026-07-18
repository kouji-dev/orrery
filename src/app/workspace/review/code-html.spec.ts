import { describe, expect, it } from "vitest";
import { buildLineHtml, buildNewLineHtml, segToRanges } from "./code-html";
import { diffToHunks, diffToRows, type Seg } from "./unified-diff";

describe("segToRanges", () => {
  it("returns [] for no seg", () => {
    expect(segToRanges(undefined)).toEqual([]);
  });
  it("maps changed segments to char ranges", () => {
    const seg: Seg[] = [
      { c: false, s: "const " },
      { c: true, s: "x" },
      { c: false, s: " = 1" },
    ];
    expect(segToRanges(seg)).toEqual([[6, 7]]);
  });
});

describe("buildLineHtml", () => {
  it("escapes plain text with no runs and no changes", () => {
    expect(buildLineHtml(null, "a < b && c", [], "")).toBe("a &lt; b &amp;&amp; c");
  });

  it("wraps only the changed range in the change class", () => {
    // "const x = 1" with "x" changed
    const html = buildLineHtml(null, "const x = 1", [[6, 7]], "rc-chg-add");
    expect(html).toBe('const <span class="rc-chg-add">x</span> = 1');
  });

  it("keeps syntax token classes and overlays the change class", () => {
    const runs = [
      { s: "const", cls: "tok-keyword" },
      { s: " x = 1", cls: "" },
    ];
    const html = buildLineHtml(runs, "const x = 1", [[6, 7]], "rc-chg-add");
    // keyword span preserved; the changed "x" (offset 6) gets the overlay class
    expect(html).toContain('<span class="tok-keyword">const</span>');
    expect(html).toContain('<span class="rc-chg-add">x</span>');
  });

  it("splits a change that spans multiple token runs", () => {
    const runs = [
      { s: "ab", cls: "tok-a" },
      { s: "cd", cls: "tok-b" },
    ];
    // change covers offsets [1,3) → "b" (in run a) + "c" (in run b)
    const html = buildLineHtml(runs, "abcd", [[1, 3]], "chg");
    expect(html).toBe(
      '<span class="tok-a">a</span><span class="tok-a chg">b</span>' +
        '<span class="tok-b chg">c</span><span class="tok-b">d</span>',
    );
  });
});

describe("buildNewLineHtml", () => {
  it("renders a collapsed deletion bar at the removal offset", () => {
    // new line "ab" with ", sql"-like text removed at offset 2 (end)
    const html = buildNewLineHtml(null, "ab", [], [{ at: 2, s: "X" }], false);
    expect(html).toBe('ab<span class="rc-del-caret" title="removed: X"></span>');
  });

  it("reveals the removed text in red when revealed", () => {
    const html = buildNewLineHtml(null, "ab", [], [{ at: 2, s: "X" }], true);
    expect(html).toBe('ab<span class="rc-chg-del rc-del-shown" title="click to hide">X</span>');
  });

  it("overlays added ranges green and keeps token classes", () => {
    const runs = [{ s: "ab", cls: "tok-x" }];
    const html = buildNewLineHtml(runs, "ab", [[1, 2]], [], false);
    // 'a' plain-in-token, 'b' token+add overlay
    expect(html).toBe('<span class="tok-x">a</span><span class="tok-x rc-chg-add">b</span>');
  });

  it("places a deletion bar between characters", () => {
    const html = buildNewLineHtml(null, "ac", [], [{ at: 1, s: "b" }], false);
    expect(html).toBe('a<span class="rc-del-caret" title="removed: b"></span>c');
  });
});

describe("diffToRows single-panel folding", () => {
  it("folds the old line and marks the new line with an inline deletion", () => {
    const rows = diffToRows(diffToHunks("const x = 1\n", "const  = 1\n"));
    const del = rows.find((r) => r.type === "code" && r.k === "-");
    const add = rows.find((r) => r.type === "code" && r.k === "+");
    expect(del?.type === "code" && del.pairedHidden).toBe(true);
    expect(add?.type === "code" && (add.dels?.length ?? 0) > 0).toBe(true);
  });
});

describe("diffToRows intra-line word diff", () => {
  it("marks only the changed word on a modified line pair", () => {
    const rows = diffToRows(diffToHunks("const x = 1\n", "const y = 1\n"));
    const del = rows.find((r) => r.type === "code" && r.k === "-");
    const add = rows.find((r) => r.type === "code" && r.k === "+");
    expect(del?.type === "code" && del.seg).toBeTruthy();
    expect(add?.type === "code" && add.seg).toBeTruthy();
    // the changed segment carries just the word "x" / "y", the rest is unchanged
    if (del?.type === "code" && del.seg) {
      expect(del.seg.filter((s) => s.c).map((s) => s.s)).toEqual(["x"]);
      expect(del.seg.some((s) => !s.c)).toBe(true);
    }
    if (add?.type === "code" && add.seg) {
      expect(add.seg.filter((s) => s.c).map((s) => s.s)).toEqual(["y"]);
    }
  });

  it("leaves wholly-different lines without a seg (plain full-line tint)", () => {
    const rows = diffToRows(diffToHunks("aaaa\n", "bbbb\n"));
    const del = rows.find((r) => r.type === "code" && r.k === "-");
    expect(del?.type === "code" && del.seg).toBeFalsy();
  });
});
