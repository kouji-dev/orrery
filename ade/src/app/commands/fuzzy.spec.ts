import { describe, expect, it } from "vitest";
import { bindingFromEvent, fzMatch, kbdLabel, matchBinding } from "./fuzzy";
import { fzSegments } from "./overlay-shell.component";

describe("fzMatch", () => {
  it("matches subsequences and returns the hit indices", () => {
    const m = fzMatch("Show All Commands", "sac");
    expect(m).not.toBeNull();
    expect(m!.idx.length).toBe(3);
  });

  it("returns null when a char is missing", () => {
    expect(fzMatch("Recent Files", "zzz")).toBeNull();
  });

  it("empty query matches everything with zero score", () => {
    expect(fzMatch("anything", "")).toEqual({ score: 0, idx: [] });
  });

  it("prefers word-boundary matches", () => {
    const boundary = fzMatch("in-files", "fi")!;
    const middle = fzMatch("affix", "fi")!;
    expect(boundary).not.toBeNull();
    expect(middle).not.toBeNull();
    expect(boundary.score).toBeGreaterThan(middle.score);
  });
});

describe("bindingFromEvent (B6.2 keymap capture)", () => {
  const ev = (init: KeyboardEventInit) => new KeyboardEvent("keydown", init);

  it("produces strings matchBinding round-trips", () => {
    const e = ev({ key: "s", ctrlKey: true, altKey: true });
    const b = bindingFromEvent(e)!;
    expect(b).toBe("Ctrl+Alt+s");
    expect(matchBinding(e, b)).toBe(true);
  });

  it("captures shift as part of the chord", () => {
    expect(bindingFromEvent(ev({ key: "P", ctrlKey: true, shiftKey: true }))).toBe("Ctrl+Shift+p");
  });

  it("rejects pure-modifier presses and unmodified keys", () => {
    expect(bindingFromEvent(ev({ key: "Control", ctrlKey: true }))).toBeNull();
    expect(bindingFromEvent(ev({ key: "Shift", shiftKey: true }))).toBeNull();
    expect(bindingFromEvent(ev({ key: "x" }))).toBeNull(); // would fire while typing
    expect(bindingFromEvent(ev({ key: "x", shiftKey: true }))).toBeNull(); // Shift+x = typing "X"
  });

  it("alt-only chords are recordable", () => {
    expect(bindingFromEvent(ev({ key: "F7", altKey: true }))).toBe("Alt+f7");
  });
});

describe("fzSegments", () => {
  it("splits text into hit/non-hit runs", () => {
    expect(fzSegments("abc", [0, 1])).toEqual([
      { t: "ab", hit: true },
      { t: "c", hit: false },
    ]);
  });

  it("returns one flat segment without indices", () => {
    expect(fzSegments("abc", [])).toEqual([{ t: "abc", hit: false }]);
  });
});

describe("kbdLabel", () => {
  it("renders windows-style chords", () => {
    expect(kbdLabel("Ctrl+Shift+p")).toBe("Ctrl+Shift+P");
  });
  it("keeps the double-shift special", () => {
    expect(kbdLabel("Shift Shift")).toMatch(/Shift Shift|⇧⇧/);
  });
});
