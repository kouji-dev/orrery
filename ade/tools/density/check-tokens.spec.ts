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

  it("covers width properties — a text container must grow with its text", () => {
    // These were deliberately out of scope until the type ramp was rebased, at
    // which point fixed-width panels began clipping their own contents.
    expect(scanText("max-height:320px;width:252px")).toEqual([
      "max-height: 320px",
      "width: 252px",
    ]);
  });

  it("still ignores line-height and border", () => {
    expect(scanText("line-height:20px;border:1px solid")).toEqual([]);
  });

  it("accepts the density-derived and responsive forms", () => {
    // A px literal multiplied by the density scalar IS the tokenized form.
    expect(scanText("width:round(calc(196px * var(--density)), 1px)")).toEqual([]);
    expect(scanText("max-width:calc(100vw - 32px)")).toEqual([]);
    expect(scanText("width:min(340px, 86%)")).toEqual([]);
  });

  it("exempts allowlisted px TOKENS, not whole declarations", () => {
    // Matching used to be a substring test on the raw declaration, so an entry
    // for "4px" silently permitted 24px/104px, and one matched token exempted
    // every other px beside it.
    expect(scanText("gap:4px", "conflict-view.component.ts")).toEqual([]);
    expect(scanText("padding:104px", "conflict-view.component.ts")).toEqual([
      "padding: 104px",
    ]);
    expect(scanText("padding:3px 33px", "conflict-view.component.ts")).toEqual([
      "padding: 3px 33px",
    ]);
  });

  it("does not match custom-property definitions", () => {
    expect(scanText("--row-h: calc(30px * var(--density))")).toEqual([]);
  });

  it("flags negative margins", () => {
    expect(scanText("margin-left:-8px")).toEqual(["margin-left: -8px"]);
  });
});
