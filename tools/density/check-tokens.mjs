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
  // Widths were out of scope until the type ramp was rebased, at which point
  // fixed-width containers started clipping their own contents (the tweaks
  // panel lost its third density option behind `width:288px;overflow:hidden`).
  // A container that holds text has to be free to grow with it.
  "width","min-width","max-width","max-height",
];
const PROPS = TARGET.slice().sort((a, b) => b.length - a.length).join("|");
const RE = new RegExp(`(?<![\\w-])(${PROPS})\\s*:\\s*([^;"'\`}]*)`, "g");
const PX = /-?\d*\.?\d+px/g;
const ALLOW_PX = new Set(["0px", "1px", "999px"]);

// Documented exceptions: { file: <substring>, value: <substring> }. Bespoke
// structural heights that intentionally stay fixed go here (populated in sweep).
export const ALLOWLIST = [
  // ---- recipes that moved into styles.css during the type-system sweep ----
  // .fab svg mirrors its 19px SVG viewport (was tweaks-panel's .tweak-fab).
  { file: "styles.css", value: "19px" },
  // The version pill is a fixed 17px badge with a 5px gutter between version,
  // divider and channel tag (design/app.html:4229-4235) — deliberately slimmer
  // than any control-height token. Skin moved here from version-badge.
  { file: "styles.css", value: "17px" },
  { file: "styles.css", value: "5px" },
  // Its 1x9px hairline divider is drawn geometry, not spacing.
  { file: "version-badge.component.ts", value: "9px" },

  // ---- design-exact geometry copied from the mockup ----
  // The update toast is pixel-specified in design/app.html:11512-11526: a
  // 13/14/16px padding box with 14px and 7px gutters. These are one bespoke
  // card's proportions, not steps on the spacing scale.
  { file: "update-toast.component.ts", value: "13px" },
  { file: "update-toast.component.ts", value: "14px" },
  { file: "update-toast.component.ts", value: "16px" },
  { file: "update-toast.component.ts", value: "7px" },
  // Rail count bubble: a 13px circle with 3px gutters that overlaps the icon
  // corner (design/app.html:4711). Scaling it detaches it from the glyph.
  { file: "compact-rail.component.ts", value: "13px" },
  { file: "compact-rail.component.ts", value: "3px" },
  // NavRow current-row bar: 2.5px is a hairline-plus, deliberately between
  // --sp-1 (2px) and --sp-2 (4px) (design/app.html:4576).
  { file: "sidebar.component.ts", value: "2.5px" },

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
  // review-code is a fixed-metric CODE viewer (inline-review renderer): row
  // heights, gutter widths, font sizes, and chip dimensions are driven by
  // monospace line metrics and design-reference constants — like the diff /
  // file-blame surfaces, code viewers are intentionally NOT density-scaled.
  { file: "review-code.component.ts", value: "5px" },
  { file: "review-code.component.ts", value: "7px" },
  { file: "review-code.component.ts", value: "8px" },
  { file: "review-code.component.ts", value: "8.5px" },
  { file: "review-code.component.ts", value: "9px" },
  { file: "review-code.component.ts", value: "9.5px" },
  { file: "review-code.component.ts", value: "10px" },
  { file: "review-code.component.ts", value: "10.5px" },
  { file: "review-code.component.ts", value: "11px" },
  { file: "review-code.component.ts", value: "12px" },
  { file: "review-code.component.ts", value: "14px" },
  { file: "review-code.component.ts", value: "15px" },
  { file: "review-code.component.ts", value: "16px" },
  { file: "review-code.component.ts", value: "17px" },
  { file: "review-code.component.ts", value: "38px" },
  { file: "review-code.component.ts", value: "56px" },
  // annotate-blame is a fixed-metric CODE viewer (annotate/blame renderer): author
  // gutter widths (196px, 44px), font sizes, chip dimensions, and popup geometry
  // are ported 1:1 from the design reference. Like file-blame and review-code,
  // code surfaces are intentionally NOT density-scaled.
  { file: "annotate-blame.component.ts", value: "4px" },
  { file: "annotate-blame.component.ts", value: "5px" },
  { file: "annotate-blame.component.ts", value: "6px" },
  { file: "annotate-blame.component.ts", value: "7px" },
  { file: "annotate-blame.component.ts", value: "8px" },
  { file: "annotate-blame.component.ts", value: "9px" },
  { file: "annotate-blame.component.ts", value: "9.5px" },
  { file: "annotate-blame.component.ts", value: "10px" },
  { file: "annotate-blame.component.ts", value: "10.5px" },
  { file: "annotate-blame.component.ts", value: "11px" },
  { file: "annotate-blame.component.ts", value: "12px" },
  { file: "annotate-blame.component.ts", value: "14px" },
  // review-comments.monaco is the Monaco port of the inline-review comment UX
  // (cards, composer, gutter glyphs). Its geometry is ported 1:1 from the CM
  // extension / design reference — like review-code and annotate-blame, code
  // surfaces are intentionally NOT density-scaled.
  { file: "review-comments.monaco.ts", value: "3px" },
  { file: "review-comments.monaco.ts", value: "4px" },
  { file: "review-comments.monaco.ts", value: "5px" },
  { file: "review-comments.monaco.ts", value: "7px" },
  { file: "review-comments.monaco.ts", value: "8px" },
  { file: "review-comments.monaco.ts", value: "8.5px" },
  { file: "review-comments.monaco.ts", value: "9px" },
  { file: "review-comments.monaco.ts", value: "9.5px" },
  { file: "review-comments.monaco.ts", value: "10px" },
  { file: "review-comments.monaco.ts", value: "10.5px" },
  { file: "review-comments.monaco.ts", value: "11px" },
  { file: "review-comments.monaco.ts", value: "12px" },
  { file: "review-comments.monaco.ts", value: "14px" },
  { file: "review-comments.monaco.ts", value: "17px" },
  { file: "review-comments.monaco.ts", value: "56px" },
  // send-review modal is a fixed-metric UI panel: the grouped-file list, comment
  // rows, global-note textarea, and footer all carry bespoke pixel geometry ported
  // 1:1 from the design reference. Like review-code, the comment-list surface is
  // intentionally NOT density-scaled — comment card heights, gutter widths and
  // font sizes must stay visually stable across density modes.
  { file: "send-review.component.ts", value: "2px" },
  { file: "send-review.component.ts", value: "3px" },
  { file: "send-review.component.ts", value: "4px" },
  { file: "send-review.component.ts", value: "5px" },
  { file: "send-review.component.ts", value: "6px" },
  { file: "send-review.component.ts", value: "7px" },
  { file: "send-review.component.ts", value: "8px" },
  { file: "send-review.component.ts", value: "9px" },
  { file: "send-review.component.ts", value: "9.5px" },
  { file: "send-review.component.ts", value: "10px" },
  { file: "send-review.component.ts", value: "10.5px" },
  { file: "send-review.component.ts", value: "11px" },
  { file: "send-review.component.ts", value: "11.5px" },
  { file: "send-review.component.ts", value: "12px" },
  { file: "send-review.component.ts", value: "14px" },
  { file: "send-review.component.ts", value: "18px" },
  { file: "send-review.component.ts", value: "24px" },
  { file: "send-review.component.ts", value: "40px" },
  // The update toast + What's-new digest are bespoke fixed-metric overlay panels
  // (icon boxes, button heights, badge/chip padding, the hero accent line) ported
  // pixel-for-pixel from the design — small visual constants, not control-height /
  // spacing tokens.
  { file: "update-toast.component.ts", value: "34px" },
  { file: "update-toast.component.ts", value: "28px" },
  { file: "update-toast.component.ts", value: "2px" },
  { file: "whats-new-modal.component.ts", value: "28px" },
  { file: "whats-new-modal.component.ts", value: "6px" },
  { file: "whats-new-modal.component.ts", value: "3px" },
  { file: "whats-new-modal.component.ts", value: "2px" },
  // Find-in-files results list: 180px min-height is a structural viewport
  // constraint for the overlay's results area (like .set-modal's 600px); the
  // 26px match-row left indent aligns match text under the sticky file header
  // (sp-6 padding + 14px icon) — an alignment constant, not a spacing token.
  { file: "find-in-files.component.ts", value: "180px" },
  { file: "find-in-files.component.ts", value: "26px" },
  // Search-everywhere results list: 120px min-height is a structural viewport
  // constraint for the overlay (keeps the empty state from collapsing).
  { file: "search-everywhere.component.ts", value: "120px" },
  // .dvc-pdot process-tree status dot is a 7×7px rounded square mirroring the
  // conflict-view .sq — a fixed pip visual constant, not a spacing token.
  { file: "dev-panel.component.ts", value: "7px" },
  // Conflict-view fixed-geometry mini-widgets: .sq status square (7×7),
  // .meter progress track (4px, like the settings slider track), .cf-dot
  // (15×15 circle) + .pip (5×5) form a fixed unit that must stay round,
  // .st-dot block-status dot (8×8). All are pips/tracks whose geometry is
  // ported 1:1 from the design reference and must not density-scale.
  { file: "conflict-view.component.ts", value: "7px" },
  { file: "conflict-view.component.ts", value: "4px" },
  { file: "conflict-view.component.ts", value: "15px" },
  { file: "conflict-view.component.ts", value: "5px" },
  { file: "conflict-view.component.ts", value: "8px" },
  // .cf-edit inline-resolution textarea: 70px min-height is a structural
  // constraint (~3 monospace lines) so the editor never collapses.
  { file: "conflict-view.component.ts", value: "70px" },
  // The boot/loading splash is a pre-theme screen with fixed, design-exact metrics
  // (hardcoded colors + px). The "Orrery × Kouji.dev" credit footer's gap + font
  // sizes are bespoke visual constants, not spacing/font tokens.
  { file: "loading.component.ts", value: "8px" },
  { file: "loading.component.ts", value: "13px" },
  { file: "loading.component.ts", value: "12px" },
  // ── Fixed glyph geometry (width/min-width/max-width sweep) ──
  // Added when the linter's scope widened to cover width properties. Each of
  // these is a shape, not a container: accent bars and pips (2-3px), Monaco's
  // own scrollbar rails (3px/7px, !important), checkbox and toggle squares
  // (12-14px, 30px, 34px), badge minimums (16px), gutter and glyph columns
  // (24px, 44px), the collapsed icon rail (54px) and fixed label columns
  // (56px). These must NOT scale: a square that scales on one axis stops being
  // square, and a Monaco rail that scales stops matching Monaco's own layout.
  { file: "agent-row.component.ts", value: "2.5px" },
  { file: "annotate-blame.component.ts", value: "3px" },
  { file: "annotate-blame.component.ts", value: "44px" },
  { file: "commit-graph-panel.component.ts", value: "56px" },
  { file: "compact-rail.component.ts", value: "24px" },
  { file: "compact-rail.component.ts", value: "54px" },
  { file: "conflict-view.component.ts", value: "14px" },
  { file: "dev-panel.component.ts", value: "16px" },
  { file: "dev-panel.component.ts", value: "9px" },
  { file: "file-blame.component.ts", value: "3px" },
  { file: "file-blame.component.ts", value: "44px" },
  { file: "find-in-files.component.ts", value: "34px" },
  { file: "git-tab.component.ts", value: "12px" },
  { file: "monaco-file-editor.component.ts", value: "3px" },
  { file: "monaco-file-editor.component.ts", value: "7px" },
  { file: "question-stepper.component.ts", value: "13px" },
  { file: "review-comments.monaco.ts", value: "24px" },
  { file: "rich-editor.component.ts", value: "56px" },
  { file: "send-review.component.ts", value: "54px" },
  { file: "settings-modal.component.ts", value: "2.5px" },
  { file: "settings-modal.component.ts", value: "30px" },
  { file: "settings-modal.component.ts", value: "34px" },
  { file: "settings-modal.component.ts", value: "9px" },
  { file: "sidebar.component.ts", value: "16px" },
  { file: "status-bar.component.ts", value: "30px" },
  { file: "styles.css", value: "32px" },
  { file: "styles.css", value: "44px" },
  { file: "styles.css", value: "9px" },
  { file: "top-bar.component.ts", value: "3px" },
  { file: "tweaks-panel.component.ts", value: "34px" },
  { file: "whats-new-modal.component.ts", value: "7px" },
];

