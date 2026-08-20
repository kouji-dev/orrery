import { beforeEach, describe, expect, it } from "vitest";
import { clearUpdateResume, drainUpdateResume, saveUpdateResume } from "./update-restore";

const KEY = "orrery:update-restore";

describe("update-resume list", () => {
  beforeEach(() => localStorage.clear());

  it("round-trips the saved agent ids through drain", () => {
    saveUpdateResume(["a1", "a2"]);
    expect(drainUpdateResume()).toEqual(["a1", "a2"]);
  });

  it("drain is one-shot — the second call gets nothing", () => {
    saveUpdateResume(["a1"]);
    expect(drainUpdateResume()).toEqual(["a1"]);
    expect(drainUpdateResume()).toEqual([]);
  });

  it("ignores a stale list (failed install that never relaunched)", () => {
    saveUpdateResume(["a1"]);
    const raw = JSON.parse(localStorage.getItem(KEY)!);
    raw.at = Date.now() - 16 * 60_000;
    localStorage.setItem(KEY, JSON.stringify(raw));
    expect(drainUpdateResume()).toEqual([]);
    expect(localStorage.getItem(KEY)).toBeNull(); // still consumed
  });

  it("ignores malformed payloads", () => {
    localStorage.setItem(KEY, "{not json");
    expect(drainUpdateResume()).toEqual([]);
    localStorage.setItem(KEY, JSON.stringify({ v: 99 }));
    expect(drainUpdateResume()).toEqual([]);
  });

  it("clearUpdateResume removes a pending list", () => {
    saveUpdateResume(["a1"]);
    clearUpdateResume();
    expect(drainUpdateResume()).toEqual([]);
  });
});
