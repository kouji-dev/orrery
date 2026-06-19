import { DiffHunk, DiffLine } from "../../models";

export type Row =
  | { type: "hunk"; meta: string }
  | { type: "code"; k: "+" | "-" | " "; n: number; s: string; side: "old" | "new" | "file" };

/** Split into lines without a trailing empty element for a final newline. */
function splitLines(s: string): string[] {
  if (!s.length) return [];
  return s.replace(/\n$/, "").split("\n");
}

interface Op {
  k: "+" | "-" | " ";
  s: string;
}

/** Classic LCS over lines → an op list (equal / deleted / added). */
function lcsOps(a: string[], b: string[]): Op[] {
  const m = a.length, n = b.length;
  // dp[i][j] = LCS length of a[i..m), b[j..n) (suffixes, filled i from m-1 down to 0)
  const dp: number[][] = Array.from({ length: m + 1 }, () => new Array(n + 1).fill(0));
  for (let i = m - 1; i >= 0; i--)
    for (let j = n - 1; j >= 0; j--)
      dp[i][j] = a[i] === b[j] ? dp[i + 1][j + 1] + 1 : Math.max(dp[i + 1][j], dp[i][j + 1]);
  const ops: Op[] = [];
  let i = 0, j = 0;
  while (i < m && j < n) {
    if (a[i] === b[j]) {
      ops.push({ k: " ", s: a[i] });
      i++;
      j++;
    } else if (dp[i + 1][j] >= dp[i][j + 1]) {
      ops.push({ k: "-", s: a[i] });
      i++;
    } else {
      ops.push({ k: "+", s: b[j] });
      j++;
    }
  }
  while (i < m) { ops.push({ k: "-", s: a[i] }); i++; }
  while (j < n) { ops.push({ k: "+", s: b[j] }); j++; }
  return ops;
}

/**
 * Assign side line numbers to each op. `-` lines consume old-side numbers;
 * `+` lines consume new-side numbers; ` ` (context) lines consume both.
 */
interface NumberedOp extends Op {
  oldN: number; // old-side line number (meaningful for k==="-" and k===" ")
  newN: number; // new-side line number (meaningful for k==="+" and k===" ")
}

function numberOps(ops: Op[]): NumberedOp[] {
  let oldN = 0, newN = 0;
  return ops.map((op) => {
    if (op.k === " ") { oldN++; newN++; return { ...op, oldN, newN }; }
    if (op.k === "-") { oldN++; return { ...op, oldN, newN }; }
    // k === "+"
    newN++;
    return { ...op, oldN, newN };
  });
}

/** Build unified hunks (with `context` equal lines around changes) from old/new text. */
export function diffToHunks(oldText: string, newText: string, context = 3): DiffHunk[] {
  const a = splitLines(oldText);
  const b = splitLines(newText);
  const numbered = numberOps(lcsOps(a, b));

  const hunks: DiffHunk[] = [];

  // We'll collect hunk "windows": each window is a run of changed ops plus up
  // to `context` equal ops on each side. We build them by scanning for changes
  // and merging windows whose context windows overlap.

  // First pass: find indices of changed ops.
  const changeIdxs = numbered.reduce<number[]>((acc, op, idx) => {
    if (op.k !== " ") acc.push(idx);
    return acc;
  }, []);

  if (changeIdxs.length === 0) return [];

  // Group change indices into "windows" [start, end] in numbered[] coords.
  const windows: Array<[number, number]> = [];
  let winStart = Math.max(0, changeIdxs[0] - context);
  let winEnd = Math.min(numbered.length - 1, changeIdxs[0] + context);

  for (let ci = 1; ci < changeIdxs.length; ci++) {
    const lo = Math.max(0, changeIdxs[ci] - context);
    const hi = Math.min(numbered.length - 1, changeIdxs[ci] + context);
    if (lo <= winEnd + 1) {
      // Overlapping or adjacent windows — extend.
      winEnd = Math.max(winEnd, hi);
    } else {
      windows.push([winStart, winEnd]);
      winStart = lo;
      winEnd = hi;
    }
  }
  windows.push([winStart, winEnd]);

  // Second pass: build DiffHunk for each window.
  for (const [start, end] of windows) {
    const lines: DiffLine[] = [];
    for (let idx = start; idx <= end; idx++) {
      const op = numbered[idx];
      if (op.k === " ") {
        lines.push({ k: " ", n: op.newN, s: op.s });
      } else if (op.k === "-") {
        lines.push({ k: "-", n: op.oldN, s: op.s });
      } else {
        lines.push({ k: "+", n: op.newN, s: op.s });
      }
    }

    // Count old-side and new-side lines in this hunk.
    const oLines = lines.filter((l) => l.k !== "+");
    const nLines = lines.filter((l) => l.k !== "-");
    const oCount = oLines.length;
    const nCount = nLines.length;

    // Hunk header: unified diff format.
    // `-oStart,oCount +nStart,nCount`
    // For a new file (oCount===0): `-0,0 +1,N`.
    // oStart is the first old-side line number; nStart is the first new-side.
    let oStart: number;
    let nStart: number;
    if (oCount === 0) {
      oStart = 0;
    } else {
      oStart = oLines[0].n;
    }
    if (nCount === 0) {
      nStart = 0;
    } else {
      nStart = nLines[0].n;
    }

    const meta = `@@ -${oStart},${oCount} +${nStart},${nCount} @@`;
    hunks.push({ meta, lines });
  }

  return hunks;
}

export function diffToRows(hunks: DiffHunk[]): Row[] {
  const rows: Row[] = [];
  for (const h of hunks) {
    rows.push({ type: "hunk", meta: h.meta });
    for (const ln of h.lines) {
      rows.push({
        type: "code",
        k: ln.k,
        n: ln.n,
        s: ln.s,
        side: ln.k === "-" ? "old" : "new",
      });
    }
  }
  return rows;
}

export function fileToRows(text: string): Row[] {
  return splitLines(text).map((s, i) => ({
    type: "code" as const,
    k: " " as const,
    n: i + 1,
    s,
    side: "file" as const,
  }));
}
