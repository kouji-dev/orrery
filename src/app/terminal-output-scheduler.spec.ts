import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import {
  BACKLOG_WARNING,
  DRAIN_INTERVAL_MS,
  HIDDEN_FIRST_FLUSH_DELAY_MS,
  discardTerminalQueue,
  flushTerminalQueue,
  resetTerminalSchedulerForTests,
  setSchedulerStatsEnabled,
  terminalSchedulerStats,
  writeScheduled,
} from "./terminal-output-scheduler";

/** Fake xterm: records writes, fires the parsed callback synchronously. */
function fakeTerm() {
  const writes: string[] = [];
  return {
    writes,
    write(data: string, cb?: () => void) {
      writes.push(data);
      cb?.();
    },
  };
}

describe("terminal-output-scheduler", () => {
  beforeEach(() => {
    vi.useFakeTimers();
    resetTerminalSchedulerForTests();
  });
  afterEach(() => vi.useRealTimers());

  it("visible writes go direct, after any queued backlog (order preserved)", () => {
    const t = fakeTerm();
    writeScheduled("a", t, "queued1", { visible: false });
    writeScheduled("a", t, "queued2", { visible: false });
    expect(t.writes).toEqual([]); // hidden → not written yet
    writeScheduled("a", t, "live", { visible: true });
    expect(t.writes).toEqual(["queued1queued2", "live"]); // backlog first, then live
  });

  it("hidden writes drain after the first flush delay, in order", () => {
    const t = fakeTerm();
    writeScheduled("a", t, "one", { visible: false });
    writeScheduled("a", t, "two", { visible: false });
    vi.advanceTimersByTime(HIDDEN_FIRST_FLUSH_DELAY_MS + 1);
    expect(t.writes).toEqual(["onetwo"]);
  });

  it("round-robins across hidden terminals with bounded writes per tick", () => {
    const big = "x".repeat(20 * 1024); // > one drain chunk each
    const ta = fakeTerm();
    const tb = fakeTerm();
    writeScheduled("a", ta, big, { visible: false });
    writeScheduled("b", tb, big, { visible: false });
    vi.advanceTimersByTime(HIDDEN_FIRST_FLUSH_DELAY_MS + 1); // first tick: 2 writes max
    expect(ta.writes.length + tb.writes.length).toBe(2);
    expect(ta.writes.length).toBe(1); // one each — not both from "a"
    expect(tb.writes.length).toBe(1);
    vi.advanceTimersByTime(DRAIN_INTERVAL_MS * 4); // remaining tails drain
    expect(ta.writes.join("")).toBe(big);
    expect(tb.writes.join("")).toBe(big);
  });

  it("caps the hidden backlog: drops the middle, keeps the newest tail, warns once", () => {
    setSchedulerStatsEnabled(true);
    const t = fakeTerm();
    let dropped = 0;
    const chunk = "y".repeat(256 * 1024);
    for (let i = 0; i < 10; i++) {
      // 9th push crosses the 2MB cap → backlog replaced; 10th queues after it
      writeScheduled("a", t, chunk, {
        visible: false,
        onBacklogDropped: () => dropped++,
      });
    }
    expect(dropped).toBe(1); // notified exactly once
    flushTerminalQueue("a");
    // lossy semantics: lose the MIDDLE, keep the warning + the newest tail
    expect(t.writes.join("")).toBe(BACKLOG_WARNING + chunk);
    expect(terminalSchedulerStats().droppedBacklogs).toBe(1);
  });

  it("flushTerminalQueue writes everything queued immediately", () => {
    const t = fakeTerm();
    writeScheduled("a", t, "hello ", { visible: false });
    writeScheduled("a", t, "world", { visible: false });
    flushTerminalQueue("a");
    expect(t.writes.join("")).toBe("hello world");
    vi.advanceTimersByTime(1000);
    expect(t.writes.join("")).toBe("hello world"); // nothing left for the drain
  });

  it("discardTerminalQueue drops silently", () => {
    const t = fakeTerm();
    writeScheduled("a", t, "gone", { visible: false });
    discardTerminalQueue("a");
    vi.advanceTimersByTime(1000);
    expect(t.writes).toEqual([]);
  });

  it("propagates onParsed for both direct and drained writes", () => {
    const t = fakeTerm();
    let parsed = 0;
    writeScheduled("a", t, "h", { visible: false, onParsed: () => parsed++ });
    vi.advanceTimersByTime(HIDDEN_FIRST_FLUSH_DELAY_MS + 1);
    writeScheduled("a", t, "v", { visible: true, onParsed: () => parsed++ });
    expect(parsed).toBe(2);
  });

  describe("stats gate (setSchedulerStatsEnabled)", () => {
    it("disabled by default: bumpStats is a no-op — stats stay zero", () => {
      const t = fakeTerm();
      writeScheduled("a", t, "chunk", { visible: true });
      expect(terminalSchedulerStats().directWrites).toBe(0);
      expect(terminalSchedulerStats().queuedChars).toBe(0);
    });

    it("disabled: hidden enqueue does not update stats", () => {
      const t = fakeTerm();
      writeScheduled("a", t, "hidden", { visible: false });
      vi.advanceTimersByTime(HIDDEN_FIRST_FLUSH_DELAY_MS + 1);
      expect(terminalSchedulerStats().drainedWrites).toBe(0);
      expect(terminalSchedulerStats().queuedChars).toBe(0);
    });

    it("enabled: direct writes increment directWrites", () => {
      setSchedulerStatsEnabled(true);
      const t = fakeTerm();
      writeScheduled("a", t, "chunk1", { visible: true });
      writeScheduled("a", t, "chunk2", { visible: true });
      expect(terminalSchedulerStats().directWrites).toBe(2);
    });

    it("enabled: hidden drain increments drainedWrites", () => {
      setSchedulerStatsEnabled(true);
      const t = fakeTerm();
      writeScheduled("a", t, "x", { visible: false });
      vi.advanceTimersByTime(HIDDEN_FIRST_FLUSH_DELAY_MS + 1);
      expect(terminalSchedulerStats().drainedWrites).toBe(1);
    });

    it("re-enable resets counters to zero", () => {
      setSchedulerStatsEnabled(true);
      const t = fakeTerm();
      writeScheduled("a", t, "chunk", { visible: true });
      expect(terminalSchedulerStats().directWrites).toBe(1);
      // disable then re-enable — counters must reset
      setSchedulerStatsEnabled(false);
      setSchedulerStatsEnabled(true);
      expect(terminalSchedulerStats().directWrites).toBe(0);
      expect(terminalSchedulerStats().drainedWrites).toBe(0);
      expect(terminalSchedulerStats().droppedBacklogs).toBe(0);
    });

    it("enabling twice is idempotent: does not reset mid-session", () => {
      setSchedulerStatsEnabled(true);
      const t = fakeTerm();
      writeScheduled("a", t, "c1", { visible: true });
      setSchedulerStatsEnabled(true); // second enable — should be no-op
      expect(terminalSchedulerStats().directWrites).toBe(1);
    });

    it("disabled: backlog drop does not increment droppedBacklogs", () => {
      const t = fakeTerm();
      const chunk = "y".repeat(256 * 1024);
      for (let i = 0; i < 10; i++) {
        writeScheduled("a", t, chunk, { visible: false });
      }
      expect(terminalSchedulerStats().droppedBacklogs).toBe(0);
    });
  });
});
