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
    const buf = createPtyTailBuffer({ capBytes: 10 });
    buf.push("a", "one\n"); // 4 bytes
    buf.push("a", "two\n"); // 8 bytes
    buf.push("a", "three\n"); // 14 → "one\n" dropped (10 left)
    expect(buf.tail("a")).toEqual(["two", "three", ""]);
  });

  it("a single chunk larger than the cap keeps only its tail bytes", () => {
    const buf = createPtyTailBuffer({ capBytes: 6 });
    buf.push("a", "abc\ndef\nghi"); // 11 bytes → keep the last 6: "ef\nghi"
    expect(buf.tail("a")).toEqual(["ef", "ghi"]);
  });

  it("cap and tails are tracked per agent independently", () => {
    const buf = createPtyTailBuffer({ capBytes: 12 });
    buf.push("a", "aaaa\n");
    buf.push("b", "bbbb\n");
    buf.push("a", "AAAA\n");
    buf.push("a", "filler\n"); // 17 bytes total → "aaaa\n" drops out of a's ring only
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
});
