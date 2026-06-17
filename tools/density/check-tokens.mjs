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
  // .toggle knob is a fixed 13x13 circle matching the fixed-geometry switch track
  // (34x19 + 2px inset + 15px travel). Its width/height must stay equal and fixed
  // so the thumb stays round; scaling only height made it oval.
  { file: "tweaks-panel.component.ts", value: "13px" },
  // The old "Run all / Pause all" standalone button (25px) was replaced by the
  // icon-only .pill-seg square buttons that match --row-h. No entry needed.
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
  // Popover project-icon badge in compact-rail is 17px — sits between sp-7 (16px)
  // and ctl-h-sm (22px); it is a bespoke fixed square that mirrors the SVG icon
  // viewport and does not map to a standard control-height or spacing token.
  { file: "compact-rail.component.ts", value: "17px" },
  // Project-icon badge in project-group is 19px — sits between sp-7 (16px) and
  // ctl-h-sm (22px); it is a bespoke fixed square for the sidebar project row icon
  // and does not map to a standard control-height or spacing token.
  { file: "project-group.component.ts", value: "19px" },
  // Right-panel agent-header is 38px tall — one pixel above ctl-h-lg (34px) and
  // below topbar-h (44px); it is a deliberate structural panel-header height that
  // mirrors the left-panel rail and does not snap to a standard control-height token.
  { file: "right-panel.component.ts", value: "38px" },
  // Right-panel tab-bar is 36px tall — sits between ctl-h-lg (34px) and topbar-h
  // (44px); it is a bespoke structural tab-bar height intentionally distinct from
  // the control-height scale.
  { file: "right-panel.component.ts", value: "36px" },
  // Empty-state icon container in backlog is 52×52px with border-radius:14px — a
  // bespoke decorative square that sits well above ctl-h-lg (34px) and topbar-h
  // (44px); it does not map to any control-height token and its fixed size is a
  // deliberate visual design constant.
  { file: "backlog.component.ts", value: "52px" },
  // Project icon badge in ticket-page is 19×19px with border-radius:5px — sits
  // between sp-7 (16px) and ctl-h-sm (22px); it is a bespoke fixed square that
  // mirrors the SVG icon viewport and does not map to a standard control-height or
  // spacing token.
  { file: "ticket-page.component.ts", value: "19px" },
  // Dev-panel FAB svg icon (19px) mirrors the SVG viewport of the line-chart icon
  // inside the 44px FAB button — a bespoke visual constant that does not map to
  // a control-height or spacing token.
  { file: "dev-panel.component.ts", value: "19px" },
  // Dev-panel alert badge (.dvc-badge) is 17px tall — intentionally slimmer than
  // ctl-h-sm (22px) and above sp-7 (≈16px); it is a fixed pill badge matching the
  // FAB corner overlay and does not map to a standard control-height token.
  // .dvc-tool and .dvc-kind are 17×17px square icon containers — between sp-7 (16px)
  // and ctl-h-sm (22px); they mirror their SVG icon viewports and are bespoke fixed
  // squares that do not snap to a standard control-height token.
  { file: "dev-panel.component.ts", value: "17px" },
  // .dvc-ring is a 46×46px dashed circular empty-state ring with border-radius:50%
  // — sits above topbar-h (44px); it is a decorative visual element and does not map
  // to any control-height token.
  { file: "dev-panel.component.ts", value: "46px" },
  // .set-tgl is a tightly-coupled toggle mini-widget: the 34×19 track pill and the
  // 15×15 circular thumb form a fixed-geometry unit where all dims must stay equal
  // and fixed so the thumb remains round and travels exactly 15px (34−15−2×2=15).
  // Scaling height alone would distort the thumb into an oval. Kept fully fixed.
  { file: "settings-modal.component.ts", value: "19px" },
  { file: "settings-modal.component.ts", value: "15px" },
  // .set-modal height is a fixed structural viewport constraint (600px) — the modal
  // is designed for an 84vh max with a 600px preferred height. This is not a control
  // height token; it is a bespoke panel dimension.
  { file: "settings-modal.component.ts", value: "600px" },
  // .set-nav-item height is 35px — sits between ctl-h-lg (34px) and topbar-h (44px).
  // It is a bespoke nav sidebar row height that intentionally exceeds ctl-h-lg by 1px
  // for comfortable tap targets and does not map to a standard control-height token.
  { file: "settings-modal.component.ts", value: "35px" },
  // .set-slider track (4px) and thumb (14×14) are a tightly-coupled fixed-geometry
  // slider mini-widget. The 128px track width and 14px thumb diameter determine the
  // slider's travel range (128−14=114px); altering any single dimension independently
  // breaks the visual and interaction geometry. All dims kept fully fixed.
  { file: "settings-modal.component.ts", value: "4px" },
  { file: "settings-modal.component.ts", value: "14px" },
  // file-blame is a fixed-metric CODE viewer: the row height (21px) equals the
  // virtual-scroll itemSize and the monospace line metrics (12px) drive it — like
  // the terminal / CodeMirror diff, code surfaces are not density-scaled. The
  // gutter label/spacing px live inside that fixed 21px row, so they stay fixed too.
  { file: "file-blame.component.ts", value: "12px" },
  { file: "file-blame.component.ts", value: "21px" },
  { file: "file-blame.component.ts", value: "10.5px" },
  { file: "file-blame.component.ts", value: "11px" },
  { file: "file-blame.component.ts", value: "10px" },
  { file: "file-blame.component.ts", value: "9px" },
  { file: "file-blame.component.ts", value: "7px" },
  { file: "file-blame.component.ts", value: "5px" },
  { file: "file-blame.component.ts", value: "14px" },
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
