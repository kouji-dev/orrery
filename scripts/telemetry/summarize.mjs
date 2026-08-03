#!/usr/bin/env node
// A0.7 Phase 1 deliverable: turn a day's raw emit trace (NDJSON: one
// `{ts,name,key?,bytes}` line per emit) into the inventory table that feeds
// the Phase-2 class decision (roadmap open decision 8):
//
//   event name · count · total bytes · p50/p95 payload · peak rate · suggested class
//
// Usage:
//   node scripts/telemetry/summarize.mjs [path-to-ndjson]
//   node scripts/telemetry/summarize.mjs            # today's file in app data
//
// Privacy note: the input already contains names/keys/byte counts ONLY —
// payload contents are never written by the funnel, so nothing here can leak.

import { readFileSync, existsSync, readdirSync } from "node:fs";
import { join } from "node:path";

function defaultFile() {
  // %APPDATA%/com.kouji.orrery/telemetry/emits-YYYY-MM-DD.ndjson
  const base =
    process.platform === "win32"
      ? join(process.env.APPDATA ?? "", "com.kouji.orrery", "telemetry")
      : join(process.env.HOME ?? "", ".local", "share", "com.kouji.orrery", "telemetry");
  const day = new Date().toISOString().slice(0, 10);
  const todays = join(base, `emits-${day}.ndjson`);
  if (existsSync(todays)) return todays;
  // fall back to the newest trace present
  if (existsSync(base)) {
    const traces = readdirSync(base).filter((f) => f.startsWith("emits-") && f.endsWith(".ndjson")).sort();
    if (traces.length) return join(base, traces[traces.length - 1]);
  }
  return todays; // will produce the "not found" error below
}

const file = process.argv[2] ?? defaultFile();
if (!existsSync(file)) {
  console.error(`no trace file at ${file}`);
  console.error("the raw trace is opt-in — enable it in Settings → Permissions & safety → Diagnostics");
  process.exit(1);
}

/** @type {Map<string, {count:number,total:number,sizes:number[],perSec:Map<number,number>}>} */
const byName = new Map();
let lines = 0;
let badLines = 0;

for (const line of readFileSync(file, "utf8").split("\n")) {
  if (!line.trim()) continue;
  lines++;
  let e;
  try {
    e = JSON.parse(line);
  } catch {
    badLines++; // torn final line from a crash — expected, skip
    continue;
  }
  let agg = byName.get(e.name);
  if (!agg) {
    agg = { count: 0, total: 0, sizes: [], perSec: new Map() };
    byName.set(e.name, agg);
  }
  agg.count++;
  agg.total += e.bytes;
  agg.sizes.push(e.bytes);
  const sec = Math.floor(e.ts / 1000);
  agg.perSec.set(sec, (agg.perSec.get(sec) ?? 0) + 1);
}

const pct = (sorted, p) => (sorted.length ? sorted[Math.min(sorted.length - 1, Math.ceil(p * sorted.length) - 1)] : 0);
const fmtB = (n) =>
  n >= 1e9 ? (n / 1e9).toFixed(2) + " GB" : n >= 1e6 ? (n / 1e6).toFixed(1) + " MB" : n >= 1e3 ? Math.round(n / 1e3) + " KB" : n + " B";

// Suggested Phase-2 class — a HEURISTIC seed for decision 8, not the decision:
// - tiny + rare → bypass (latency-critical, throttling buys nothing)
// - small + repetitive → coalesced state (only the latest value is true)
// - big or fast → bulk weighted (where byte quanta belong)
function suggestClass(p95, peak) {
  if (p95 <= 512 && peak <= 2) return "bypass";
  if (p95 <= 4096 && peak <= 30) return "coalesced state";
  return "bulk weighted";
}

const rows = [...byName.entries()]
  .map(([name, a]) => {
    const sorted = [...a.sizes].sort((x, y) => x - y);
    const peak = Math.max(0, ...a.perSec.values());
    const p95 = pct(sorted, 0.95);
    return {
      name,
      count: a.count,
      total: a.total,
      p50: pct(sorted, 0.5),
      p95,
      peak,
      cls: suggestClass(p95, peak),
    };
  })
  .sort((a, b) => b.total - a.total);

const cols = [
  ["event name", (r) => r.name, "left"],
  ["count", (r) => r.count.toLocaleString("en-US"), "right"],
  ["total bytes", (r) => fmtB(r.total), "right"],
  ["p50", (r) => fmtB(r.p50), "right"],
  ["p95", (r) => fmtB(r.p95), "right"],
  ["peak/s", (r) => String(r.peak), "right"],
  ["suggested class", (r) => r.cls, "left"],
];
const widths = cols.map(([h, f]) => Math.max(h.length, ...rows.map((r) => f(r).length)));
const pad = (s, w, align) => (align === "right" ? s.padStart(w) : s.padEnd(w));

console.log(`emit inventory — ${file}`);
console.log(`${lines.toLocaleString("en-US")} emits · ${byName.size} event names${badLines ? ` · ${badLines} torn lines skipped` : ""}\n`);
console.log(cols.map(([h], i) => pad(h, widths[i], cols[i][2])).join("  "));
console.log(widths.map((w) => "-".repeat(w)).join("  "));
for (const r of rows) console.log(cols.map(([, f], i) => pad(f(r), widths[i], cols[i][2])).join("  "));
console.log(
  "\nclasses are a heuristic seed for the Phase-2 taxonomy (roadmap decision 8) — decide from a week of data, not this one file.",
);
