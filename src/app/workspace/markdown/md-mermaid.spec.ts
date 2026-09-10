import { beforeEach, describe, expect, it, vi } from "vitest";

import { renderMermaidBlocks } from "./md-mermaid";

const { initialize, parse, render } = vi.hoisted(() => ({ initialize: vi.fn(), parse: vi.fn(), render: vi.fn() }));
vi.mock("mermaid", () => ({ default: { initialize, parse, render } }));

/** marked-mermaid's output for a ```mermaid fence. */
const SRC = "graph TD; A-->B;";
const FENCE = `<div class="md-box diagram pending"><pre><code class="language-mermaid">graph TD; A--&gt;B;</code></pre></div>`;

function host(html: string): HTMLElement {
  const el = document.createElement("div");
  el.innerHTML = html;
  document.body.appendChild(el);
  return el;
}

describe("renderMermaidBlocks", () => {
  beforeEach(() => {
    initialize.mockClear();
    parse.mockReset();
    render.mockReset();
    parse.mockResolvedValue({ diagramType: "flowchart" });
    render.mockResolvedValue({ svg: '<svg class="mmd-out"></svg>' });
    document.body.innerHTML = "";
  });

  it("replaces a pending placeholder with the design's diagram box, keeping source + theme in dataset", async () => {
    const el = host(`<h1>doc</h1>${FENCE}`);
    await renderMermaidBlocks(el, "dark");

    expect(parse).toHaveBeenCalledWith(SRC, { suppressErrors: false });
    expect(render).toHaveBeenCalledTimes(1);
    expect(render.mock.calls[0][1]).toBe(SRC);
    expect(initialize).toHaveBeenCalledWith(expect.objectContaining({ theme: "dark", securityLevel: "strict" }));

    const box = el.querySelector<HTMLElement>(".md-box.diagram")!;
    expect(box).not.toBeNull();
    expect(box.classList.contains("pending")).toBe(false);
    expect(box.querySelector(".box-scroll.mmd > svg.mmd-out")).not.toBeNull();
    // design toolbar: copy · full · | · theme
    const acts = Array.from(box.querySelectorAll<HTMLElement>(".box-tb [data-act]")).map((b) => b.dataset["act"]);
    expect(acts).toEqual(["copy", "full", "theme"]);
    expect(box.dataset["mmdSrc"]).toBe(SRC);
    expect(box.dataset["mmdTheme"]).toBe("dark");
    expect(el.querySelector("code.language-mermaid")).toBeNull();
    expect(el.querySelector("h1")).not.toBeNull(); // rest of the doc untouched
  });

  it("does not load mermaid when the document has no mermaid blocks", async () => {
    const el = host(`<div class="md-box fence"><pre><code class="language-ts">const x = 1;</code></pre></div>`);
    await renderMermaidBlocks(el, "dark");
    expect(initialize).not.toHaveBeenCalled();
    expect(parse).not.toHaveBeenCalled();
    expect(el.querySelector("code.language-ts")).not.toBeNull();
  });

  it("swaps a failing diagram for the design's error box: message, dimmed caret, collapsed source", async () => {
    parse.mockRejectedValueOnce(new Error("Parse error on line 1:\n...graph TD; A-->B;\n-----------^\nExpecting 'SEMI', got 'PIPE'"));
    const el = host(`<h1>doc</h1>${FENCE}<p>after</p>`);
    await renderMermaidBlocks(el, "dark");

    expect(render).not.toHaveBeenCalled();
    expect(el.querySelector(".md-box.diagram")).toBeNull();
    const box = el.querySelector<HTMLElement>(".md-box.error")!;
    expect(box).not.toBeNull();
    expect(box.querySelector(".hd .t")?.textContent).toBe("Diagram failed to render");
    expect(box.querySelector(".hd .s")?.textContent).toBe("mermaid · the rest of this document is unaffected");
    expect(box.querySelector(".hd svg.icon")).not.toBeNull();
    expect(box.querySelector("pre .m")?.textContent).toBe("Parse error on line 1:");
    expect(box.querySelector("pre .c")?.textContent).toBe("-----------^");
    const hd = box.querySelector<HTMLElement>(".fence-hd")!;
    expect(hd.dataset["act"]).toBe("toggle-src");
    expect(hd.querySelector(".chip.mono")?.textContent).toBe("mermaid");
    expect(hd.querySelector(".tnum.mono")?.textContent).toBe("1 lines");
    expect(hd.querySelector<HTMLElement>(".right")?.dataset["act"]).toBe("copy");
    const src = box.querySelector<HTMLElement>("pre.src")!;
    expect(src.hidden).toBe(true);
    expect(src.textContent).toBe(SRC);
    expect(box.dataset["mmdSrc"]).toBe(SRC);
    // the page around it is intact, and the box sits where the fence was
    expect(el.children[1]).toBe(box);
    expect(el.querySelector("p")?.textContent).toBe("after");
  });

  it("re-renders an existing diagram when the theme changes, and only then; error boxes are final", async () => {
    parse.mockRejectedValueOnce(new Error("Parse error"));
    const el = host(`${FENCE}${FENCE}`);
    await renderMermaidBlocks(el, "dark");
    expect(render).toHaveBeenCalledTimes(1);
    expect(el.querySelector(".md-box.error")).not.toBeNull();

    // same theme again → no work
    await renderMermaidBlocks(el, "dark");
    expect(render).toHaveBeenCalledTimes(1);

    // theme toggle → re-render the diagram from the stored source, not the error box
    await renderMermaidBlocks(el, "light");
    expect(render).toHaveBeenCalledTimes(2);
    expect(render.mock.calls[1][1]).toBe(SRC);
    expect(initialize).toHaveBeenLastCalledWith(expect.objectContaining({ theme: "neutral" }));
    expect(el.querySelector<HTMLElement>(".md-box.diagram")!.dataset["mmdTheme"]).toBe("light");
    expect(el.querySelectorAll(".md-box.error").length).toBe(1);
  });
});
