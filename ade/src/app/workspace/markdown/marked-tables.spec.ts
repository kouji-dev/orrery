import { describe, expect, it } from "vitest";

import { createMarked } from "./marked.config";
import { synthesizeTableSeparators } from "./marked-tables";

/** Pure-parse specs — jsdom for the DOM assertions only, no TestBed. */
const dom = (src: string): HTMLElement => {
  const el = document.createElement("div");
  el.innerHTML = createMarked().parse(src) as string;
  return el;
};

const LOOSE = "| Name | Size |\n| a | 1 |\n| b | 2 |\n";
const VALID = "| Name | Size |\n| --- | ---: |\n| a | 1 |\n| b | 2 |\n";

describe("tables extension — separator synthesis", () => {
  it("turns a separator-less 3-line pipe run into a table: 1 header row + 2 body rows", () => {
    const el = dom(LOOSE);
    const table = el.querySelector(".md-box.table > .box-scroll > table");
    expect(table).not.toBeNull();
    expect(table?.querySelectorAll("thead tr")).toHaveLength(1);
    expect(table?.querySelectorAll("thead th")).toHaveLength(2);
    expect(table?.querySelectorAll("tbody tr")).toHaveLength(2);
    expect(el.querySelector("p")).toBeNull();
  });

  it("leaves a prose paragraph containing a stray pipe untouched", () => {
    const src = "Either a | b works here.\n\nAnd | so | does this.\n";
    expect(synthesizeTableSeparators(src)).toBe(src);
    const el = dom(src);
    expect(el.querySelector("table")).toBeNull();
    expect(el.querySelectorAll("p")).toHaveLength(2);
  });

  it("never touches a pipe table inside a fenced code block", () => {
    const src = "```\n" + LOOSE + "```\n\n~~~text\n" + LOOSE + "~~~\n";
    expect(synthesizeTableSeparators(src)).toBe(src);
    const el = dom(src);
    expect(el.querySelector("table")).toBeNull();
    const codes = el.querySelectorAll(".md-box.fence pre code");
    expect(codes).toHaveLength(2);
    // marked drops the fence body's trailing newline; the pipes are otherwise verbatim
    expect(codes[0].textContent).toBe(LOOSE.trimEnd());
  });

  it("is idempotent: an already-valid table is left alone and keeps its alignment", () => {
    expect(synthesizeTableSeparators(VALID)).toBe(VALID);
    expect(synthesizeTableSeparators(synthesizeTableSeparators(LOOSE))).toBe(synthesizeTableSeparators(LOOSE));
    const el = dom(VALID);
    expect(el.querySelectorAll("tbody tr")).toHaveLength(2);
    expect(el.querySelector("tbody td:last-child")?.getAttribute("align")).toBe("right");
  });

  it("skips a run whose rows disagree on cell count, and a single pipe line", () => {
    const ragged = "| a | b |\n| 1 | 2 | 3 |\n";
    expect(synthesizeTableSeparators(ragged)).toBe(ragged);
    const single = "| a | b |\n\ntext\n";
    expect(synthesizeTableSeparators(single)).toBe(single);
  });
});
