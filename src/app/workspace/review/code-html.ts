import type { TokRun } from "../code-lang";
import type { DelMark, Seg } from "./unified-diff";

/** Minimal HTML-text escape (we only ever emit <span class="…"> wrappers). */
function esc(s: string): string {
  return s.replace(/[&<>]/g, (c) => (c === "&" ? "&amp;" : c === "<" ? "&lt;" : "&gt;"));
}

/** Escape for an attribute value (title tooltip). */
function escAttr(s: string): string {
  return esc(s).replace(/"/g, "&quot;");
}

/** Changed character ranges [from,to) of a line from its word-diff segments. */
export function segToRanges(seg?: Seg[]): Array<[number, number]> {
  if (!seg) return [];
  const out: Array<[number, number]> = [];
  let off = 0;
  for (const s of seg) {
    const len = s.s.length;
    if (s.c && len) out.push([off, off + len]);
    off += len;
  }
  return out;
}

/**
 * Render one code line to HTML, overlaying the intra-line change background
 * (`chgClass`) onto the syntax-highlighted token runs. Foreground = syntax token
 * color (tok-* class); background = the changed-span tint. Boundaries of BOTH
 * are honored, so a change can start mid-token and span multiple tokens.
 *
 * `runs` null/empty → plain text (used before async highlight lands, or for
 * languages with no grammar); the word-diff overlay still applies.
 */
export function buildLineHtml(
  runs: TokRun[] | null,
  text: string,
  changes: Array<[number, number]>,
  chgClass: string,
): string {
  const parts: TokRun[] = runs && runs.length ? runs : [{ s: text, cls: "" }];
  let out = "";
  let off = 0;
  let ci = 0;
  const emit = (cls: string, chg: boolean, str: string): void => {
    if (!str) return;
    const c = [cls, chg ? chgClass : ""].filter(Boolean).join(" ");
    out += c ? `<span class="${c}">${esc(str)}</span>` : esc(str);
  };
  for (const run of parts) {
    const start = off;
    const end = off + run.s.length;
    off = end;
    let pos = start;
    while (pos < end) {
      while (ci < changes.length && changes[ci][1] <= pos) ci++;
      const ch = ci < changes.length ? changes[ci] : null;
      if (!ch || ch[0] >= end) {
        emit(run.cls, false, text.slice(pos, end));
        pos = end;
      } else {
        if (ch[0] > pos) {
          emit(run.cls, false, text.slice(pos, ch[0]));
          pos = ch[0];
        }
        const segEnd = Math.min(ch[1], end);
        emit(run.cls, true, text.slice(pos, segEnd));
        pos = segEnd;
      }
    }
  }
  return out;
}

/**
 * Render a NEW (added/modified) line for the single-panel diff: syntax token
 * foreground, a green overlay on the actually-added spans, and inline deletion
 * bars at the points where old text was removed. When `revealed`, each deletion
 * bar expands into the removed text shown in red instead of a thin bar.
 *
 * Per-character assembly — lines are short, and it keeps the 3-way merge (tokens
 * × add-ranges × zero-width deletions) obviously correct.
 */
export function buildNewLineHtml(
  runs: TokRun[] | null,
  text: string,
  addRanges: Array<[number, number]>,
  dels: DelMark[],
  revealed: boolean,
): string {
  const n = text.length;
  const cls: string[] = new Array(n).fill("");
  if (runs && runs.length) {
    let o = 0;
    for (const r of runs) {
      for (let k = 0; k < r.s.length && o + k < n; k++) cls[o + k] = r.cls;
      o += r.s.length;
    }
  }
  const add: boolean[] = new Array(n).fill(false);
  for (const [s, e] of addRanges) for (let k = s; k < e && k < n; k++) add[k] = true;
  const delAt = new Map<number, string>();
  for (const d of dels) delAt.set(d.at, (delAt.get(d.at) ?? "") + d.s);

  let out = "";
  const flushDel = (at: number): void => {
    const s = delAt.get(at);
    if (s === undefined) return;
    out += revealed
      ? `<span class="rc-chg-del rc-del-shown" title="click to hide">${esc(s)}</span>`
      : `<span class="rc-del-caret" title="removed: ${escAttr(s)}"></span>`;
  };
  const emit = (klass: string, changed: boolean, str: string): void => {
    const c = [klass, changed ? "rc-chg-add" : ""].filter(Boolean).join(" ");
    out += c ? `<span class="${c}">${esc(str)}</span>` : esc(str);
  };

  flushDel(0);
  let i = 0;
  while (i < n) {
    const c = cls[i];
    const a = add[i];
    let j = i + 1;
    while (j < n && cls[j] === c && add[j] === a && !delAt.has(j)) j++;
    emit(c, a, text.slice(i, j));
    i = j;
    flushDel(i);
  }
  return out;
}
