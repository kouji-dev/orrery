#!/usr/bin/env node
// noise.mjs — synthetic PTY load generator, zero deps, Node >=18
// Flags: --bytes-per-sec N, --mode lines|redraw, --duration-sec N

const args = process.argv.slice(2);

function usage() {
  console.error('Usage: node noise.mjs [--bytes-per-sec N] [--mode lines|redraw] [--duration-sec N]');
  process.exit(1);
}

function getNum(flag, def) {
  const i = args.indexOf(flag);
  if (i === -1) return def;
  const raw = args[i + 1];
  if (raw === undefined || raw.startsWith('--')) {
    console.error(`noise: missing value for ${flag}`);
    usage();
  }
  const n = Number(raw);
  if (!Number.isFinite(n)) {
    console.error(`noise: non-numeric value for ${flag}: ${raw}`);
    usage();
  }
  return n;
}

const get = (flag, def) => {
  const i = args.indexOf(flag);
  return i !== -1 ? args[i + 1] : def;
};

const BPS       = getNum('--bytes-per-sec', 50000);
const MODE      = get('--mode',         'redraw');
const DURATION  = getNum('--duration-sec',  0); // 0 = forever

const TICK_MS   = 50;
const ROWS      = 40;
const COLS      = 120;

// ANSI helpers
const ESC = '\x1b';
const HOME_CLEAR = `${ESC}[H${ESC}[2J`;

let tick = 0;
let bytesSent = 0;
const startMs = Date.now();

function makeFrame() {
  const lines = [];
  for (let r = 0; r < ROWS; r++) {
    const label = `[agent] row=${String(r).padStart(2,'0')} tick=${String(tick).padStart(6,'0')} `;
    const pad = COLS - label.length;
    lines.push(label + (pad > 0 ? 'x'.repeat(pad) : ''));
  }
  return HOME_CLEAR + lines.join('\r\n');
}

function makeLine() {
  const ts = new Date().toISOString();
  const msg = `[agent] ${ts} tick=${tick} data=${'a'.repeat(64)}\n`;
  return msg;
}

const bytesPerTick = Math.round((BPS / 1000) * TICK_MS);

const iv = setInterval(() => {
  tick++;

  const elapsed = (Date.now() - startMs) / 1000;
  if (DURATION > 0 && elapsed >= DURATION) {
    clearInterval(iv);
    process.stdout.write('\n');
    process.exit(0);
  }

  const chunk = MODE === 'lines' ? makeLine() : makeFrame();
  // Emit only up to bytesPerTick worth per tick to pace throughput
  // chunk is ASCII-only, so slice(0, N) counts code units == bytes
  const slice = chunk.slice(0, bytesPerTick);
  process.stdout.write(slice);
  bytesSent += Buffer.byteLength(slice, 'utf8');
}, TICK_MS);

// Graceful exit on Ctrl-C
process.on('SIGINT', () => {
  clearInterval(iv);
  process.stderr.write(`\nnoise: sent ${bytesSent} bytes in ${((Date.now()-startMs)/1000).toFixed(1)}s\n`);
  process.exit(0);
});
