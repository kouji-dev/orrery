import { describe, expect, it } from "vitest";

import { createMarked } from "./marked.config";
import { blockSpans, rangeToLines, stampBlocks } from "./md-source-map";

/**
 * Source-map specs — pure functions + jsdom for the stamping, no TestBed
 * (see ticket-card.component.spec.ts for why).
 */

// prettier-ignore
const FIXTURE = [
  "# Title",              // 1  heading
  "",                     // 2
  "Intro paragraph",      // 3  paragraph
  "spanning two lines.",  // 4
  "",                     // 5
  "[ref]: https://x.y",   // 6  def — renders NOTHING
  "",                     // 7
  "| a | b |",            // 8  table
  "|---|---|",            // 9
  "| 1 | 2 |",            // 10 row 0
  "| 3 | 4 |",            // 11 row 1
  "",                     // 12
  "```js",                // 13 fence
  "const a = 1;",         // 14
  "```",                  // 15
  "",                     // 16
  "- one",                // 17 list
  "- two",                // 18
  "",                     // 19
  "> quoted [link][ref]", // 20 blockquote
  "",                     // 21
  "---",                  // 22 hr
  "",                     // 23
  "Last paragraph",       // 24 paragraph
  "",
].join("\n");

const EXPECTED = [
  { from: 1, to: 1, type: "heading" },
  { from: 3, to: 4, type: "paragraph" },
  { from: 8, to: 11, type: "table" },
  { from: 13, to: 15, type: "code" },
  { from: 17, to: 18, type: "list" },
  { from: 20, to: 20, type: "blockquote" },
  { from: 22, to: 22, type: "hr" },
  { from: 24, to: 24, type: "paragraph" },
];

function render(src: string): HTMLElement {
  const host = document.createElement("div");
  host.className = "rte-view";
  host.innerHTML = createMarked().parse(src) as string;
  return host;
}

describe("blockSpans", () => {
  it("maps every rendered block to its exact source lines, skipping blank lines and link defs", () => {
    expect(blockSpans(FIXTURE)).toEqual(EXPECTED);
  });

  it("does not let a link-reference definition offset the blocks after it", () => {
    const withDef = blockSpans("[ref]: https://x.y\n\n# H\n\ntext\n");
    expect(withDef).toEqual([
      { from: 3, to: 3, type: "heading" },
      { from: 5, to: 5, type: "paragraph" },
    ]);
    // the def contributed lines to the cursor but no span
    expect(withDef.length).toBe(render("[ref]: https://x.y\n\n# H\n\ntext\n").children.length);
  });

  it("counts a mermaid fence with a mis-indented closer as exactly its own lines", () => {
    const spans = blockSpans("```mermaid\ngraph TD; A-->B;\n    ```\n\n## After\n");
    expect(spans).toEqual([
      { from: 1, to: 3, type: "code" },
      { from: 5, to: 5, type: "heading" },
    ]);
    // unclosed: the block stops before the heading, blank lines excluded from `to`
    expect(blockSpans("```mermaid\ngraph TD; A-->B;\n\n## After\n")).toEqual([
      { from: 1, to: 2, type: "code" },
      { from: 4, to: 4, type: "heading" },
    ]);
  });

  it("yields one span per element child of marked's output, in order", () => {
    const host = render(FIXTURE);
    const spans = blockSpans(FIXTURE);
    expect(spans.length).toBe(host.children.length);
    const tags = Array.from(host.children).map((c) => c.tagName.toLowerCase());
    expect(tags).toEqual(["h1", "p", "div", "div", "ul", "blockquote", "hr", "p"]);
  });
});

describe("stampBlocks", () => {
  it("stamps data-l0/data-l1 on each top-level block and per-row on tables; idempotent", () => {
    const host = render(FIXTURE);
    const spans = blockSpans(FIXTURE);
    stampBlocks(host, spans);
    stampBlocks(host, spans); // second pass must not shift anything

    const kids = Array.from(host.children) as HTMLElement[];
    expect(kids.map((k) => [Number(k.dataset["l0"]), Number(k.dataset["l1"])])).toEqual(
      EXPECTED.map((s) => [s.from, s.to]),
    );
    const rows = Array.from(host.querySelectorAll<HTMLElement>("tbody tr"));
    expect(rows.map((r) => r.dataset["l0"])).toEqual(["10", "11"]);
    expect(rows.map((r) => r.dataset["l1"])).toEqual(["10", "11"]);
  });

  it("survives a diagram box replacing its placeholder in place", () => {
    const src = "# H\n\n```mermaid\ngraph TD; A-->B;\n```\n\ntext\n";
    const host = render(src);
    const spans = blockSpans(src);
    stampBlocks(host, spans);
    const box = document.createElement("div");
    box.className = "md-box diagram";
    host.querySelector(".md-box.diagram.pending")!.replaceWith(box);
    stampBlocks(host, spans);
    expect(box.dataset["l0"]).toBe("3");
    expect(box.dataset["l1"]).toBe("5");
    expect((host.lastElementChild as HTMLElement).dataset["l0"]).toBe("7");
  });
});

describe("rangeToLines", () => {
  it("resolves a selection to the stamped lines of its start and end blocks, ordered", () => {
    const host = render(FIXTURE);
    stampBlocks(host, blockSpans(FIXTURE));
    const p = host.querySelector("p")!; // lines 3-4
    const li = host.querySelector("li")!; // inside the list, lines 17-18
    const range = document.createRange();
    range.setStart(p.firstChild!, 3);
    range.setEnd(li.firstChild!, 2);
    expect(rangeToLines(host, range)).toEqual({ from: 3, to: 18 });
    // a selection inside one block reports that block's whole span
    const back = document.createRange();
    back.setStart(li.firstChild!, 0);
    back.setEnd(li.firstChild!, 1);
    expect(rangeToLines(host, back)).toEqual({ from: 17, to: 18 });
  });

  it("anchors a table-cell selection to the row's own line", () => {
    const host = render(FIXTURE);
    stampBlocks(host, blockSpans(FIXTURE));
    const td = host.querySelectorAll("tbody td")[2]!; // row 1, col 0 → line 11
    const range = document.createRange();
    range.selectNodeContents(td);
    expect(rangeToLines(host, range)).toEqual({ from: 11, to: 11 });
  });

  it("falls back to line 1 when nothing is stamped", () => {
    const host = render("plain\n");
    const range = document.createRange();
    range.selectNodeContents(host.querySelector("p")!);
    expect(rangeToLines(host, range)).toEqual({ from: 1, to: 1 });
  });
});
