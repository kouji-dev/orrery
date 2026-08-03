import { computed, inject, Injectable, signal } from "@angular/core";
import { CostEstimate, CostRate } from "../models";
import { SettingsStore } from "../settings/settings.store";

/** Which git operation an AI variant would perform (drives the heuristic). */
export type CostOpKind =
  | "commit"
  | "push"
  | "merge"
  | "rebase"
  | "conflict"
  | "hunk"
  | "cherrypick"
  | "revert"
  | "squash"
  | "branch";

export interface EstimateInput {
  op: CostOpKind;
  /** Files the model must reason about (changed / conflicted). */
  files?: number;
  /** Total diff bytes it must read. */
  diffBytes?: number;
  /** Conflicts it must resolve. */
  conflicts?: number;
  /** The agent's model id (rate-table key). */
  model?: string;
  /** Verbose variant multiplies the envelope (~1.7×). */
  verbose?: boolean;
}

/** Built-in provider rates ($/Mtok). The settings `costRates` map overrides
 *  per model id — rates change faster than releases, so the table is
 *  user-editable rather than hardcoded truth. */
export const DEFAULT_RATES: Record<string, CostRate> = {
  "opus-4.6": { in: 15, out: 75 },
  "sonnet-4.6": { in: 3, out: 15 },
  "haiku-4.6": { in: 0.8, out: 4 },
  "gpt-5.3-codex": { in: 2.5, out: 10 },
  "o5-mini": { in: 1.1, out: 4.4 },
  "composer-2": { in: 1.5, out: 6 },
  "gemini-3-pro": { in: 2, out: 12 },
  "gemini-3-flash": { in: 0.3, out: 1.2 },
  default: { in: 3, out: 15 },
};

// Per-op heuristic: a base token envelope + a per-KB term for the diff the
// model must read + per-conflict/per-file terms. Numbers are the design
// bundle's starting proposal (gitactions.jsx) — deliberately wide until
// calibration against actuals lands.
// Why these constants: lo/hi bracket observed tool transcripts for each op
// class; perKb ≈ tokens per KB of diff context (~4 bytes/token, read ~once).
const OP_SPEC: Record<CostOpKind, { lo: number; hi: number; perKb: number }> = {
  commit: { lo: 1200, hi: 2600, perKb: 260 },
  push: { lo: 400, hi: 900, perKb: 0 },
  merge: { lo: 4500, hi: 9000, perKb: 420 },
  rebase: { lo: 6000, hi: 13000, perKb: 520 },
  conflict: { lo: 2600, hi: 6200, perKb: 700 },
  hunk: { lo: 900, hi: 2100, perKb: 520 },
  cherrypick: { lo: 3000, hi: 7000, perKb: 450 },
  revert: { lo: 2500, hi: 6000, perKb: 400 },
  squash: { lo: 2200, hi: 5200, perKb: 300 },
  branch: { lo: 260, hi: 700, perKb: 0 },
};

// Why 800/1500: each conflict adds a read-think-write round over its hunk.
// Why 120/260: per-file overhead (path + hunk headers + tool call framing).
const PER_CONFLICT = { lo: 800, hi: 1500 };
const PER_FILE = { lo: 120, hi: 260 };
// Why 0.75/0.25: git-driving transcripts are read-heavy — roughly three
// quarters of the tokens are input (diffs, status, file reads).
const INPUT_SHARE = 0.75;

export const fmtTok = (n: number): string =>
  n >= 1000 ? (n / 1000).toFixed(n >= 10000 ? 0 : 1).replace(/\.0$/, "") + "k" : String(Math.round(n));
export const fmtUsd = (n: number): string => (n >= 0.01 ? "$" + n.toFixed(2) : "$" + n.toFixed(3));

/**
 * Static heuristic cost estimator for AI git actions (A4.3). Inputs: op kind,
 * file count, diff bytes, conflict count, model + the user-editable rate table
 * (settings). Output: a token/USD envelope shown ON the dropdown row before
 * anything runs. Also owns the minimal budget guard (A4.4): a session spend
 * tally against the settings cap, and the confirm-above threshold.
 *
 * TODO(calibration, A6): once the actuals ledger exists, use the observed
 * median for the op/model pair after N runs instead of this static heuristic
 * (open decision #5 — calibration window size).
 */
@Injectable({ providedIn: "root" })
export class EstimateService {
  private readonly settings = inject(SettingsStore);

  /** USD spent on AI git actions this app session, tallied from ESTIMATES at
   *  press time (no actuals ledger yet — see TODO above). */
  readonly sessionSpentUsd = signal(0);

  readonly capUsd = computed(() => this.settings.settings().budgetCapUsd);
  readonly confirmAboveUsd = computed(() => this.settings.settings().confirmAboveUsd);
  /** USD left under the cap; Infinity when no cap is set. */
  readonly remainingUsd = computed(() => {
    const cap = this.capUsd();
    return cap > 0 ? Math.max(0, cap - this.sessionSpentUsd()) : Infinity;
  });

  rateOf(model: string | undefined): CostRate {
    const table = { ...DEFAULT_RATES, ...this.settings.settings().costRates };
    return table[model ?? ""] ?? table["default"];
  }

  estimate(input: EstimateInput): CostEstimate {
    const spec = OP_SPEC[input.op] ?? OP_SPEC.commit;
    const files = input.files ?? 0;
    const conflicts = input.conflicts ?? 0;
    const kb = (input.diffBytes ?? 0) / 1024;
    let lo = spec.lo + spec.perKb * kb + conflicts * PER_CONFLICT.lo + files * PER_FILE.lo;
    // Why 1.6: the model often re-reads context after edits — the high bound
    // assumes the diff is visited more than once.
    let hi = spec.hi + spec.perKb * kb * 1.6 + conflicts * PER_CONFLICT.hi + files * PER_FILE.hi;
    if (input.verbose) {
      lo *= 1.7;
      hi *= 1.8;
    }
    const model = input.model ?? "default";
    const r = this.rateOf(model);
    const usd = (tok: number) => (tok * INPUT_SHARE * r.in + tok * (1 - INPUT_SHARE) * r.out) / 1e6;
    return {
      op: input.op,
      model,
      tokensLow: Math.round(lo),
      tokensHigh: Math.round(hi),
      usdLow: usd(lo),
      usdHigh: usd(hi),
      confidence: "heuristic",
    };
  }

  /** Would running this estimate exceed the remaining budget? (cap guard) */
  overCap(est: CostEstimate): boolean {
    return est.usdHigh > this.remainingUsd();
  }
  /** Does this estimate need a confirming second click? (confirm-above guard) */
  needsConfirm(est: CostEstimate): boolean {
    const t = this.confirmAboveUsd();
    return t > 0 && est.usdHigh > t;
  }
  /** Record a press against the session budget (estimate high bound — the
   *  honest stand-in until actuals land). */
  recordSpend(est: CostEstimate): void {
    this.sessionSpentUsd.update((v) => v + est.usdHigh);
  }
}
