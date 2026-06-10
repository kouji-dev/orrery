import { appendPtyTail } from "../utils";

/**
 * Per-agent bounded buffer of RAW PTY chunks with a lazily folded tail.
 *
 * Why: eagerly folding every chunk through appendPtyTail costs O(tokens) per
 * flush for EVERY agent, but the folded tail is only ever read at process exit
 * (the "finished" notification detail) and by the needs-input heuristic for
 * un-hooked tools (gemini). Hook-driven tools (claude/codex/cursor) stream
 * megabytes that nobody reads until exit — so `push` just stores the raw
 * string (no parsing at all) and `tail` folds whatever is pending into the
 * cached lines on demand. Pending raw chunks ARE the dirty flag: a read with
 * nothing pending returns the cached fold untouched.
 *
 * The raw side is capped (default 64 KiU per agent — 64 * 1024 UTF-16 code
 * units, so the actual memory bound is ~2× that in bytes for strings held by
 * the JS engine). Oldest chunks are dropped past the cap so an unread agent
 * can stream forever without growing memory. appendPtyTail keeps at most 60
 * lines, so the most recent 64 KiU reproduces the same visible tail in
 * practice; folding stays chunk-by-chunk (not one joined string) to preserve
 * the eager pipeline's exact fold order.
 *
 * Surrogate-safe trim: when a single oversized chunk is sliced at the cap
 * boundary the implementation ensures the cut never falls between a surrogate
 * pair (U+D800–U+DFFF) and never leaves a partial leading ESC sequence at the
 * new start. The scan is bounded (≤ a small constant number of code units), so
 * the overall push path remains O(1)-ish.
 */
export interface PtyTailBuffer {
  /** Append a raw chunk — O(1), no parsing. Oldest raw code units drop past the cap. */
  push(id: string, chunk: string): void;
  /** The folded plain-text tail. Folds pending raw chunks first (then caches);
   *  returns the cached array — treat as read-only. [] for an unknown agent. */
  tail(id: string): string[];
  /** Drop ALL state (raw + folded) for an agent — on removal. */
  clear(id: string): void;
}

/** Default cap in UTF-16 code units (~64 KiU, ~128 KiB worst-case memory). */
const DEFAULT_CAP_UNITS = 64 * 1024;

/**
 * Advance `offset` past the end of an incomplete ANSI/VT escape sequence that
 * begins at `s[offset]`. Returns the adjusted offset.
 *
 * The caller guarantees that s[offset] === '\x1b'. If the escape looks
 * complete (e.g. ESC alone, or ESC followed by a character that is not '[',
 * '(' etc.) it is left as-is. Only incomplete CSI sequences (ESC [ … not yet
 * terminated by a final byte in 0x40-0x7E) are skipped. Scan is bounded to
 * avoid large strings: we look at most 64 code units forward.
 */
function skipIncompleteEsc(s: string, offset: number): number {
  if (s.charCodeAt(offset) !== 0x1b) return offset;
  const next = s.charCodeAt(offset + 1);
  // ESC [ — CSI sequence; final byte is 0x40–0x7E
  if (next === 0x5b /* '[' */) {
    const limit = Math.min(s.length, offset + 64);
    for (let i = offset + 2; i < limit; i++) {
      const c = s.charCodeAt(i);
      if (c >= 0x40 && c <= 0x7e) return offset; // sequence is complete — keep it
    }
    // Sequence is incomplete (no final byte in window) — skip past ESC
    return offset + 1;
  }
  // Single-character escapes (ESC alone, ESC followed by a non-[ byte) are
  // always "complete" from a splitting perspective — leave them intact.
  return offset;
}

/**
 * Given a string `s` and a desired cut point `cut` (code-unit index), return
 * a safe cut point that:
 *   1. Does not split a UTF-16 surrogate pair.
 *   2. Does not leave the remaining tail starting with an incomplete ESC
 *      sequence that was split by the cut.
 *
 * The returned value is always ≤ `cut`. Scan is bounded to a small constant.
 */
function safeCutPoint(s: string, cut: number): number {
  // 1. Back up if we'd split a surrogate pair: if the code unit at `cut` is a
  //    low surrogate (0xDC00–0xDFFF) then the high surrogate sits at cut-1 and
  //    we must move back by one.
  let pos = cut;
  const c = s.charCodeAt(pos);
  if (c >= 0xdc00 && c <= 0xdfff) pos -= 1;

  // 2. Scan backward (bounded) to see if an ESC sequence that started before
  //    `pos` extends into the tail portion. We look back up to 64 code units.
  const scanStart = Math.max(0, pos - 64);
  for (let i = pos - 1; i >= scanStart; i--) {
    if (s.charCodeAt(i) === 0x1b) {
      // There's an ESC at position i. Check whether the sequence it starts is
      // complete within s[i..pos-1] (i.e., entirely in the part we are
      // discarding). If the scan says it's incomplete the sequence bleeds into
      // the tail, so move pos forward past it to avoid a partial leading escape
      // in the retained slice.
      const adjusted = skipIncompleteEsc(s, i);
      if (adjusted > i) {
        // sequence was incomplete — the retained portion starts after the ESC
        pos = i + 1;
      }
      break; // only care about the last ESC before the cut
    }
  }

  return Math.max(0, pos);
}

export function createPtyTailBuffer(
  opts: { capBytes?: number; capUnits?: number; fold?: typeof appendPtyTail } = {},
): PtyTailBuffer {
  // Accept the legacy capBytes name (treated as code units for back-compat) or
  // the new capUnits name; capUnits wins if both are supplied.
  const capUnits = opts.capUnits ?? opts.capBytes ?? DEFAULT_CAP_UNITS;
  const fold = opts.fold ?? appendPtyTail;
  const bufs = new Map<string, { raw: string[]; units: number; folded: string[] }>();

  return {
    push(id, chunk) {
      if (!chunk) return;
      let b = bufs.get(id);
      if (!b) {
        b = { raw: [], units: 0, folded: [] };
        bufs.set(id, b);
      }
      b.raw.push(chunk);
      b.units += chunk.length;
      while (b.units > capUnits && b.raw.length > 1) b.units -= b.raw.shift()!.length;
      if (b.units > capUnits) {
        // A single chunk larger than the whole cap — keep its tail code units.
        const rawCut = b.units - capUnits;
        const safeCut = safeCutPoint(b.raw[0], rawCut);
        b.raw[0] = b.raw[0].slice(safeCut);
        b.units = b.raw[0].length;
      }
    },
    tail(id) {
      const b = bufs.get(id);
      if (!b) return [];
      if (b.raw.length) {
        for (const c of b.raw) b.folded = fold(b.folded, c);
        b.raw = [];
        b.units = 0;
      }
      return b.folded;
    },
    clear(id) {
      bufs.delete(id);
    },
  };
}
