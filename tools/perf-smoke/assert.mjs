#!/usr/bin/env node
// assert.mjs — perf budget assertion for dev-panel capture JSON
// Usage: node assert.mjs <capture.json> [--budgets budgets.json] [--self-test]

import { readFileSync } from 'node:fs';
import { resolve } from 'node:path';

// ── Default budgets ───────────────────────────────────────────────────────────
const DEFAULT_BUDGETS = {
  agent_input:        { p95Rt: 25 },
  agent_resize:       { p95Rt: 25 },
  agent_output_emit:  { calls10s: 750 },
  js_longtask:        { maxRt: 100, skipIfAbsent: true },
};

// ── Self-test fixtures ────────────────────────────────────────────────────────
const PASS_FIXTURE = {
  capturedAt: '2026-06-10T00:00:00Z',
  tier: 'dev',
  windowMs: 10000,
  rows: [
    { cmd: 'agent_input',       calls10s: 10,  avgRt: 5,  avgExec: 0.1, overhead: 4.9,  p95Rt: 20,  maxRt: 24,  errPct: 0 },
    { cmd: 'agent_resize',      calls10s: 5,   avgRt: 4,  avgExec: 0.1, overhead: 3.9,  p95Rt: 18,  maxRt: 22,  errPct: 0 },
    { cmd: 'agent_output_emit', calls10s: 500, avgRt: 2,  avgExec: 0.1, overhead: 1.9,  p95Rt: 5,   maxRt: 8,   errPct: 0 },
  ],
};

const FAIL_FIXTURE = {
  capturedAt: '2026-06-10T00:00:00Z',
  tier: 'dev',
  windowMs: 10000,
  rows: [
    { cmd: 'agent_input',       calls10s: 10,  avgRt: 50, avgExec: 0.1, overhead: 49.9, p95Rt: 300, maxRt: 400, errPct: 0 },  // p95Rt violation
    { cmd: 'agent_resize',      calls10s: 5,   avgRt: 4,  avgExec: 0.1, overhead: 3.9,  p95Rt: 18,  maxRt: 22,  errPct: 0 },
    { cmd: 'agent_output_emit', calls10s: 900, avgRt: 2,  avgExec: 0.1, overhead: 1.9,  p95Rt: 5,   maxRt: 8,   errPct: 0 },  // calls10s violation
    { cmd: 'some_other_cmd',    calls10s: 2,   avgRt: 2,  avgExec: 0.1, overhead: 1.9,  p95Rt: 5,   maxRt: 8,   errPct: 5 },  // errPct violation
  ],
};

// ── Helpers ───────────────────────────────────────────────────────────────────
function parseArgs() {
  const argv = process.argv.slice(2);
  const flags = {};
  let captureArg = null;
  for (let i = 0; i < argv.length; i++) {
    if (argv[i] === '--budgets')   { flags.budgets = argv[++i]; continue; }
    if (argv[i] === '--self-test') { flags.selfTest = true; continue; }
    if (!argv[i].startsWith('--')) { captureArg = argv[i]; }
  }
  return { captureArg, ...flags };
}

function assertCapture(data, budgets) {
  const rows = data.rows ?? [];
  const results = [];
  let failed = false;

  // Budget-based checks
  for (const [cmd, budget] of Object.entries(budgets)) {
    const row = rows.find(r => r.cmd === cmd && !r.stale);
    if (!row) {
      if (budget.skipIfAbsent) {
        results.push({ cmd, metric: '(absent)', budget: 'skip', actual: 'n/a', pass: true });
      } else {
        results.push({ cmd, metric: 'row', budget: 'present', actual: 'MISSING', pass: false });
        failed = true;
      }
      continue;
    }
    for (const [metric, limit] of Object.entries(budget)) {
      if (metric === 'skipIfAbsent') continue;
      const actual = row[metric];
      const pass = actual !== undefined && actual <= limit;
      if (!pass) failed = true;
      results.push({ cmd, metric, budget: `<=${limit}`, actual: actual ?? 'n/a', pass });
    }
  }

  // errPct==0 for every non-stale row
  for (const row of rows) {
    if (row.stale) continue;
    const pass = row.errPct === 0;
    if (!pass) failed = true;
    results.push({ cmd: row.cmd, metric: 'errPct', budget: '==0', actual: row.errPct, pass });
  }

  return { results, failed };
}

function printTable(results) {
  const w = [28, 16, 12, 12, 6];
  const head = ['CMD', 'METRIC', 'BUDGET', 'ACTUAL', 'PASS'];
  const line = (cols) => cols.map((c, i) => String(c).padEnd(w[i])).join('  ');
  const sep = w.map(n => '-'.repeat(n)).join('  ');
  console.log(line(head));
  console.log(sep);
  for (const r of results) {
    const tick = r.pass ? 'PASS' : 'FAIL';
    console.log(line([r.cmd, r.metric, r.budget, r.actual, tick]));
  }
}

// ── Self-test ─────────────────────────────────────────────────────────────────
function runSelfTest() {
  console.log('=== self-test PASS fixture ===');
  const pr = assertCapture(PASS_FIXTURE, DEFAULT_BUDGETS);
  printTable(pr.results);
  if (pr.failed) { console.error('SELF-TEST ERROR: pass fixture should not fail'); process.exit(1); }

  console.log('\n=== self-test FAIL fixture ===');
  const fr = assertCapture(FAIL_FIXTURE, DEFAULT_BUDGETS);
  printTable(fr.results);
  if (!fr.failed) { console.error('SELF-TEST ERROR: fail fixture should have failures'); process.exit(1); }

  // Verify specific expected violations
  const violations = fr.results.filter(r => !r.pass);
  const hasP95 = violations.some(r => r.cmd === 'agent_input' && r.metric === 'p95Rt');
  const hasCalls = violations.some(r => r.cmd === 'agent_output_emit' && r.metric === 'calls10s');
  const hasErrPct = violations.some(r => r.cmd === 'some_other_cmd' && r.metric === 'errPct');
  if (!hasP95 || !hasCalls || !hasErrPct) {
    console.error('SELF-TEST ERROR: expected violations not detected');
    process.exit(1);
  }

  console.log('\nself-test PASSED');
  process.exit(0);
}

// ── Main ──────────────────────────────────────────────────────────────────────
const { captureArg, budgets: budgetsPath, selfTest } = parseArgs();

if (selfTest) {
  runSelfTest();
}

if (!captureArg) {
  console.error('Usage: node assert.mjs <capture.json> [--budgets budgets.json] [--self-test]');
  process.exit(1);
}

const capturePath = resolve(captureArg);
let data;
try {
  data = JSON.parse(readFileSync(capturePath, 'utf8'));
} catch (e) {
  console.error(`Failed to read capture file: ${e.message}`);
  process.exit(1);
}

let budgets = DEFAULT_BUDGETS;
if (budgetsPath) {
  try {
    budgets = JSON.parse(readFileSync(resolve(budgetsPath), 'utf8'));
  } catch (e) {
    console.error(`Failed to read budgets file: ${e.message}`);
    process.exit(1);
  }
}

console.log(`Capture: ${capturePath}`);
console.log(`CapturedAt: ${data.capturedAt ?? 'unknown'}  windowMs: ${data.windowMs ?? '?'}`);
console.log();

const { results, failed } = assertCapture(data, budgets);
printTable(results);

if (failed) {
  console.log('\nRESULT: FAIL — budget violations detected');
  process.exit(1);
} else {
  console.log('\nRESULT: PASS');
  process.exit(0);
}
