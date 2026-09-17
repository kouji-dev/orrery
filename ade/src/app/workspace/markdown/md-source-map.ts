import type { Token } from "marked";
import { createMarked } from "./marked.config";

/** 1-based inclusive source-line range of one top-level block. */
export interface BlockSpan {
  from: number;
  to: number;
  /** marked token type — `table` rows get per-row stamps in {@link stampBlocks}. */
  type: string;
}

/**
 * Source-line spans for the rendered document's top-level blocks, IN ORDER,
 * one per element child of the `.rte-view` host. Derived from the lexer's
 * token `raw` strings, which tile the source exactly — so counting newlines
 * walks a cursor through the file without a second parser.
 *
 * Dropped tokens: `space` (blank lines render nothing) and `def` (a link
 * reference definition renders nothing — keeping it would offset every later
 * block by one element). Consecutive top-level `text` tokens merge into one
 * span because the parser folds them into a single <p>.
 */
export function blockSpans(src: string): BlockSpan[] {
  return spansOf(createMarked().lexer(src));
}

export function spansOf(tokens: Token[]): BlockSpan[] {
  const out: BlockSpan[] = [];
  let cursor = 1;
  let prevText = false;
  for (const t of tokens) {
    const raw = t.raw ?? "";
    const nl = (raw.match(/\n/g) ?? []).length;
    const emits =
      t.type !== "space" &&
      t.type !== "def" &&
      // an HTML block that is only a comment yields a comment node, not an element
      !(t.type === "html" && !/<[a-zA-Z]/.test(raw));
    if (emits) {
      // trailing blank lines belong to the token's raw but not to the block —
      // `to` is the last line with content, which is what a comment anchors to
      const body = raw.replace(/\n+$/, "");
      const span = body ? (body.match(/\n/g) ?? []).length + 1 : 1;
      const to = cursor + span - 1;
      if (t.type === "text" && prevText) out[out.length - 1].to = to;
      else out.push({ from: cursor, to, type: t.type });
    }
    prevText = emits && t.type === "text";
    cursor += nl;
  }
  return out;
}

/**
 * Stamp `data-l0` / `data-l1` (the design's attribute names) on the host's
 * element children by index. Idempotent — re-running after a mermaid box was
 * swapped in (replaceWith keeps the position) just rewrites the same values.
 * Table rows get their own line so a selection inside a cell anchors to the
 * row: header line, delimiter line, then row i → from + 2 + i.
 */
export function stampBlocks(host: HTMLElement, spans: BlockSpan[]): void {
  const kids = host.children;
  spans.forEach((s, i) => {
    const el = kids[i] as HTMLElement | undefined;
    if (!el) return;
    el.dataset["l0"] = String(s.from);
    el.dataset["l1"] = String(s.to);
    if (s.type === "table") {
      el.querySelectorAll<HTMLElement>("tbody tr").forEach((tr, r) => {
        tr.dataset["l0"] = tr.dataset["l1"] = String(s.from + 2 + r);
      });
    }
  });
}

/** Source lines a DOM selection covers — the design's `blockOf`: the nearest
 *  stamped ancestor of each end, ordered. Unstamped ends fall back to line 1
 *  (start) / the start line (end). */
export function rangeToLines(_host: HTMLElement, range: Range): { from: number; to: number } {
  const blockOf = (n: Node): HTMLElement | null => {
    const el = n.nodeType === Node.TEXT_NODE ? n.parentElement : (n as Element);
    return el?.closest<HTMLElement>("[data-l0]") ?? null;
  };
  const b1 = blockOf(range.startContainer);
  const b2 = blockOf(range.endContainer);
  const f = b1 ? Number(b1.dataset["l0"]) : 1;
  const t = b2 ? Number(b2.dataset["l1"]) : f;
  return { from: Math.min(f, t), to: Math.max(f, t) };
}
