import { Injectable, computed, signal } from "@angular/core";

/** Rolling window for rate/aggregates (spec: 10s). */
export const PERF_WINDOW_MS = 10_000;
/** Per-command sample ring cap — enough for p95/max + a 16-point sparkline. */
const RING = 128;
/** Points kept for the sparkline trend. */
const HIST = 16;
/** Recent calls shown in a command's expand detail (dev tier). */
const RECENT = 9;

export interface PerfSample {
  /** epoch ms */ ts: number;
  /** round-trip ms */ ms: number;
  ok: boolean;
}

/** Backend exec aggregate for one command, pushed on `perf://stats`. */
export interface ExecAgg {
  cmd: string;
  avgMs: number;
  p95Ms: number;
  maxMs: number;
}

/** One row in the perf table — round-trip (frontend) merged with exec (backend). */
export interface PerfRow {
  cmd: string;
  calls10s: number;
  avgRt: number | null;
  p95Rt: number | null;
  maxRt: number | null;
  errPct: number;
  avgExec: number | null;
  /** round-trip − exec, i.e. IPC overhead. null until exec is known. */
  overhead: number | null;
  /** recent round-trip values for the sparkline (oldest→newest). */
  hist: number[];
  /** most-recent calls first, for the expand detail. */
  recent: PerfSample[];
}

function percentile(sortedAsc: number[], p: number): number {
  if (!sortedAsc.length) return 0;
  const idx = Math.ceil(p * sortedAsc.length) - 1;
  return sortedAsc[Math.min(sortedAsc.length - 1, Math.max(0, idx))];
}

/**
 * Per-command round-trip metrics, recorded by {@link InstrumentedBridge} and
 * merged with backend exec aggregates from the `perf://stats` event. Aggregates
 * are computed over a rolling {@link PERF_WINDOW_MS} window. In-memory only —
 * nothing persisted, nothing uploaded.
 */
@Injectable({ providedIn: "root" })
export class PerfStore {
  private readonly rings = new Map<string, PerfSample[]>();
  private readonly exec = signal(new Map<string, ExecAgg>());
  /** Bumped on every record / exec push / clear so `rows` recomputes. */
  private readonly rev = signal(0);

  /** Record one command round-trip. Called by the instrumented bridge. */
  record(cmd: string, ms: number, ok: boolean): void {
    let ring = this.rings.get(cmd);
    if (!ring) {
      ring = [];
      this.rings.set(cmd, ring);
    }
    ring.push({ ts: Date.now(), ms, ok });
    if (ring.length > RING) ring.shift();
    this.rev.update((v) => v + 1);
  }

  /** Merge backend exec aggregates (from `perf://stats`). */
  setExec(aggs: ExecAgg[]): void {
    this.exec.set(new Map(aggs.map((a) => [a.cmd, a])));
    this.rev.update((v) => v + 1);
  }

  /** Force `rows` to recompute so the 10s window ages out without new calls.
   *  The DevConsole calls this on a 1s timer while it's open. */
  tick(): void {
    this.rev.update((v) => v + 1);
  }

  /** Reset all counters (the panel's Reset button). */
  clear(): void {
    this.rings.clear();
    this.exec.set(new Map());
    this.rev.update((v) => v + 1);
  }

  readonly rows = computed<PerfRow[]>(() => {
    this.rev(); // recompute trigger
    const now = Date.now();
    const exec = this.exec();
    const cmds = new Set<string>([...this.rings.keys(), ...exec.keys()]);
    const out: PerfRow[] = [];
    for (const cmd of cmds) {
      const ring = this.rings.get(cmd) ?? [];
      const win = ring.filter((s) => s.ts > now - PERF_WINDOW_MS);
      const ms = win.map((s) => s.ms).sort((a, b) => a - b);
      const ex = exec.get(cmd);
      const avgRt = ms.length ? ms.reduce((a, b) => a + b, 0) / ms.length : null;
      const avgExec = ex ? ex.avgMs : null;
      out.push({
        cmd,
        calls10s: win.length,
        avgRt,
        p95Rt: ms.length ? percentile(ms, 0.95) : null,
        maxRt: ms.length ? ms[ms.length - 1] : null,
        errPct: win.length ? (win.filter((s) => !s.ok).length / win.length) * 100 : 0,
        avgExec,
        overhead: avgRt != null && avgExec != null ? avgRt - avgExec : null,
        hist: ring.slice(-HIST).map((s) => s.ms),
        recent: ring.slice(-RECENT).reverse(),
      });
    }
    return out;
  });
}
