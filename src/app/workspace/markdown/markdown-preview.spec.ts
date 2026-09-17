import { signal } from "@angular/core";
import { afterEach, describe, expect, it } from "vitest";

import { svgIcon } from "./md-mermaid";
import { diagramSvg, MarkdownPreviewComponent, resolveRelative, slug } from "./markdown-preview.component";

// Pure helpers behind link following in the preview. The component itself is
// exercised end-to-end (e2e/md-preview-links.spec.ts) — signal inputs do not
// render under vitest's JIT (NG0950), so only the path/slug math lives here.
describe("resolveRelative", () => {
  it("joins onto the current file's folder and collapses . and ..", () => {
    expect(resolveRelative("docs/guide", "./notes/design.md")).toBe("docs/guide/notes/design.md");
    expect(resolveRelative("docs/guide", "../README.md")).toBe("docs/README.md");
    expect(resolveRelative("docs/guide", "../../CHANGELOG.md")).toBe("CHANGELOG.md");
    expect(resolveRelative("", "README.md")).toBe("README.md");
  });

  it("treats a leading slash as the worktree root, and never escapes above it", () => {
    expect(resolveRelative("docs/guide", "/src/main.ts")).toBe("src/main.ts");
    expect(resolveRelative("docs", "../../../etc")).toBe("etc");
  });
});

describe("slug", () => {
  it("matches GitHub-style heading anchors", () => {
    expect(slug("Second part")).toBe("second-part");
    expect(slug("  Retry: design & limits!  ")).toBe("retry-design--limits");
    expect(slug("API v2.0")).toBe("api-v20");
  });
});

/**
 * "Open full size" regression: `.md-box.diagram` opens with a toolbar whose
 * buttons carry 12px `<svg>` icons, so the first `<svg>` in the box is the Copy
 * icon, not the drawing. The scrim used to show that glyph, which read as the
 * button doing nothing.
 */
const boxes: HTMLElement[] = [];
afterEach(() => {
  boxes.splice(0).forEach((b) => b.remove());
});

function diagramBox(withDrawing = true): HTMLElement {
  const box = document.createElement("div");
  box.className = "md-box diagram";
  box.innerHTML =
    `<div class="box-tb" role="toolbar" aria-label="Diagram">` +
    `<button data-act="copy">${svgIcon("dup")}Copy source</button>` +
    `<button data-act="full">${svgIcon("maximize")}Open full size</button>` +
    `<button data-act="theme">${svgIcon("palette")}Theme</button>` +
    `</div>` +
    (withDrawing
      ? `<div class="box-scroll mmd"><svg id="mmd-svg-1" width="100%"><g class="node"></g></svg></div>`
      : `<div class="box-scroll mmd"></div>`);
  box.dataset["mmdSrc"] = "graph TD; A-->B;";
  document.body.appendChild(box);
  boxes.push(box);
  return box;
}

/** The component without its DI tree: onClick's "full" branch touches only
 *  `full`, so the real method can run over a real click event. */
function handler(): { comp: MarkdownPreviewComponent; full: ReturnType<typeof signal<SVGElement | null>> } {
  const comp = Object.create(MarkdownPreviewComponent.prototype) as MarkdownPreviewComponent;
  const full = signal<SVGElement | null>(null);
  (comp as unknown as { full: unknown }).full = full;
  (comp as unknown as { ui: unknown }).ui = { flash: () => {} };
  return { comp, full };
}

function clickFull(box: HTMLElement, comp: MarkdownPreviewComponent): void {
  const listener = (e: Event): void => comp.onClick(e as MouseEvent);
  document.addEventListener("click", listener);
  try {
    box.querySelector<HTMLElement>('[data-act="full"]')!.dispatchEvent(new MouseEvent("click", { bubbles: true }));
  } finally {
    document.removeEventListener("click", listener);
  }
}

describe("diagramSvg / Open full size", () => {
  it("picks the diagram SVG, never a toolbar icon", () => {
    const box = diagramBox();
    const drawing = box.querySelector(".box-scroll.mmd svg");
    const icon = box.querySelector("svg");

    // the bug, pinned: document order puts an icon first
    expect(icon).not.toBe(drawing);
    expect(icon!.classList.contains("icon")).toBe(true);
    expect(diagramSvg(box)).toBe(drawing);
  });

  it("clicking the toolbar button opens the drawing, not the 12px icon", () => {
    const box = diagramBox();
    const { comp, full } = handler();

    clickFull(box, comp);

    const drawing = box.querySelector(".box-scroll.mmd svg");
    expect(full()).toBe(drawing);
    expect(full()!.getAttribute("id")).toBe("mmd-svg-1");
    expect(full()!.classList.contains("icon")).toBe(false);
  });

  it("opens no scrim when the box has no drawing", () => {
    const { comp, full } = handler();

    clickFull(diagramBox(false), comp);

    expect(full()).toBeNull();
    expect(diagramSvg(null)).toBeNull();
  });
});
