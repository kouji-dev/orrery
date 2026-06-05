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
    expect(detectTitleStatus("~/projects/katrix")).toBeNull();
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
