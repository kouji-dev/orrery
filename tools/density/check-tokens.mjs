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
  // .tweak-fab svg icon (19px) and .toggle switch (19px) are bespoke visual
  // constants: the icon mirrors its SVG viewport and the toggle thumb track is a
  // fixed-size pill — neither maps to a control-height or spacing token.
  { file: "tweaks-panel.component.ts", value: "19px" },
  // "Run all / Pause all" button in the top-bar is 25px tall — sits between the
  // ctl-h-sm (22px) and ctl-h (26px) control-height tokens and is an intentional
  // compact inline action that doesn't map to a standard control-height token.
  { file: "top-bar.component.ts", value: "25px" },
  // .rte-btn toolbar buttons are 27px square — sits between ctl-h-sm (22px) and
  // ctl-h (26/28px); the square icon-button size is a deliberate design constant
  // for the rich editor toolbar and does not map to a standard control-height token.
  { file: "rich-editor.component.ts", value: "27px" },
  // Version badge pill height is 17px — intentionally slimmer than ctl-h-sm (22px)
  // and taller than the sp-* spacing scale (sp-7≈16px); it is a fixed visual
  // constant for the header badge and does not map to a control-height token.
  { file: "version-badge.component.ts", value: "17px" },
  // mini-term preview container is exactly 60px = 3 rows × (10px × 1.6 line-height)
  // + 12px vertical padding (2×6px). This is a deliberately calculated fixed layout
  // height — not a control-height token — so that idle and streaming cards are
  // pixel-identical regardless of content.
  { file: "mini-term.component.ts", value: "60px" },
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
