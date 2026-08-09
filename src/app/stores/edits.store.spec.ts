import { describe, expect, it } from "vitest";

import { EditsStore } from "./edits.store";

const A = "agent-1";
const P = "src/lib.rs";

describe("EditsStore", () => {
  it("open initializes a clean buffer and re-open refreshes it", () => {
    const s = new EditsStore();
    const b = s.open(A, P, "one");
    expect(b).toEqual({ baseText: "one", text: "one", dirty: false });
    // clean buffer follows the disk (silent swap on external change)
    const b2 = s.open(A, P, "two");
    expect(b2.text).toBe("two");
    expect(s.isDirty(A, P)).toBe(false);
  });

  it("open is idempotent for an unchanged clean buffer (no signal write)", () => {
    // Regression: the editor's adoption effect calls open() — an unconditional
    // state write made that effect retrigger itself forever (frozen UI).
    const s = new EditsStore();
    const b1 = s.open(A, P, "one");
    const b2 = s.open(A, P, "one");
    expect(b2).toBe(b1); // same reference ⇒ state untouched
  });

  it("update flips dirty and typing back to base clears it", () => {
    const s = new EditsStore();
    s.open(A, P, "one");
    s.update(A, P, "one edited");
    expect(s.isDirty(A, P)).toBe(true);
    expect(s.dirtyPaths(A)).toEqual([P]);
    s.update(A, P, "one");
    expect(s.isDirty(A, P)).toBe(false);
  });

  it("open never clobbers a dirty buffer", () => {
    const s = new EditsStore();
    s.open(A, P, "one");
    s.update(A, P, "mine");
    const kept = s.open(A, P, "theirs");
    expect(kept.text).toBe("mine");
    expect(kept.baseText).toBe("one"); // caller sees disk≠base ⇒ conflict banner
    expect(s.isDirty(A, P)).toBe(true);
  });

  it("saved adopts the buffer as the new base (watcher echo-tolerance)", () => {
    const s = new EditsStore();
    s.open(A, P, "one");
    s.update(A, P, "mine");
    s.saved(A, P);
    expect(s.get(A, P)).toEqual({ baseText: "mine", text: "mine", dirty: false });
    // the save's own watcher burst re-reads disk ("mine") — a no-op open
    const after = s.open(A, P, "mine");
    expect(after.dirty).toBe(false);
  });

  it("discard drops edits in favor of disk", () => {
    const s = new EditsStore();
    s.open(A, P, "one");
    s.update(A, P, "mine");
    s.discard(A, P, "disk");
    expect(s.get(A, P)).toEqual({ baseText: "disk", text: "disk", dirty: false });
  });

  it("close and closeAgent drop buffers", () => {
    const s = new EditsStore();
    s.open(A, P, "one");
    s.open(A, "other.ts", "x");
    s.open("agent-2", P, "y");
    s.close(A, P);
    expect(s.get(A, P)).toBeUndefined();
    s.closeAgent(A);
    expect(s.get(A, "other.ts")).toBeUndefined();
    expect(s.get("agent-2", P)).toBeDefined();
  });

  it("dirtyKeys drives per-agent dirty listings", () => {
    const s = new EditsStore();
    s.open(A, "a.ts", "1");
    s.open(A, "b.ts", "2");
    s.update(A, "a.ts", "1!");
    s.update(A, "b.ts", "2!");
    expect(s.dirtyPaths(A).sort()).toEqual(["a.ts", "b.ts"]);
    s.saved(A, "a.ts");
    expect(s.dirtyPaths(A)).toEqual(["b.ts"]);
  });
});
