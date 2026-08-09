import { beforeEach, describe, expect, it, vi } from "vitest";

import { renderMermaidBlocks } from "./md-mermaid";

const { initialize, render } = vi.hoisted(() => ({ initialize: vi.fn(), render: vi.fn() }));
vi.mock("mermaid", () => ({ default: { initialize, render } }));

/** marked's output for a ```mermaid fence. */
const FENCE = `<pre><code class="language-mermaid">graph TD; A--&gt;B;</code></pre>`;

function host(html: string): HTMLElement {
  const el = document.createElement("div");
  el.innerHTML = html;
  document.body.appendChild(el);
  return el;
}

describe("renderMermaidBlocks", () => {
  beforeEach(() => {
    initialize.mockClear();
    render.mockClear();
    render.mockResolvedValue({ svg: '<svg class="mmd-out"></svg>' });
    document.body.innerHTML = "";
  });

  it("replaces a mermaid fence with the rendered SVG, keeping source + theme in data attrs", async () => {
    const el = host(`<h1>doc</h1>${FENCE}`);
    await renderMermaidBlocks(el, "dark");

    expect(render).toHaveBeenCalledTimes(1);
    expect(render.mock.calls[0][1]).toBe("graph TD; A-->B;");
    expect(initialize).toHaveBeenCalledWith(expect.objectContaining({ theme: "dark", securityLevel: "strict" }));

    const box = el.querySelector<HTMLElement>(".mmd")!;
    expect(box).not.toBeNull();
    expect(box.querySelector("svg.mmd-out")).not.toBeNull();
    expect(box.dataset["mmdSrc"]).toBe("graph TD; A-->B;");
    expect(box.dataset["mmdTheme"]).toBe("dark");
    expect(el.querySelector("code.language-mermaid")).toBeNull();
    expect(el.querySelector("h1")).not.toBeNull(); // rest of the doc untouched
  });

  it("does not load mermaid when the document has no mermaid blocks", async () => {
    const el = host(`<pre><code class="language-ts">const x = 1;</code></pre>`);
    await renderMermaidBlocks(el, "dark");
    expect(initialize).not.toHaveBeenCalled();
    expect(render).not.toHaveBeenCalled();
    expect(el.querySelector("code.language-ts")).not.toBeNull();
  });

  it("keeps the code fence when the diagram fails to parse", async () => {
    render.mockRejectedValueOnce(new Error("Parse error"));
    const el = host(FENCE);
    await renderMermaidBlocks(el, "dark");
    expect(el.querySelector(".mmd")).toBeNull();
    expect(el.querySelector("code.language-mermaid")).not.toBeNull();
  });

  it("re-renders an existing diagram when the theme changes, and only then", async () => {
    const el = host(FENCE);
    await renderMermaidBlocks(el, "dark");
    expect(render).toHaveBeenCalledTimes(1);

    // same theme again → no work
    await renderMermaidBlocks(el, "dark");
    expect(render).toHaveBeenCalledTimes(1);

    // theme toggle → re-render from the stored source
    await renderMermaidBlocks(el, "light");
    expect(render).toHaveBeenCalledTimes(2);
    expect(render.mock.calls[1][1]).toBe("graph TD; A-->B;");
    expect(initialize).toHaveBeenLastCalledWith(expect.objectContaining({ theme: "neutral" }));
    const box = el.querySelector<HTMLElement>(".mmd")!;
    expect(box.dataset["mmdTheme"]).toBe("light");
  });
});
