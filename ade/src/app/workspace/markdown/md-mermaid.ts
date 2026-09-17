/**
 * Mermaid hydration for the markdown preview.
 *
 * marked (see marked-mermaid.ts) renders a ```mermaid fence as a
 * `.md-box.diagram.pending` placeholder holding the escaped source. Angular's
 * [innerHTML] sanitizer would strip mermaid's SVG output (style elements,
 * foreignObject) — and it also drops data-* attributes — so diagrams are built
 * by post-processing the live preview DOM: each placeholder is replaced by the
 * design's diagram box (toolbar over the SVG) or, when mermaid rejects the
 * source, the design's error box that shows the parse message and keeps the
 * source one click away. Source + theme ride on `el.dataset` (set from JS,
 * never through the sanitizer) so switching the app theme re-renders the
 * diagram in place without re-parsing the markdown, and the toolbar's Copy
 * reads the source back.
 *
 * mermaid (~2.5MB) loads as a lazy chunk on the first document that needs it —
 * same pattern as the Monaco loader, including retry after a failed load.
 */

import { escHtml } from "./marked-mermaid";

type MermaidApi = typeof import("mermaid").default;

let mermaidLoad: Promise<MermaidApi> | null = null;

function loadMermaid(): Promise<MermaidApi> {
  mermaidLoad ??= import("mermaid").then(
    (m) => m.default,
    (e) => {
      mermaidLoad = null; // allow a retry on the next render pass
      throw e;
    },
  );
  return mermaidLoad;
}

/** Inline icon paths from the design (orrery-v2 `MD_ICON`) — the boxes are
 *  built outside Angular, so app-icon is not available here. */
export const MD_ICON: Record<string, string> = {
  alert: "M12 3a9 9 0 100 18 9 9 0 000-18zM12 8v5M12 16h.01",
  dup: "M9 9h10v10H9zM5 15V5h10",
  maximize: "M5 9V5h4M19 9V5h-4M5 15v4h4M19 15v4h-4",
  chevron: "M9 6l6 6-6 6",
  chat: "M4 5h16v10H9l-4 4V5z",
};

export const svgIcon = (n: keyof typeof MD_ICON, w = 12, cls = ""): string =>
  `<svg class="icon ${cls}" width="${w}" height="${w}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" aria-hidden="true"><path d="${MD_ICON[n]}"/></svg>`;

const lineCount = (src: string): number => src.split("\n").length;

/** Design `mermaidBlock` success branch: toolbar over the scrolling SVG. The
 *  `.mmd` class stays on the scroll container — the e2e locks `.mmd svg`. */
function diagramBox(svg: string, src: string, theme: string): HTMLElement {
  const box = document.createElement("div");
  box.className = "md-box diagram";
  box.innerHTML =
    `<div class="box-tb" role="toolbar" aria-label="Diagram">` +
    `<button data-act="copy">${svgIcon("dup")}Copy source</button>` +
    `<button data-act="full">${svgIcon("maximize")}Open full size</button>` +
    `</div><div class="box-scroll mmd">${svg}</div>`;
  box.dataset["mmdSrc"] = src;
  box.dataset["mmdTheme"] = theme;
  return box;
}

/** Design `mermaidBlock` failure branch: the block failed, not the page. The
 *  first message line is the headline; mermaid's caret lines (`---^`) are
 *  dimmed; the source sits collapsed under a toggle row. */
function errorBox(err: unknown, src: string): HTMLElement {
  const msg = escHtml(err instanceof Error ? err.message : String(err)).split("\n");
  const first = msg.shift() ?? "Parse error";
  const body = msg.map((l) => (/^-+\^$/.test(l) ? `<span class="c">${l}</span>` : l)).join("\n");
  const box = document.createElement("div");
  box.className = "md-box error";
  box.setAttribute("role", "group");
  box.setAttribute("aria-label", "Diagram failed to render");
  box.innerHTML =
    `<div class="hd">${svgIcon("alert", 15)}<div><div class="t">Diagram failed to render</div>` +
    `<div class="s">mermaid · the rest of this document is unaffected</div></div></div>` +
    `<pre><span class="m">${first}</span>\n${body}</pre>` +
    `<div class="fence-hd" data-act="toggle-src">${svgIcon("chevron", 11, "chev")}Source` +
    `<span class="chip mono">mermaid</span><span class="tnum mono">${lineCount(src)} lines</span>` +
    `<span class="right" data-act="copy">${svgIcon("dup")}Copy source</span></div>` +
    `<pre class="src" hidden>${escHtml(src)}</pre>`;
  box.dataset["mmdSrc"] = src;
  return box;
}

let seq = 0;

/**
 * Hydrate every mermaid block inside `host` for the given theme: pending
 * placeholders from marked, plus already-rendered diagrams whose theme no
 * longer matches (the app theme changed). Error boxes are final — a parse
 * failure does not depend on the theme, so they are never retried.
 */
export async function renderMermaidBlocks(host: HTMLElement, theme: "dark" | "light"): Promise<void> {
  const fresh = Array.from(host.querySelectorAll<HTMLElement>(".md-box.diagram.pending"));
  const stale = Array.from(host.querySelectorAll<HTMLElement>(".md-box.diagram:not(.pending)")).filter(
    (el) => el.dataset["mmdTheme"] !== theme,
  );
  if (fresh.length === 0 && stale.length === 0) return;

  const mermaid = await loadMermaid();
  mermaid.initialize({
    startOnLoad: false,
    securityLevel: "strict",
    theme: theme === "dark" ? "dark" : "neutral",
  });

  const jobs = [
    ...fresh.map((box) => ({ target: box, src: box.querySelector("code")?.textContent ?? "" })),
    ...stale.map((el) => ({ target: el, src: el.dataset["mmdSrc"] ?? "" })),
  ];
  for (const { target, src } of jobs) {
    if (!target?.isConnected) continue;
    const id = `mmd-svg-${++seq}`;
    try {
      // parse first: a clean rejection with mermaid's line/caret message, and
      // no half-built render scratch to clean up for the common failure path
      await mermaid.parse(src, { suppressErrors: false });
      const { svg } = await mermaid.render(id, src);
      target.replaceWith(diagramBox(svg, src, theme));
    } catch (e) {
      // mermaid can leave its scratch nodes in <body> on a render error
      document.getElementById(id)?.remove();
      document.getElementById("d" + id)?.remove();
      target.replaceWith(errorBox(e, src));
    }
  }
}
