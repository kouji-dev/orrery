import { describe, expect, it, vi } from "vitest";
import { appendPtyTail } from "../utils";
import { createPtyTailBuffer } from "./pty-tail-buffer";

// Eager reference: what the old pipeline produced — every chunk folded through
// appendPtyTail as it arrived. The lazy buffer must reproduce this exactly
// (chunk-by-chunk fold order preserved) as long as nothing was dropped by the cap.
function eagerFold(chunks: string[], max?: number): string[] {
  return chunks.reduce<string[]>((prev, c) => appendPtyTail(prev, c, max), []);
}

describe("createPtyTailBuffer", () => {
  it("tail of an unknown agent is empty", () => {
    const buf = createPtyTailBuffer();
    expect(buf.tail("nope")).toEqual([]);
  });

  it("lazy fold matches the eager chunk-by-chunk reference", () => {
    // Realistic PTY traffic: partial lines, \r progress overwrite, cursor-up
    // redraw, clear-screen frame swap — the cases appendPtyTail interprets.
    const chunks = [
      "hel",
      "lo\nworld\n",
      "10%\r50%\r100%\n",
      "Building\n",
      "\x1b[1A\x1b[2KBuilt\n",
      "\x1b[2J\x1b[Hfresh frame\n",
      "Proceed? (y/n)",
    ];
    const buf = createPtyTailBuffer();
    for (const c of chunks) buf.push("a", c);
    expect(buf.tail("a")).toEqual(eagerFold(chunks));
  });

  it("reads interleaved with pushes still match the eager reference", () => {
    const chunks = ["one\n", "two\r2..\rtwo!\n", "thr", "ee\n", "\x1b[2Jlast\n"];
    const buf = createPtyTailBuffer();
    const seen: string[] = [];
    for (const c of chunks) {
      buf.push("a", c);
      seen.push(c);
      expect(buf.tail("a")).toEqual(eagerFold(seen)); // parity at every read point
    }
  });

  it("push never folds; tail folds pending once and caches until dirtied", () => {
    const fold = vi.fn(appendPtyTail);
    const buf = createPtyTailBuffer({ fold });
    buf.push("a", "one\n");
    buf.push("a", "two\n");
    expect(fold).not.toHaveBeenCalled(); // raw append only — zero parsing on push

    const first = buf.tail("a");
    expect(fold).toHaveBeenCalledTimes(2); // one fold per buffered chunk
    expect(buf.tail("a")).toBe(first); // clean → cached array, no re-fold
    expect(fold).toHaveBeenCalledTimes(2);

    buf.push("a", "three\n"); // new raw chunk → dirty
    expect(buf.tail("a")).toEqual(eagerFold(["one\n", "two\n", "three\n"]));
    expect(fold).toHaveBeenCalledTimes(3); // only the NEW chunk got folded
  });

  it("ring cap drops the oldest unread chunks", () => {
    const buf = createPtyTailBuffer({ capUnits: 10 });
    buf.push("a", "one\n"); // 4 code units
    buf.push("a", "two\n"); // 8 code units
    buf.push("a", "three\n"); // 14 → "one\n" dropped (10 left)
    expect(buf.tail("a")).toEqual(["two", "three", ""]);
  });

  it("a single chunk larger than the cap keeps only its tail code units", () => {
    const buf = createPtyTailBuffer({ capUnits: 6 });
    buf.push("a", "abc\ndef\nghi"); // 11 code units → keep the last 6: "ef\nghi"
    expect(buf.tail("a")).toEqual(["ef", "ghi"]);
  });

  it("cap and tails are tracked per agent independently", () => {
    const buf = createPtyTailBuffer({ capUnits: 12 });
    buf.push("a", "aaaa\n");
    buf.push("b", "bbbb\n");
    buf.push("a", "AAAA\n");
    buf.push("a", "filler\n"); // 17 code units total → "aaaa\n" drops out of a's ring only
    expect(buf.tail("a")).toEqual(["AAAA", "filler", ""]);
    expect(buf.tail("b")).toEqual(["bbbb", ""]); // b untouched by a's cap
  });

  it("clear drops raw and folded state for that agent", () => {
    const fold = vi.fn(appendPtyTail);
    const buf = createPtyTailBuffer({ fold });
    buf.push("a", "old\n");
    buf.push("b", "kept-by-b\n");
    buf.tail("a");
    buf.push("a", "pending\n");
    fold.mockClear();

    buf.clear("a");
    expect(buf.tail("a")).toEqual([]); // cache gone…
    expect(fold).not.toHaveBeenCalled(); // …and pending raw never folded
    expect(buf.tail("b")).toEqual(["kept-by-b", ""]); // other agents unaffected
  });

  describe("unicode/escape-safe trim at the ring seam", () => {
    /**
     * Checks a folded output array for lone surrogates and for a partial ESC
     * sequence at the very start of the first line's content.
     *
     * A "partial ESC at the seam" means the joined output begins with ESC
     * immediately followed by '[' but the sequence has no final byte — which
     * would happen if the cut sliced through an ESC[… sequence.
     */
    function assertNoLoneSurrogatesOrPartialEsc(lines: string[]) {
      const joined = lines.join("\n");

      // No lone surrogates: every high surrogate (0xD800-0xDBFF) must be
      // followed by a low surrogate (0xDC00-0xDFFF) and vice-versa.
      for (let i = 0; i < joined.length; i++) {
        const c = joined.charCodeAt(i);
        if (c >= 0xd800 && c <= 0xdbff) {
          // high surrogate — next must be low
          const next = joined.charCodeAt(i + 1);
          expect(next >= 0xdc00 && next <= 0xdfff, `lone high surrogate at ${i}`).toBe(true);
          i++; // skip the paired low surrogate
        } else if (c >= 0xdc00 && c <= 0xdfff) {
          // unpaired low surrogate
          expect.fail(`lone low surrogate at position ${i}`);
        }
      }

      // No partial ESC[… at the seam: the output must not START with an
      // incomplete CSI sequence (ESC [ not terminated by a byte in 0x40-0x7E).
      if (joined.charCodeAt(0) === 0x1b && joined.charCodeAt(1) === 0x5b /* '[' */) {
        let hasFinal = false;
        for (let i = 2; i < joined.length; i++) {
          const c = joined.charCodeAt(i);
          if (c >= 0x40 && c <= 0x7e) {
            hasFinal = true;
            break;
          }
        }
        expect(hasFinal, "output starts with an incomplete CSI escape sequence").toBe(true);
      }
    }

    it("surrogate pair straddling the trim cut is not split", () => {
      // U+1F600 GRINNING FACE encodes as two UTF-16 code units: 0xD83D 0xDE00.
      // Build a chunk whose trim cut would land between the two surrogates, then
      // verify the retained slice has no lone surrogates.
      const emoji = "😀"; // 2 code units
      // Pad so the cut (capUnits = 5) lands right at the high surrogate.
      // "ab\n" = 3 code units, then emoji = 2 → total 5; with cap=4 the cut is at index 1,
      // and with the string "x\n" + emoji (4 code units) + "y\n" we can position precisely.
      // Easier: make a 10-CU string, cap=5, so cut=5 which sits on the low surrogate.
      // Layout: "abc\n" (4 CU) + high (1 CU) | low (1 CU) + "\nok\n" (4 CU) = 10 CU
      const chunk = "abc\n" + "\uD83D" + "\uDE00" + "\nok\n";
      expect(chunk.length).toBe(10);
      const buf = createPtyTailBuffer({ capUnits: 5 }); // cut = 10 - 5 = 5 → sits on low surrogate
      buf.push("a", chunk);
      const result = buf.tail("a");
      assertNoLoneSurrogatesOrPartialEsc(result);
    });

    it("incomplete ESC sequence at the trim cut is not leaked into output", () => {
      // Build a chunk where an ESC[ sequence straddles the trim boundary so
      // that without the safe-cut logic the retained suffix would begin with
      // an incomplete/partial CSI sequence.
      // Layout (10 CU): "abcd" (4) + ESC (1) + "[" (1) + "12" (2) + "m\n" (2)
      // capUnits = 6 → cut = 4; without the fix the retained part starts with ESC[12m\n
      // which IS a complete sequence — so let's make an incomplete one:
      // Layout (10 CU): "abcd" (4) + ESC (1) + "[" (1) + "12" (2) + "34" (2)  ← no final byte
      // capUnits = 6 → cut = 4; retained = ESC[1234  — incomplete CSI, no final byte.
      const chunk = "abcd" + "\x1b" + "[" + "1234"; // 10 CU, no CSI final byte
      expect(chunk.length).toBe(10);
      const buf = createPtyTailBuffer({ capUnits: 6 }); // cut = 10 - 6 = 4
      buf.push("a", chunk);
      const result = buf.tail("a");
      // The output must not begin with an incomplete CSI sequence.
      assertNoLoneSurrogatesOrPartialEsc(result);
    });

    it("complete ESC sequence that ends exactly at cut boundary is preserved", () => {
      // "ab\x1b[1mcd\n" — ESC[1m is complete (ESC + [ + 1 + m = 4 CU), final byte = 'm'.
      // chunk = "ab" (2) + ESC (1) + "[" (1) + "1" (1) + "m" (1) + "cd" (2) + "\n" (1) = 9 CU
      const chunk = "ab\x1b[1mcd\n";
      expect(chunk.length).toBe(9);
      const buf = createPtyTailBuffer({ capUnits: 7 }); // cut = 9 - 7 = 2, just before the ESC
      buf.push("a", chunk);
      const result = buf.tail("a");
      // Output is valid — no lone surrogates, no partial ESC at seam.
      assertNoLoneSurrogatesOrPartialEsc(result);
    });
  });
});
