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
 * The raw side is capped (default 64 KiB per agent, oldest chunks dropped) so
 * an unread agent can stream forever without growing memory. appendPtyTail
 * keeps at most 60 lines, so the most recent 64 KiB reproduces the same
 * visible tail in practice; folding stays chunk-by-chunk (not one joined
 * string) to preserve the eager pipeline's exact fold order.
 */
export interface PtyTailBuffer {
  /** Append a raw chunk — O(1), no parsing. Oldest raw bytes drop past the cap. */
  push(id: string, chunk: string): void;
  /** The folded plain-text tail. Folds pending raw chunks first (then caches);
   *  returns the cached array — treat as read-only. [] for an unknown agent. */
  tail(id: string): string[];
  /** Drop ALL state (raw + folded) for an agent — on removal. */
  clear(id: string): void;
}

const DEFAULT_CAP_BYTES = 64 * 1024;

export function createPtyTailBuffer(
  opts: { capBytes?: number; fold?: typeof appendPtyTail } = {},
): PtyTailBuffer {
  const cap = opts.capBytes ?? DEFAULT_CAP_BYTES;
  const fold = opts.fold ?? appendPtyTail;
  const bufs = new Map<string, { raw: string[]; bytes: number; folded: string[] }>();

  return {
    push(id, chunk) {
      if (!chunk) return;
      let b = bufs.get(id);
      if (!b) {
        b = { raw: [], bytes: 0, folded: [] };
        bufs.set(id, b);
      }
      b.raw.push(chunk);
      b.bytes += chunk.length;
      while (b.bytes > cap && b.raw.length > 1) b.bytes -= b.raw.shift()!.length;
      if (b.bytes > cap) {
        // a single chunk larger than the whole cap — keep its tail bytes
        b.raw[0] = b.raw[0].slice(b.bytes - cap);
        b.bytes = cap;
      }
    },
    tail(id) {
      const b = bufs.get(id);
      if (!b) return [];
      if (b.raw.length) {
        for (const c of b.raw) b.folded = fold(b.folded, c);
        b.raw = [];
        b.bytes = 0;
      }
      return b.folded;
    },
    clear(id) {
      bufs.delete(id);
    },
  };
}
