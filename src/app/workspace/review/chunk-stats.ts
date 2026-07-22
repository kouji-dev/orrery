import type { Text } from "@codemirror/state";
import type { Chunk } from "@codemirror/merge";

/**
 * Header stats derived from the ONE diff the unified merge view computed —
 * replaces the old triple bookkeeping (LCS rows vs multiset shared-line counts
 * vs whole-file hunk header) with a single source of truth.
 *
 * Pure module (type-only CM imports) so vitest can exercise it against the
 * local @codemirror packages without touching the esm.sh runtime loader.
 */
export interface DiffStats {
  add: number; // added (new-side) line count
  del: number; // deleted (old-side) line count
  hunks: number; // number of changed chunks
  hunk: string; // "@@ -a,n +b,m @@" label for the first chunk
}

/** Lines covered by one side of a chunk. Chunk `to` is one past the end of the
 *  last changed line (may exceed doc length); an empty side has to === from. */
function sideLines(doc: Text, from: number, to: number): number {
  if (to <= from) return 0;
  const end = Math.min(to - 1, doc.length);
  return doc.lineAt(end).number - doc.lineAt(from).number + 1;
}

export function chunkStats(chunks: readonly Chunk[], oldDoc: Text, newDoc: Text): DiffStats {
  // A truly empty side means added/deleted file — CM's Text still reports one
  // (empty) line there, so bypass the chunk walk for exact +N / −N counts.
  const oldEmpty = oldDoc.length === 0;
  const newEmpty = newDoc.length === 0;
  if (oldEmpty && newEmpty) return { add: 0, del: 0, hunks: 0, hunk: "@@ -0,0 +0,0 @@" };
  if (oldEmpty) return { add: newDoc.lines, del: 0, hunks: 1, hunk: `@@ -0,0 +1,${newDoc.lines} @@` };
  if (newEmpty) return { add: 0, del: oldDoc.lines, hunks: 1, hunk: `@@ -1,${oldDoc.lines} +0,0 @@` };

  let add = 0;
  let del = 0;
  for (const c of chunks) {
    del += sideLines(oldDoc, c.fromA, c.toA);
    add += sideLines(newDoc, c.fromB, c.toB);
  }
  let hunk = "@@ -0,0 +0,0 @@";
  if (chunks.length) {
    const c = chunks[0];
    const aLines = sideLines(oldDoc, c.fromA, c.toA);
    const bLines = sideLines(newDoc, c.fromB, c.toB);
    const aStart = aLines ? oldDoc.lineAt(c.fromA).number : Math.max(0, oldDoc.lineAt(Math.min(c.fromA, oldDoc.length)).number - 1);
    const bStart = bLines ? newDoc.lineAt(c.fromB).number : Math.max(0, newDoc.lineAt(Math.min(c.fromB, newDoc.length)).number - 1);
    hunk = `@@ -${aStart},${aLines} +${bStart},${bLines} @@`;
  }
  return { add, del, hunks: chunks.length, hunk };
}
