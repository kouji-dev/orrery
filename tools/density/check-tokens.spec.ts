import { describe, it, expect } from "vitest";
import { scanText } from "./check-tokens.mjs";

describe("scanText", () => {
  it("flags px literals in target properties", () => {
    expect(scanText('style="padding:6px 10px 7px;gap:7px"')).toEqual([
      "padding: 6px 10px 7px",
      "gap: 7px",
    ]);
    expect(scanText("font-size:9.5px")).toEqual(["font-size: 9.5px"]);
  });

  it("allows tokens, 0, 1px borders, 999px, percentages", () => {
    expect(scanText("padding:var(--sp-3) var(--sp-5)")).toEqual([]);
    expect(scanText("height:1px")).toEqual([]);
    expect(scanText("border-radius:999px")).toEqual([]);
    expect(scanText("margin:0;width:100%")).toEqual([]);
  });

  it("ignores out-of-scope properties (max-height, width, line-height, border)", () => {
    expect(scanText("max-height:320px;width:252px;line-height:20px;border:1px solid")).toEqual([]);
  });

  it("does not match custom-property definitions", () => {
    expect(scanText("--row-h: calc(30px * var(--density))")).toEqual([]);
  });

  it("flags negative margins", () => {
    expect(scanText("margin-left:-8px")).toEqual(["margin-left: -8px"]);
  });
});
