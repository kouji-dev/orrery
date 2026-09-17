# perf-smoke

Standalone load harness for Orrery PTY perf budgets.

## Run steps

1. **Start app** in dev mode: `npm run dev`
2. **Create N agents** using command:
   ```
   node tools/perf-smoke/noise.mjs
   ```
   Optional flags: `--bytes-per-sec 50000 --mode redraw|lines --duration-sec 60`
3. **Run all agents**, type in one, let run for ≥10s
4. **Export capture**: Dev Panel → perf tab → Export JSON → save as `capture.json`
5. **Assert budgets**:
   ```
   pnpm perf:assert capture.json
   ```
   Or directly: `node tools/perf-smoke/assert.mjs capture.json`  
   Custom budgets: `--budgets my-budgets.json`

## Default budgets

| cmd               | metric    | limit  |
|-------------------|-----------|--------|
| agent_input       | p95Rt     | ≤25ms  |
| agent_resize      | p95Rt     | ≤25ms  |
| agent_output_emit | calls10s  | ≤750   |
| js_longtask       | maxRt     | ≤100ms (skip if absent) |
| any non-stale row | errPct    | ==0    |

## npm shortcuts

```
npm run perf:noise    # start noise generator
npm run perf:assert   # assert (pass capture.json manually)
```

## Self-test

```
node tools/perf-smoke/assert.mjs --self-test
```
