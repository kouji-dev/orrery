import { describe, expect, it } from "vitest";
import {
  appendPtyTail,
  detectTitleStatus,
  isAwaitingInput,
  isPermissionPrompt,
  stripAnsi,
} from "./utils";

describe("stripAnsi", () => {
  it("removes CSI color / SGR sequences", () => {
    expect(stripAnsi("\x1b[31mred\x1b[0m done")).toBe("red done");
    expect(stripAnsi("\x1b[1;32mok\x1b[0m")).toBe("ok");
  });

  it("removes cursor-movement and erase sequences", () => {
    expect(stripAnsi("a\x1b[2K\x1b[1Gb")).toBe("ab");
  });

  it("removes OSC (window-title) sequences terminated by BEL or ST", () => {
    expect(stripAnsi("\x1b]0;my title\x07hi")).toBe("hi");
    expect(stripAnsi("\x1b]0;t\x1b\\hi")).toBe("hi");
  });

  it("leaves plain text untouched", () => {
    expect(stripAnsi("just text")).toBe("just text");
  });
});

describe("appendPtyTail", () => {
  it("splits a chunk into lines on \\n", () => {
    expect(appendPtyTail([], "one\ntwo\nthree")).toEqual(["one", "two", "three"]);
  });

  it("continues an in-progress line across chunks", () => {
    const a = appendPtyTail([], "hel");
    const b = appendPtyTail(a, "lo\nworld");
    expect(b).toEqual(["hello", "world"]);
  });

  it("overwrites the current line on carriage return (progress bars)", () => {
    expect(appendPtyTail([], "10%\r50%\r100%")).toEqual(["100%"]);
  });

  it("strips ANSI before folding", () => {
    expect(appendPtyTail([], "\x1b[32m✓ built\x1b[0m\n")).toEqual(["✓ built", ""]);
  });

  it("drops stray control characters", () => {
    expect(appendPtyTail([], "a\x07b\x08c")).toEqual(["abc"]);
  });

  it("caps the tail at `max` lines, keeping the most recent", () => {
    const out = appendPtyTail([], "1\n2\n3\n4\n5", 3);
    expect(out).toEqual(["3", "4", "5"]);
  });

  it("expands tabs to spaces", () => {
    expect(appendPtyTail([], "a\tb")).toEqual(["a  b"]);
  });

  it("resets the buffer on a clear-screen so a redraw doesn't stack frames", () => {
    const a = appendPtyTail([], "old frame line 1\nold frame line 2\n");
    // ESC[2J (clear screen) + ESC[H (home) then the new frame
    const b = appendPtyTail(a, "\x1b[2J\x1b[Hfresh frame\n");
    expect(b).toEqual(["fresh frame", ""]);
  });

  it("clears with ESC[3J as well", () => {
    const a = appendPtyTail([], "stale\n");
    expect(appendPtyTail(a, "\x1b[3Jnew")).toEqual(["new"]);
  });

  it("overwrites in place when the cursor is homed (ESC[H) without a clear", () => {
    const a = appendPtyTail([], "AAAA\nBBBB");
    // home cursor, write over the top line
    const b = appendPtyTail(a, "\x1b[Hxx");
    expect(b).toEqual(["xxAA", "BBBB"]);
  });

  it("moves up and overwrites with cursor-up (ESC[nA) — a spinner repaint", () => {
    // first frame: a label line, then a spinner line below
    const a = appendPtyTail([], "Building\n");
    // cursor up 1, erase the line, repaint -> overwrites "Building"
    const b = appendPtyTail(a, "\x1b[1A\x1b[2KBuilt");
    expect(b).toEqual(["Built", ""]);
  });

  it("erases to end of line with ESC[K", () => {
    const a = appendPtyTail([], "long progress text");
    // carriage return, write short text, erase the rest of the line
    const b = appendPtyTail(a, "\rdone\x1b[K");
    expect(b).toEqual(["done"]);
  });

  it("positions the cursor with ESC[row;colH (1-based) overwriting in place", () => {
    const a = appendPtyTail([], "line1\nline2\nline3");
    // row 2, col 3 (1-based) -> overwrite chars 3..4 of "line2" -> "liXX2"
    const b = appendPtyTail(a, "\x1b[2;3HXX");
    expect(b).toEqual(["line1", "liXX2", "line3"]);
  });
});

