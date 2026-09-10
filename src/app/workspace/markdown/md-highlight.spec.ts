import { describe, expect, it } from "vitest";

import { applyHighlight, unwrapSelection } from "./md-highlight";

/** Pure DOM (jsdom) — no TestBed, same as md-source-map.spec.ts. */
function host(html: string): HTMLElement {
  const el = document.createElement("div");
  el.innerHTML = html;
  return el;
}

describe("applyHighlight", () => {
  it("wraps the snippet's text range and drops a mark after it", () => {
    const root = host("<p>Intro <em>paragraph</em> spanning two lines.</p>");
    applyHighlight(root, { id: "rc1", snippet: "paragraph spanning", fromLine: 3, toLine: 4 });
    const hl = root.querySelector<HTMLElement>("span.md-hl");
    expect(hl?.dataset["id"]).toBe("rc1");
    expect(hl?.textContent).toBe("paragraph spanning");
    const mark = hl?.nextElementSibling as HTMLElement;
    expect(mark.classList.contains("md-mark")).toBe(true);
    expect(mark.dataset["id"]).toBe("rc1");
    expect(mark.title).toBe("Comment · lines 3–4");
    expect(mark.querySelector("svg")).not.toBeNull();
    // the paragraph reads the same
    expect(root.querySelector("p")?.textContent).toBe("Intro paragraph spanning two lines.");
  });

  it("matches across collapsed whitespace and labels a single line", () => {
    const root = host("<ul><li>one\n  two   three</li></ul>");
    applyHighlight(root, { id: "rc2", snippet: "two three", fromLine: 7, toLine: 7 });
    expect(root.querySelector(".md-hl")?.textContent).toBe("two   three");
    expect(root.querySelector<HTMLElement>(".md-mark")?.title).toBe("Comment · lines 7");
  });

  it("is a no-op when the text is gone or the snippet is blank", () => {
    const root = host("<p>hello</p>");
    applyHighlight(root, { id: "x", snippet: "missing", fromLine: 1, toLine: 1 });
    applyHighlight(root, { id: "y", snippet: "   ", fromLine: 1, toLine: 1 });
    expect(root.querySelector(".md-hl, .md-mark")).toBeNull();
  });
});

describe("unwrapSelection", () => {
  it("removes the .md-sel wrap and merges the text back", () => {
    const root = host("<p>a <span class=\"md-sel\">b c</span> d</p>");
    unwrapSelection(root);
    expect(root.querySelector(".md-sel")).toBeNull();
    expect(root.querySelector("p")?.childNodes.length).toBe(1);
    expect(root.querySelector("p")?.textContent).toBe("a b c d");
  });
});
