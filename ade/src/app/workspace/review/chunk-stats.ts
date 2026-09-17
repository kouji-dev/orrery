/**
 * Header stats derived from the ONE diff the Monaco diff editor computed —
 * single source of truth for the diff header (+N / −N / hunk label).
 *
 * Pure module: `LineChangeLite` is the slice of Monaco's `ILineChange` the
 * math needs, so vitest exercises this with plain objects (no editor).
 */
export interface DiffStats {
  add: number; // added (new-side) line count
  del: number; // deleted (old-side) line count
  hunks: number; // number of changed chunks
  hunk: string; // "@@ -a,n +b,m @@" label for the first chunk
}

/** Monaco `ILineChange` convention: an END of 0 means "no lines on this side",
 *  with START then naming the line BEFORE the insertion/deletion point. */
export interface LineChangeLite {
  originalStartLineNumber: number;
  originalEndLineNumber: number;
  modifiedStartLineNumber: number;
  modifiedEndLineNumber: number;
}

function lineCount(s: string): number {
  let n = 1;
  for (let i = 0; i < s.length; i++) if (s.charCodeAt(i) === 10) n++;
  return n;
}

function sideLines(start: number, end: number): number {
  return end >= start && end > 0 ? end - start + 1 : 0;
}

export function lineChangeStats(
  changes: readonly LineChangeLite[],
  oldText: string,
  newText: string,
): DiffStats {
  // A truly empty side means added/deleted file — bypass the change walk for
  // exact +N / −N counts (Monaco still reports one empty line there).
  const oldEmpty = oldText.length === 0;
  const newEmpty = newText.length === 0;
  if (oldEmpty && newEmpty) return { add: 0, del: 0, hunks: 0, hunk: "@@ -0,0 +0,0 @@" };
  if (oldEmpty) {
    const n = lineCount(newText);
    return { add: n, del: 0, hunks: 1, hunk: `@@ -0,0 +1,${n} @@` };
  }
  if (newEmpty) {
    const n = lineCount(oldText);
    return { add: 0, del: n, hunks: 1, hunk: `@@ -1,${n} +0,0 @@` };
  }

  let add = 0;
  let del = 0;
  for (const c of changes) {
    del += sideLines(c.originalStartLineNumber, c.originalEndLineNumber);
    add += sideLines(c.modifiedStartLineNumber, c.modifiedEndLineNumber);
  }
  let hunk = "@@ -0,0 +0,0 @@";
  if (changes.length) {
    const c = changes[0];
    const aLines = sideLines(c.originalStartLineNumber, c.originalEndLineNumber);
    const bLines = sideLines(c.modifiedStartLineNumber, c.modifiedEndLineNumber);
    // an empty side's START already names the line before the change point —
    // exactly the "@@" convention — so it is used as-is when the count is 0.
    hunk = `@@ -${c.originalStartLineNumber},${aLines} +${c.modifiedStartLineNumber},${bLines} @@`;
  }
  return { add, del, hunks: changes.length, hunk };
}
