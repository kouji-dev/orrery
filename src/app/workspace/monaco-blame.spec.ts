import { describe, expect, it } from "vitest";
import { BlameLine } from "../models";
import { BLAME_LABEL_WIDTH, authorBucket, blameLabel, initials, relTime } from "./review/blame-format";
import { blameSpecs, blameDecorations, shaAtLine } from "./monaco-blame";

const NOW = Date.UTC(2026, 0, 10, 12, 0, 0); // fixed clock — relative strings
const secs = (d: number) => Math.floor(NOW / 1000) - d;

function line(n: number, sha: string, author: string, when: number, text: string): BlameLine {
  return { n, sha, author, when, summary: `commit ${sha}`, line: text };
}

describe("blame-format", () => {
  it("labels every line to the same width, so the code starts at one column", () => {
    const row = { author: "Ada Lovelace", sha: "abc1234def", when: secs(3600) };
    const first = blameLabel(row, true, NOW);
    const cont = blameLabel(row, false, NOW);
    expect(first).toHaveLength(BLAME_LABEL_WIDTH);
    expect(cont).toHaveLength(BLAME_LABEL_WIDTH);
    expect(cont.trim()).toBe(""); // a continuation line shows nothing
    expect(first).toContain("AL");
    expect(first).toContain("abc1234"); // 7-char sha, not the full one
    expect(first).toContain("1h");
  });

  it("keeps the column for an uncommitted line, which has no sha", () => {
    const label = blameLabel({ author: "Uncommitted", sha: "0000000", when: 0 }, true, NOW);
    expect(label).toHaveLength(BLAME_LABEL_WIDTH);
    expect(label).toContain("·······");
    expect(label).not.toContain("0000000");
  });

  it("truncates a long name and a long relative time into their columns", () => {
    const label = blameLabel(
      { author: "Wolfgang Amadeus Mozart", sha: "0123456789", when: secs(60 * 60 * 24 * 800) },
      true,
      NOW,
    );
    expect(label).toHaveLength(BLAME_LABEL_WIDTH);
    expect(label.startsWith("WA ")).toBe(true);
    expect(label).toContain("2y");
  });

  it("buckets authors into the hue range the CSS defines", () => {
    for (const name of ["a", "Ada", "Grace Hopper", "", "агент"]) {
      const b = authorBucket(name);
      expect(b).toBeGreaterThanOrEqual(0);
      expect(b).toBeLessThan(12);
      expect(Number.isInteger(b)).toBe(true);
    }
    expect(authorBucket("Ada")).toBe(authorBucket("Ada")); // stable
  });

  it("formats initials and relative times", () => {
    expect(initials("Ada Lovelace")).toBe("AL");
    expect(initials("cher")).toBe("C");
    expect(relTime(0, NOW)).toBe("");
    expect(relTime(secs(30), NOW)).toBe("30s");
    expect(relTime(secs(60 * 60 * 24 * 2), NOW)).toBe("2d");
  });
});

describe("blameSpecs", () => {
  const lines = [
    line(1, "aaa1111", "Ada Lovelace", secs(3600), "const a = 1;"),
    line(2, "aaa1111", "Ada Lovelace", secs(3600), "const b = 2;"),
    line(3, "bbb2222", "Grace Hopper", secs(86400), "const c = 3;"),
  ];

  it("labels only the first line of each commit run", () => {
    const specs = blameSpecs(lines, NOW);
    expect(specs.map((s) => s.label.trim() !== "")).toEqual([true, false, true]);
    expect(specs[0].cls).toBe(`blm blm-h${authorBucket("Ada Lovelace")}`);
    expect(specs[1].cls).toBe("blm blm-cont");
  });

  it("carries the author, summary and sha into the hover", () => {
    const [first] = blameSpecs(lines, NOW);
    expect(first.hover).toContain("Ada Lovelace");
    expect(first.hover).toContain("commit aaa1111");
    expect(first.hover).toContain("aaa1111");
  });

  it("resolves the sha under a line, and refuses the uncommitted one", () => {
    const specs = blameSpecs(
      [...lines, line(4, "0000000", "Uncommitted", 0, "const d = 4;")],
      NOW,
    );
    expect(shaAtLine(specs, 3)).toBe("bbb2222");
    expect(shaAtLine(specs, 4)).toBeNull(); // nothing to open
    expect(shaAtLine(specs, 99)).toBeNull();
  });
});

describe("blameDecorations", () => {
  // Just enough of the Monaco namespace: Range plus the two enums used.
  const monaco = {
    Range: class {
      constructor(
        readonly startLineNumber: number,
        readonly startColumn: number,
        readonly endLineNumber: number,
        readonly endColumn: number,
      ) {}
    },
    editor: {
      TrackedRangeStickiness: { NeverGrowsWhenTypingAtEdges: 1 },
      InjectedTextCursorStops: { None: 3 },
    },
  } as never;

  it("injects one before-decoration per line, at column 1", () => {
    const specs = blameSpecs([line(1, "aaa1111", "Ada", secs(60), "x")], NOW);
    const [d] = blameDecorations(monaco, specs, 10);
    expect(d.range.startLineNumber).toBe(1);
    expect(d.range.startColumn).toBe(1);
    expect(d.options.before?.content).toHaveLength(BLAME_LABEL_WIDTH);
    expect(d.options.before?.inlineClassName).toContain("blm");
    // without this Monaco filters an injected decoration on an empty range out
    // of the view — it renders nothing at all
    expect(d.options.showIfCollapsed).toBe(true);
  });

  it("drops lines past the end of the model — the buffer can have shrunk", () => {
    const specs = blameSpecs(
      [
        line(1, "aaa1111", "Ada", secs(60), "x"),
        line(2, "aaa1111", "Ada", secs(60), "y"),
        line(3, "aaa1111", "Ada", secs(60), "z"),
      ],
      NOW,
    );
    expect(blameDecorations(monaco, specs, 2)).toHaveLength(2);
    expect(blameDecorations(monaco, specs, 0)).toHaveLength(0);
  });
});
