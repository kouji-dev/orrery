import { afterEach, describe, expect, it, vi } from "vitest";
import { PerfStore } from "./perf.store";

function row(store: PerfStore, cmd: string) {
  return store.rows().find((r) => r.cmd === cmd);
}

afterEach(() => vi.useRealTimers());

describe("PerfStore", () => {
  it("aggregates round-trip samples per command", () => {
    const s = new PerfStore();
    s.record("agent_diff", 10, true);
    s.record("agent_diff", 20, true);
    s.record("agent_diff", 30, false);
    const r = row(s, "agent_diff")!;
    expect(r.calls10s).toBe(3);
    expect(r.avgRt).toBe(20);
    expect(r.maxRt).toBe(30);
    expect(r.errPct).toBeCloseTo(100 / 3);
    expect(r.hist).toEqual([10, 20, 30]);
  });

  it("computes p95 from the window", () => {
    const s = new PerfStore();
    for (let i = 1; i <= 100; i++) s.record("c", i, true);
    // 100 samples 1..100 → ceil(0.95*100)-1 = idx 94 → value 95
    expect(row(s, "c")!.p95Rt).toBe(95);
  });

  it("merges backend exec aggregates and derives overhead", () => {
    const s = new PerfStore();
    s.record("git_status", 96, true);
    s.setExec([{ cmd: "git_status", calls10s: 0, avgMs: 91, p95Ms: 142, maxMs: 210 }]);
    const r = row(s, "git_status")!;
    expect(r.avgExec).toBe(91);
    expect(r.overhead).toBeCloseTo(96 - 91);
    // frontend sample wins: max(1 invoke, 0 exec calls10s) = 1
    expect(r.calls10s).toBe(1);
  });

  it("backend-only rows take their call rate from exec aggregates", () => {
    const s = new PerfStore();
    s.setExec([{ cmd: "agent_output_emit", calls10s: 940, avgMs: 0.1, p95Ms: 0.3, maxMs: 1.2 }]);
    const r = row(s, "agent_output_emit")!;
    expect(r.calls10s).toBe(940); // not 0 — no frontend invokes exist for pushes
    expect(r.avgRt).toBeNull();
    expect(r.avgExec).toBe(0.1);
  });

  it("passes backend byte volume through for throughput rows", () => {
    const s = new PerfStore();
    s.setExec([
      { cmd: "agent_output_emit", calls10s: 900, bytes10s: 1_024_000, avgMs: 0.1, p95Ms: 0.2, maxMs: 0.5 },
      { cmd: "agent_scan", calls10s: 3, avgMs: 40, p95Ms: 80, maxMs: 120 },
    ]);
    expect(row(s, "agent_output_emit")!.bytes10s).toBe(1_024_000);
    expect(row(s, "agent_scan")!.bytes10s).toBeNull(); // latency-only rows stay null
  });

  it("leaves exec/overhead null until a stat is pushed", () => {
    const s = new PerfStore();
    s.record("file_read", 7, true);
    const r = row(s, "file_read")!;
    expect(r.avgExec).toBeNull();
    expect(r.overhead).toBeNull();
  });

  it("windows latency stats so stale bursts age out of avg/p95/max", () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
    const s = new PerfStore();
    s.record("old", 50, true);
    vi.setSystemTime(11_000); // 11s later → outside the 10s window
    s.record("old", 8, true);
    const r = row(s, "old")!;
    expect(r.calls10s).toBe(1); // rate: only the fresh call is in-window
    expect(r.avgRt).toBe(8); // latency: windowed too — the 50ms relic is gone
    expect(r.maxRt).toBe(8);
    expect(r.stale).toBe(false);
  });

  it("falls back to the lifetime ring and flags stale when the window is empty", () => {
    vi.useFakeTimers();
    vi.setSystemTime(0);
    const s = new PerfStore();
    s.record("idle", 50, true);
    s.record("idle", 10, true);
    vi.setSystemTime(20_000); // both samples aged out
    s.tick();
    const r = row(s, "idle")!;
    expect(r.calls10s).toBe(0);
    expect(r.avgRt).toBe(30); // fallback: whole ring → (50 + 10) / 2
    expect(r.maxRt).toBe(50);
    expect(r.stale).toBe(true); // panel dims the frozen values
  });

  it("backend-only rows inherit staleness from the exec push", () => {
    const s = new PerfStore();
    s.setExec([
      { cmd: "fresh_emit", calls10s: 10, avgMs: 1, p95Ms: 2, maxMs: 3, stale: false },
      { cmd: "idle_emit", calls10s: 0, avgMs: 1, p95Ms: 2, maxMs: 3, stale: true },
    ]);
    expect(row(s, "fresh_emit")!.stale).toBe(false);
    expect(row(s, "idle_emit")!.stale).toBe(true);
  });

  it("a fresh round-trip keeps a row live even when the exec agg is stale", () => {
    const s = new PerfStore();
    s.record("c", 5, true);
    s.setExec([{ cmd: "c", calls10s: 0, avgMs: 4, p95Ms: 6, maxMs: 9, stale: true }]);
    expect(row(s, "c")!.stale).toBe(false);
  });

  it("clear() empties everything", () => {
    const s = new PerfStore();
    s.record("c", 5, true);
    s.setExec([{ cmd: "c", calls10s: 1, avgMs: 4, p95Ms: 6, maxMs: 9 }]);
    s.clear();
    expect(s.rows()).toEqual([]);
  });
});
