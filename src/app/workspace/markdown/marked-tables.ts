import type { MarkedExtension } from "marked";

/**
 * marked extension for the markdown preview: synthesize the `|---|` separator
 * row that LLM-written tables routinely omit.
 *
 * WHY: GFM only recognises a table when the second line is a delimiter row.
 * Without it marked emits ONE paragraph of literal pipes — the whole table
 * collapses into prose. A `hooks.preprocess` pass is the simplest, safest fix:
 * it runs on the raw source before the lexer, so the tokenizer, the renderer's
 * `.md-box.table` wrapper and the source map all see a well-formed table.
 *
 * Rule: a run of ≥2 consecutive lines that each start AND end with `|` (after
 * trim), all with the same cell count ≥2, whose second line is NOT already a
 * delimiter row → insert `| --- | --- | …` after the first line. Lines inside a
 * ``` / ~~~ fence are never touched. Already-valid tables are left alone, so
 * the pass is idempotent.
 */

/** Fence opener: ≤3-space indent, ``` or ~~~ — captures the marker. */
const FENCE_OPEN = /^ {0,3}(`{3,}|~{3,})/;
/** One delimiter cell: `---`, `:--`, `--:`, `:-:` (spaces around allowed). */
const DELIM_CELL = /^\s*:?-+:?\s*$/;

const isPipeRow = (line: string): boolean => {
  const t = line.trim();
  return t.length >= 2 && t.startsWith("|") && t.endsWith("|");
};

/** Cells of a pipe row — outer pipes stripped, `\|` does not split. */
const cells = (line: string): string[] => line.trim().slice(1, -1).split(/(?<!\\)\|/);

const isDelimiterRow = (line: string): boolean => isPipeRow(line) && cells(line).every((c) => DELIM_CELL.test(c));

/** Pure preprocess step — exported so the spec can assert idempotence directly. */
export function synthesizeTableSeparators(markdown: string): string {
  const lines = markdown.split("\n");
  const out: string[] = [];
  let fence: string | null = null;
  for (let i = 0; i < lines.length; i++) {
    const line = lines[i];
    out.push(line);
    if (fence) {
      // closer: same marker char, at least as many, nothing else once trimmed
      if (new RegExp(`^${fence[0]}{${fence.length},}$`).test(line.trim())) fence = null;
      continue;
    }
    const open = FENCE_OPEN.exec(line);
    if (open) {
      fence = open[1];
      continue;
    }
    // only the FIRST row of a pipe run may receive a separator
    if (!isPipeRow(line) || (i > 0 && isPipeRow(lines[i - 1]))) continue;
    let end = i + 1;
    while (end < lines.length && isPipeRow(lines[end])) end++;
    if (end - i < 2 || isDelimiterRow(lines[i + 1])) continue;
    const n = cells(line).length;
    if (n < 2) continue;
    let uniform = true;
    for (let k = i + 1; k < end && uniform; k++) uniform = cells(lines[k]).length === n;
    if (!uniform) continue;
    out.push(`|${" --- |".repeat(n)}`);
  }
  return out.join("\n");
}

export function tablesExtension(): MarkedExtension {
  return {
    hooks: {
      preprocess(markdown: string): string {
        return synthesizeTableSeparators(markdown);
      },
    },
  };
}
