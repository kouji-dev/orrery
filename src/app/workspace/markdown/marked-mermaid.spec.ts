import { describe, expect, it } from "vitest";

import { createMarked } from "./marked.config";

/**
 * Pure-parse specs for the mermaid fence extension — jsdom only for the
 * DOM assertions on marked's HTML output, no TestBed (see
 * ticket-card.component.spec.ts for why component rendering is off limits).
 */

const parse = (src: string): string => createMarked().parse(src) as string;
const dom = (src: string): HTMLElement => {
  const el = document.createElement("div");
  el.innerHTML = parse(src);
  return el;
};

describe("mermaid fence tokenizer", () => {
  it("closes a mermaid fence on a closer indented 4 spaces — the heading after it still renders", () => {
    const el = dom("```mermaid\ngraph TD; A-->B;\n    ```\n\n## After\n");
    expect(el.querySelector("h2")?.textContent).toBe("After");
    expect(el.querySelector(".md-box.diagram.pending code.language-mermaid")?.textContent).toBe("graph TD; A-->B;");
  });

  it("ends an unclosed mermaid fence at the next heading instead of swallowing the rest of the document", () => {
    const el = dom("```mermaid\ngraph TD; A-->B;\n\n## After\n\ntext\n");
    expect(el.querySelector("h2")?.textContent).toBe("After");
    expect(el.querySelector("p")?.textContent).toBe("text");
    // trailing blank lines are not part of the diagram source
    expect(el.querySelector("code.language-mermaid")?.textContent).toBe("graph TD; A-->B;");
  });

  it("ends an unclosed mermaid fence at end of input", () => {
    const el = dom("# Before\n\n```mermaid\ngraph TD; A-->B;\n");
    expect(el.querySelector("h1")?.textContent).toBe("Before");
    expect(el.querySelector("code.language-mermaid")?.textContent).toBe("graph TD; A-->B;");
  });

  it("keeps strict CommonMark for every other language — a 4-space closer does not close a python fence", () => {
    const el = dom("```python\nprint(1)\n    ```\n\n## After\n");
    // the closer was absorbed into the block, and so was the heading
    expect(el.querySelector("h2")).toBeNull();
    expect(el.querySelector("code.language-python")?.textContent).toContain("## After");
  });

  it("supports ~~~ fences and an indented opener (dedents the body like marked does)", () => {
    const el = dom("- item\n\n   ~~~mermaid\n   graph LR; A-->B;\n       ~~~\n\n## After\n");
    expect(el.querySelector("code.language-mermaid")?.textContent).toBe("graph LR; A-->B;");
    expect(el.querySelector("h2")?.textContent).toBe("After");
  });
});

describe("code renderer", () => {
  it("renders a mermaid fence as the pending diagram placeholder with escaped source", () => {
    const el = dom("```mermaid\ngraph TD; A-->B;\n```\n");
    const box = el.querySelector(".md-box.diagram.pending");
    expect(box).not.toBeNull();
    expect(box?.querySelector("pre > code.language-mermaid")?.textContent).toBe("graph TD; A-->B;");
    // nothing the sanitizer would strip: no data-* / id attributes
    expect(box?.getAttributeNames()).toEqual(["class"]);
  });

  it("renders a js fence as the .md-box.fence card with the lang chip and line count", () => {
    const el = dom("```js\nconst a = 1;\nconst b = 2;\n```\n");
    const box = el.querySelector(".md-box.fence")!;
    expect(box).not.toBeNull();
    expect(box.querySelector(".fence-hd.static .chip.mono")?.textContent).toBe("js");
    expect(box.querySelector(".fence-hd.static .tnum.mono")?.textContent).toBe("2 lines");
    expect(box.querySelector("pre > code.language-js")?.textContent).toBe("const a = 1;\nconst b = 2;");
  });

  it("labels a fence without a language 'text' and escapes HTML in the body", () => {
    const el = dom("```\n<b>x</b>\n```\n");
    expect(el.querySelector(".fence-hd .chip")?.textContent).toBe("text");
    expect(el.querySelector("pre > code")?.innerHTML).toBe("&lt;b&gt;x&lt;/b&gt;");
    expect(el.querySelector("pre b")).toBeNull();
  });

  it("wraps tables in the design's scrolling box", () => {
    const el = dom("| a | b |\n|---|---|\n| 1 | 2 |\n");
    expect(el.querySelector(".md-box.table > .box-scroll > table tbody td")?.textContent).toBe("1");
  });
});
