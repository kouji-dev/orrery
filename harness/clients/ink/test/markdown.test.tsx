/**
 * Task 4: a half-arrived document is plain, a complete one is formatted.
 *
 * The fixture is `streaming-markdown`, which cuts the deltas inside a code
 * fence on purpose: at the middle checkpoint the fence is open and `complete`
 * is false, and at the end both are settled.
 */

import { render } from "ink-testing-library";
import { describe, expect, it } from "vitest";

import { SurfaceStore } from "@orrery/client";

import { MarkdownSurface, parse, spans } from "../src/surfaces/Markdown.js";
import { frames } from "./fake.js";

/** Replay the first `count` frames and hand back the message surface. */
function messageAfter(count: number): { kind: { t: "markdown"; value: string; complete: boolean } } {
  const store = new SurfaceStore();
  for (const frame of frames("streaming-markdown").slice(0, count)) store.apply(frame);
  const surface = store
    .state()
    .turns.flatMap((t) => t.surfaces)
    .find((s) => s.id === "msg-1");
  if (!surface || surface.kind.t !== "markdown") throw new Error("no markdown surface");
  return surface as { kind: { t: "markdown"; value: string; complete: boolean } };
}

describe("streaming markdown", () => {
  it("markdown.plain_until_complete", () => {
    // Four frames in: `Here is the fix:` then an unclosed ```rust fence.
    const partial = messageAfter(4);
    expect(partial.kind.complete).toBe(false);
    expect(partial.kind.value).toContain("```rust");

    const mid = render(<MarkdownSurface node={partial} width={60} />);
    const plain = mid.lastFrame() ?? "";
    // Plain means plain: the fence marker is still there, undigested, and
    // the body is one verbatim block rather than a parsed document.
    expect(plain).toContain("```rust");
    expect(plain).toContain("fn main() {");
    expect(plain).not.toContain("  fn main() {");
    mid.unmount();

    const whole = messageAfter(9);
    expect(whole.kind.complete).toBe(true);
    const done = render(<MarkdownSurface node={whole} width={60} />);
    const formatted = done.lastFrame() ?? "";
    // Formatted: the fence markers are gone and the code block is indented.
    // (Colour is real but invisible here: ink-testing-library's stdout is not
    // a tty, so chalk strips every escape.)
    expect(formatted).not.toContain("```");
    expect(formatted).toContain("  fn main() {");
    expect(formatted).toMatchSnapshot();
    done.unmount();
  });

  it("renders the subset a terminal can show", () => {
    const doc = [
      "# Heading",
      "",
      "A **bold** word, an _italic_ one and `code`.",
      "",
      "- first",
      "- second",
      "",
      "> quoted",
      "",
      "```sh",
      "cargo test",
      "```",
    ].join("\n");
    const lines = parse(doc);
    expect(lines[0]).toMatchObject({ heading: true });
    expect(lines.some((l) => l.quote === true)).toBe(true);
    expect(lines.some((l) => l.code === true)).toBe(true);
    expect(lines.some((l) => l.spans.some((s) => s.text.startsWith("• ")))).toBe(true);

    const ui = render(
      <MarkdownSurface node={{ kind: { t: "markdown", value: doc, complete: true } }} width={40} />,
    );
    expect(ui.lastFrame()).toMatchSnapshot();
    ui.unmount();
  });

  it("splits emphasis without a markdown library", () => {
    expect(spans("a **b** c")).toEqual([
      { text: "a " },
      { text: "b", bold: true },
      { text: " c" },
    ]);
    expect(spans("`x`")).toEqual([{ text: "x", code: true }]);
    // An unmatched marker is text, not a parse error.
    expect(spans("a * b")).toEqual([{ text: "a * b" }]);
  });
});
