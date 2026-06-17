import { describe, it, expect } from "vitest";
import { diffWouldStall } from "./code-diff.component";

// Helpers mirror the shapes used in the headless MergeView benchmark that
// established the thresholds (many long lines = pathological; short lines and a
// single huge line build fast).
function longLines(n: number, len: number): [string, string] {
  const a: string[] = [];
  const b: string[] = [];
  for (let i = 0; i < n; i++) {
    const s = `${i}:` + "x".repeat(len);
    a.push(s);
    b.push(i % 2 === 0 ? s.slice(0, 10) + "EDIT" + s.slice(14) : s);
  }
  return [a.join("\n"), b.join("\n")];
}
function shortLines(n: number): [string, string] {
  const a: string[] = [];
  const b: string[] = [];
  for (let i = 0; i < n; i++) {
    const s = `line ${i} ` + "x".repeat(50);
    a.push(s);
    b.push(i % 3 === 0 ? s + "!" : s);
  }
  return [a.join("\n"), b.join("\n")];
}

describe("diffWouldStall", () => {
  it("does not flag small files", () => {
    expect(diffWouldStall("a\nb\nc\n", "a\nB\nc\n")).toBe(false);
  });

  it("does not flag large files made of short lines (those diff fast)", () => {
    const [o, n] = shortLines(20000); // ~1.2MB each, ~60 char lines
    expect(diffWouldStall(o, n)).toBe(false);
  });

  it("does not flag a normal large code file", () => {
    const [o, n] = longLines(5000, 76); // 5000 lines, 80-ish chars each
    expect(diffWouldStall(o, n)).toBe(false);
  });

  it("flags many long lines (the case measured at 0.7s–16s)", () => {
    expect(diffWouldStall(...longLines(1000, 2000))).toBe(true);
    expect(diffWouldStall(...longLines(3000, 2000))).toBe(true);
    expect(diffWouldStall(...longLines(1000, 8000))).toBe(true);
  });

  it("flags a huge single-line (minified) file — useless to diff anyway", () => {
    const a = "a".repeat(1_000_000);
    const b = a.slice(0, 500_000) + "XYZ" + a.slice(500_003);
    expect(diffWouldStall(a, b)).toBe(true);
  });
});
