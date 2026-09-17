import { svgIcon } from "./md-mermaid";

/** The slice of a review comment the highlight needs — kept structural so
 *  the helper stays free of the store (and trivially testable). */
export interface HighlightTarget {
  id: string;
  snippet: string;
  fromLine: number;
  toLine: number;
}

/** Design `applyHighlight`: find the comment's snippet inside the first
 *  block whose whitespace-collapsed text contains it, wrap exactly that text
 *  range in `<span class="md-hl" data-id>` and drop a `.md-mark` anchor after
 *  it. Direct DOM after [innerHTML] — the sanitizer only sees the HTML string,
 *  so `dataset` set here survives. Silently a no-op when the text is gone
 *  (the source moved on); the comment still lives in the queue. */
export function applyHighlight(root: HTMLElement, c: HighlightTarget): void {
  const want = (c.snippet || "").replace(/\s+/g, " ").trim();
  if (!want) return;
  const blocks = root.querySelectorAll<HTMLElement>("p, li, h1, h2, h3, td, th, blockquote");
  for (const b of blocks) {
    const at = (b.textContent ?? "").replace(/\s+/g, " ").indexOf(want);
    if (at < 0) continue;
    const range = document.createRange();
    let acc = "";
    let started = false;
    const walker = document.createTreeWalker(b, NodeFilter.SHOW_TEXT);
    let n: Node | null;
    while ((n = walker.nextNode())) {
      const t = n.nodeValue ?? "";
      for (let k = 0; k < t.length; k++) {
        const ch = /\s/.test(t[k]) ? " " : t[k];
        if (ch === " " && acc.endsWith(" ")) continue;
        if (!started && acc.length === at) {
          range.setStart(n, k);
          started = true;
        }
        acc += ch;
        if (started && acc.length === at + want.length) {
          range.setEnd(n, k + 1);
          const span = document.createElement("span");
          span.className = "md-hl";
          span.dataset["id"] = c.id;
          try {
            span.appendChild(range.extractContents());
            range.insertNode(span);
          } catch {
            return;
          }
          const mk = document.createElement("button");
          mk.className = "md-mark";
          mk.dataset["id"] = c.id;
          mk.title = "Comment · lines " + c.fromLine + (c.toLine !== c.fromLine ? "–" + c.toLine : "");
          mk.setAttribute("aria-label", "Comment");
          mk.innerHTML = svgIcon("chat", 9);
          span.after(mk);
          return;
        }
      }
    }
    return;
  }
}

/** Unwrap every `.md-sel` (the in-progress selection wrap) in place — used
 *  when a render pass invalidates a pending selection without re-rendering. */
export function unwrapSelection(root: HTMLElement): void {
  root.querySelectorAll<HTMLElement>("span.md-sel").forEach((s) => {
    const parent = s.parentNode;
    if (!parent) return;
    while (s.firstChild) parent.insertBefore(s.firstChild, s);
    parent.removeChild(s);
    parent.normalize();
  });
}