// ---- differential tests: run-tokenized impl vs the original char-per-token
// reference implementation (verbatim copy of appendPtyTail before the
// "tokenize in runs" rewrite). Outputs must be identical on every input.
const REF_TOKEN_RE =
  /\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[@-Z\\-_]|[\s\S]/g;
const REF_CSI_RE = /^\x1b\[([0-9;?]*)([@-~])$/;
function refStripUnactionable(s: string): string {
  return s.replace(/\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[@-Z\\-_]/g, "");
}
function appendPtyTailReference(prev: string[], chunk: string, max = 60): string[] {
  const lines = prev.length ? prev.slice() : [""];
  let row = lines.length - 1;
  let col = lines[row].length;

  const ensureRow = () => {
    if (row < 0) row = 0;
    while (lines.length <= row) lines.push("");
  };
  const writeAt = (text: string) => {
    ensureRow();
    const line = lines[row];
    const head = col <= line.length ? line.slice(0, col) : line.padEnd(col);
    lines[row] = head + text + line.slice(col + text.length);
    col += text.length;
  };

  const tokens = refStripUnactionable(chunk).match(REF_TOKEN_RE) ?? [];
  for (const tok of tokens) {
    if (tok === "\n") {
      row += 1;
      col = 0;
      ensureRow();
      continue;
    }
    if (tok === "\r") {
      col = 0;
      continue;
    }
    if (tok === "\t") {
      writeAt("  ");
      continue;
    }
    if (tok.length === 1) {
      if (tok >= " ") writeAt(tok);
      continue;
    }
    const m = REF_CSI_RE.exec(tok);
    if (!m) continue;
    const params = m[1];
    const final = m[2];
    const n = parseInt(params, 10);
    switch (final) {
      case "A":
        row = Math.max(0, row - (isNaN(n) ? 1 : n));
        break;
      case "B":
        row += isNaN(n) ? 1 : n;
        ensureRow();
        break;
      case "G":
        col = isNaN(n) ? 0 : Math.max(0, n - 1);
        break;
      case "H":
      case "f": {
        const [r, c] = params.split(";");
        row = r ? Math.max(0, parseInt(r, 10) - 1) : 0;
        col = c ? Math.max(0, parseInt(c, 10) - 1) : 0;
        ensureRow();
        break;
      }
      case "K":
        ensureRow();
        if (params === "1") lines[row] = " ".repeat(col) + lines[row].slice(col);
        else if (params === "2") lines[row] = "";
        else lines[row] = lines[row].slice(0, col);
        break;
      case "J":
        if (params === "2" || params === "3") {
          lines.length = 0;
          lines.push("");
          row = 0;
          col = 0;
        } else if (params === "1") {
          for (let i = 0; i < row; i++) lines[i] = "";
          ensureRow();
          lines[row] = lines[row].slice(col);
        } else {
          ensureRow();
          lines[row] = lines[row].slice(0, col);
          lines.length = row + 1;
        }
        break;
      default:
        break;
    }
  }
  return lines.length > max ? lines.slice(lines.length - max) : lines;
}

describe("appendPtyTail — differential vs char-per-token reference", () => {
  const both = (prev: string[], chunk: string, max?: number) => {
    const next = appendPtyTail(prev, chunk, max);
    const ref = appendPtyTailReference(prev, chunk, max);
    expect(next).toEqual(ref);
    return next;
  };

  it("matches on CR overwrite (progress bars)", () => {
    both([], "downloading 10%\rdownloading 50%\rdone        ");
    both(["partial"], "\rOVER");
    both([], "long first paint\rshort");
  });

  it("matches on backspace, including at column 0", () => {
    both([], "\x08abc"); // backspace with nothing before it
    both([], "ab\x08\x08X"); // backspaces mid-typing
    both(["row"], "\r\x08\x08tail"); // CR to col 0 then backspaces
  });

  it("matches on ANSI color mid-line", () => {
    both([], "plain \x1b[31mred\x1b[0m plain again");
    both([], "\x1b[1;32m✓\x1b[0m built \x1b[2m(2.3s)\x1b[0m\n");
    both(["existing"], "\x1b[33mwarn:\x1b[0m thing happened");
  });

  it("matches on \\r\\n mixes", () => {
    both([], "a\r\nb\r\nc");
    both([], "line one\r\r\nline two\r\n");
    both(["seed"], "\r\n\r\nafter blanks");
  });

  it("matches on long line append across chunks", () => {
    const longLine = "x".repeat(2000);
    const a = both([], longLine);
    both(a, "y".repeat(500));
    both(a, "\rrewound" + "z".repeat(100));
  });

  it("matches on writes past the line end (cursor padding)", () => {
    both(["hi"], "\x1b[10Gpadded"); // col 10 on a 2-char line
    both(["a", "b"], "\x1b[2;20HZZ"); // row 2, col 20 on a 1-char line
    both([], "\x1b[5Bdropped down"); // cursor down creates rows
  });

  it("matches on erase / clear / cursor-up repaints", () => {
    both(["frame line 1", "frame line 2"], "\x1b[2J\x1b[Hnew frame\n");
    both(["Building", "⠋ spinner"], "\x1b[1A\x1b[2KBuilt");
    both(["long progress text"], "\rdone\x1b[K");
    both(["abcdef"], "\r\x1b[3C…"); // unknown CSI (C) is a no-op in both
    both(["abcdef"], "\x1b[1K tail"); // erase to start
    both(["top", "mid", "bot"], "\x1b[2;2H\x1b[J"); // erase to end of screen
    both(["top", "mid", "bot"], "\x1b[2;2H\x1b[1J"); // erase to start of screen
  });

  it("matches on OSC titles, 2-char escapes, stray ESC and DEL", () => {
    both([], "\x1b]0;window title\x07visible");
    both([], "\x1b]0;t\x1b\\visible");
    both([], "before\x1bMafter"); // 2-char escape (RI) stripped
    both([], "dangling \x1b"); // lone trailing ESC dropped
    both([], "del\x7fchar kept"); // DEL is >= \x20 and written by both
    both([], "tab\there\tend");
  });

  it("matches on max-line capping", () => {
    both([], "1\n2\n3\n4\n5\n6\n7", 3);
    both(["a", "b", "c"], "d\ne\nf\ng", 4);
  });

  it("matches on deterministic pseudo-random mixed streams, fed chunk-by-chunk", () => {
    // simple LCG so the stream is reproducible
    let seed = 0x2026_0610;
    const rnd = (n: number) => {
      seed = (seed * 1664525 + 1013904223) >>> 0;
      return seed % n;
    };
    const pieces = [
      "lorem ", "ipsum", "\n", "\r", "\r\n", "\t", "\x08", "\x07", "✓ ", "⠋",
      "\x1b[31m", "\x1b[0m", "\x1b[2K", "\x1b[K", "\x1b[1A", "\x1b[2B",
      "\x1b[5G", "\x1b[2;3H", "\x1b[H", "\x1b[2J", "\x1b[J", "\x1b]0;t\x07",
      "\x1bM", "\x1b", "x".repeat(40), "0123456789",
    ];
    let next: string[] = [];
    let ref: string[] = [];
    for (let chunk = 0; chunk < 200; chunk++) {
      let s = "";
      const parts = 1 + rnd(8);
      for (let i = 0; i < parts; i++) s += pieces[rnd(pieces.length)];
      next = appendPtyTail(next, s, 20);
      ref = appendPtyTailReference(ref, s, 20);
      expect(next).toEqual(ref);
    }
  });
});

describe("appendPtyTail — perf sanity", () => {
  it("folds a 100KB plain chunk well under a generous bound", () => {
    // ~100KB of plain text in 80-col lines — the common "agent prints a lot"
    // shape. Run-tokenization makes this O(tokens); the old per-char fold was
    // O(chars × line length). Generous bound to stay flake-free on CI.
    const line = "the quick brown fox jumps over the lazy dog 0123456789 abcdef\n";
    const chunk = line.repeat(Math.ceil(100_000 / line.length));
    const t0 = performance.now();
    const out = appendPtyTail([], chunk);
    const elapsed = performance.now() - t0;
    expect(out.length).toBe(60); // capped at default max
    expect(elapsed).toBeLessThan(500);
  });

  it("folds a 100KB single-line chunk (no newlines) quickly too", () => {
    // worst case for the old impl: one ever-growing line. With runs this is a
    // handful of splices, not 100k whole-line rebuilds.
    const chunk = ("z".repeat(5000) + "\x1b[31m").repeat(20);
    const t0 = performance.now();
    const out = appendPtyTail([], chunk);
    const elapsed = performance.now() - t0;
    expect(out).toEqual(["z".repeat(100_000)]);
    expect(elapsed).toBeLessThan(500);
  });
});

describe("detectTitleStatus", () => {
  it("reads a braille spinner as working", () => {
    expect(detectTitleStatus("⠋ ~/proj")).toBe("working");
    expect(detectTitleStatus("⣾ Claude")).toBe("working");
  });

  it("reads working keywords as working", () => {
    expect(detectTitleStatus("Codex working")).toBe("working");
    expect(detectTitleStatus("Aider thinking…")).toBe("working");
    expect(detectTitleStatus("OpenCode running")).toBe("working");
  });

  it("does not false-positive on keywords inside words or paths", () => {
    expect(detectTitleStatus("reworking the plan")).toBeNull();
    expect(detectTitleStatus("~/codex/working")).toBeNull();
    expect(detectTitleStatus("overthinking")).toBeNull();
  });

  it("reads gemini working / silent-working glyphs as working", () => {
    expect(detectTitleStatus("✦ Gemini")).toBe("working");
    expect(detectTitleStatus("⏲ Gemini")).toBe("working");
  });

  it("reads the permission glyph as permission", () => {
    expect(detectTitleStatus("✋ Gemini")).toBe("permission");
  });

  it("reads known idle glyphs as idle", () => {
    expect(detectTitleStatus("✳ ~/proj")).toBe("idle"); // claude idle prefix
    expect(detectTitleStatus("◇ Gemini")).toBe("idle");
  });

  it("returns null for a plain title with no signal", () => {
    expect(detectTitleStatus("~/projects/orrery")).toBeNull();
    expect(detectTitleStatus("")).toBeNull();
  });

  it("prioritizes permission over a working spinner in the same title", () => {
    expect(detectTitleStatus("✋ ⠋ Gemini")).toBe("permission");
  });
});

describe("isPermissionPrompt", () => {
  it("flags yes/no permission prompts", () => {
    expect(isPermissionPrompt("Do you want to proceed? (y/n)")).toBe(true);
    expect(isPermissionPrompt("Allow this command to run?")).toBe(true);
    expect(isPermissionPrompt("Grant access to the network? [y/n]")).toBe(true);
  });

  it("flags numbered yes/no menus", () => {
    expect(isPermissionPrompt("❯ 1. Yes\n  2. No")).toBe(true);
    expect(isPermissionPrompt("1. Yes  2. No, and tell me why")).toBe(true);
  });

  it("treats open questions as not-permission", () => {
    expect(isPermissionPrompt("Which database should I use, Redis or Postgres?")).toBe(false);
    expect(isPermissionPrompt("What should the retry cap be?")).toBe(false);
  });
});

describe("isAwaitingInput", () => {
  it("is true for permission prompts", () => {
    expect(isAwaitingInput("Allow this command? (y/n)")).toBe(true);
  });

  it("is true when the last line is a question", () => {
    expect(isAwaitingInput("Analyzing repo…\nWhich database should I use?")).toBe(true);
  });

  it("is false for ongoing work output", () => {
    expect(isAwaitingInput("Drafting implementation plan\nWriting tests")).toBe(false);
    expect(isAwaitingInput("")).toBe(false);
  });

  it("only considers the final line for the trailing-question heuristic", () => {
    expect(isAwaitingInput("Is this right?\nNow committing the change")).toBe(false);
  });
});
