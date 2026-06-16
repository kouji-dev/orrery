// tools/density/check-tokens.mjs
import { readFileSync, readdirSync, statSync } from "node:fs";
import { join, resolve } from "node:path";

// Properties whose px literals MUST be tokens. max-height/width are out of scope.
const TARGET = [
  "padding","padding-top","padding-right","padding-bottom","padding-left",
  "padding-inline","padding-inline-start","padding-inline-end",
  "padding-block","padding-block-start","padding-block-end",
  "margin","margin-top","margin-right","margin-bottom","margin-left",
  "margin-inline","margin-inline-start","margin-inline-end",
  "margin-block","margin-block-start","margin-block-end",
  "gap","row-gap","column-gap","height","min-height","font-size",
];
const PROPS = TARGET.slice().sort((a, b) => b.length - a.length).join("|");
const RE = new RegExp(`(?<![\\w-])(${PROPS})\\s*:\\s*([^;"'\`}]*)`, "g");
const PX = /-?\d*\.?\d+px/g;
const ALLOW_PX = new Set(["0px", "1px", "999px"]);

// Documented exceptions: { file: <substring>, value: <substring> }. Bespoke
// structural heights that intentionally stay fixed go here (populated in sweep).
export const ALLOWLIST = [
  // .rail-btn is a compact sidebar icon button whose square 38×38 size is a
  // deliberate visual design constant — not a control height token.
  { file: "styles.css", value: "38px" },
];

export function scanText(text, file = "") {
  const out = [];
  for (const m of text.matchAll(RE)) {
    const [, prop, rawVal] = m;
    const val = rawVal.trim();
    const pxs = (val.match(PX) || []).map((s) => s.replace("-", ""));
    const bad = pxs.filter((p) => !ALLOW_PX.has(p));
    if (!bad.length) continue;
    if (ALLOWLIST.some((a) => file.includes(a.file) && val.includes(a.value))) continue;
    out.push(`${prop}: ${val}`);
  }
  return out;
}

function walk(p, acc = []) {
  const s = statSync(p);
  if (s.isDirectory()) {
    for (const e of readdirSync(p)) walk(join(p, e), acc);
  } else if (/\.(ts|css)$/.test(p) && !/\.spec\.ts$/.test(p)) {
    acc.push(p);
  }
  return acc;
}

const ROOT = resolve(import.meta.dirname, "../..");
export function defaultFiles() {
  return [resolve(ROOT, "src/styles.css"), ...walk(resolve(ROOT, "src/app"))];
}

export function scanRepo(files = defaultFiles()) {
  const byFile = {};
  let total = 0;
  for (const f of files) {
    const v = scanText(readFileSync(f, "utf8"), f);
    if (v.length) { byFile[f] = v; total += v.length; }
  }
  return { total, byFile };
}

// CLI: `node check-tokens.mjs [paths...]`
if (process.argv[1] && process.argv[1].endsWith("check-tokens.mjs")) {
  const args = process.argv.slice(2);
  const files = args.length ? args.flatMap((a) => walk(resolve(a))) : defaultFiles();
  const { total, byFile } = scanRepo(files);
  for (const [f, vs] of Object.entries(byFile)) {
    console.log(`\n${f}  (${vs.length})`);
    for (const v of vs.slice(0, 50)) console.log(`  ${v}`);
  }
  console.log(`\n${total} violations`);
  process.exit(total ? 1 : 0);
}
