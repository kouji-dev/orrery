import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { createPtyTailCoalescer } from "./pty-tail-coalescer";

describe("createPtyTailCoalescer", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("batches chunks per agent into one flush per interval", () => {
    const flushes: Map<string, string>[] = [];
    const c = createPtyTailCoalescer((b) => flushes.push(b), 80);
    c.push("a", "one");
    c.push("a", "two");
    c.push("b", "x");
    expect(flushes.length).toBe(0); // nothing before the interval
    vi.advanceTimersByTime(81);
    expect(flushes.length).toBe(1);
    expect(flushes[0].get("a")).toBe("onetwo");
    expect(flushes[0].get("b")).toBe("x");
  });

  it("a later push starts a new batch", () => {
    const flushes: Map<string, string>[] = [];
    const c = createPtyTailCoalescer((b) => flushes.push(b), 80);
    c.push("a", "1");
    vi.advanceTimersByTime(81);
    c.push("a", "2");
    vi.advanceTimersByTime(81);
    expect(flushes.length).toBe(2);
    expect(flushes[1].get("a")).toBe("2");
  });

  it("dispose drops pending without flushing", () => {
    const flushes: Map<string, string>[] = [];
    const c = createPtyTailCoalescer((b) => flushes.push(b), 80);
    c.push("a", "gone");
    c.dispose();
    vi.advanceTimersByTime(1000);
    expect(flushes.length).toBe(0);
  });
});
