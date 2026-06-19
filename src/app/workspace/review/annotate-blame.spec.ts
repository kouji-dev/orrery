import { describe, it, expect } from "vitest";
import { blameToRows } from "./annotate-blame.component";
import { BlameLine } from "../../models";

const bl = (n: number, sha: string, when: number, line: string): BlameLine => ({ n, sha, author: "Ann", when, summary: "msg", line });

describe("blameToRows", () => {
  it("flags first-of-commit rows and normalizes age 0..1", () => {
    const rows = blameToRows([bl(1, "aaa", 100, "x"), bl(2, "aaa", 100, "y"), bl(3, "bbb", 50, "z")]);
    expect(rows.map((r) => r.first)).toEqual([true, false, true]);
    expect(rows[0].age).toBeCloseTo(0, 5);   // newest
    expect(rows[2].age).toBeCloseTo(1, 5);   // oldest
    expect(rows[0].s).toBe("x");
  });

  it("makes rel time deterministic via now parameter", () => {
    // Fixed now = 1750768800000 ms
    const FIXED_NOW = 1750768800000;
    // when = 1750761600 (2 hours = 7200 seconds before FIXED_NOW)
    const WHEN_2H_AGO = 1750761600;
    const rows = blameToRows([bl(1, "aaa", WHEN_2H_AGO, "test")], FIXED_NOW);
    expect(rows[0].rel).toBe("2h");
  });
});