export function scanText(text, file = "") {
  const out = [];
  for (const m of text.matchAll(RE)) {
    const [, prop, rawVal] = m;
    const val = rawVal.trim();

    // A px literal multiplied by the density scalar IS density-aware — that is
    // the tokenized form, not a violation. Same for a viewport-relative clamp
    // (`calc(100vw - 32px)`): the literal is a gutter, and the value tracks the
    // window rather than being frozen.
    if (/var\(--(density|fs-scale)\)/.test(val)) continue;
    if (/\d+(?:\.\d+)?v[wh]/.test(val)) continue;
    // min()/max()/clamp() already bound the literal against a relative term.
    if (/(?:min|max|clamp)\(/.test(val)) continue;

    const pxs = (val.match(PX) || []).map((s) => s.replace("-", ""));
    let bad = pxs.filter((p) => !ALLOW_PX.has(p));
    if (!bad.length) continue;

    // Allowlist entries exempt the px TOKENS they name, not the declaration.
    //
    // This used to be `val.includes(a.value)`, a substring test on the raw
    // declaration, which had two compounding failure modes:
    //   1. suffix collision — an entry for "4px" also matched 14px, 24px,
    //      34px, 104px, so most of the numeric space was silently allowed;
    //   2. declaration-wide amnesty — one matched token exempted EVERY px in
    //      that declaration, so `padding: 3px 33px` passed on the "3px" entry.
    // Both are now closed: a token is exempt only if an entry names it exactly,
    // and any token left unexempted still reports.
    const exempt = new Set(
      ALLOWLIST.filter((a) => file.includes(a.file)).map((a) => a.value),
    );
    bad = bad.filter((p) => !exempt.has(p));
    if (!bad.length) continue;

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
