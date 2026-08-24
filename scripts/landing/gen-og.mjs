#!/usr/bin/env node
/**
 * Regenerate landing/og.png — the 1200x630 Open Graph card.
 *
 * Rendered headlessly by Playwright's chromium (already a devDependency) from
 * the inline template below, which reuses the landing page's own tokens
 * (--bg / --grad / Space Grotesk / JetBrains Mono) so the card and the site
 * cannot drift apart in palette or type.
 *
 * The mark is the same epicycle (rosette) the hero draws: the geometry
 * constants and epi()/epiPath() below are copied verbatim from
 * landing/home.js (A=22, B=13, K=-4, 260 segments) and flattened to a static
 * <path>, so no runtime JS is involved in the screenshot.
 *
 * Needs network access once, for the Google Fonts faces. Run:
 *   node scripts/landing/gen-og.mjs
 * then commit the PNG — the deploy ships the committed file, never this script.
 */
import { writeFileSync } from "node:fs";
import { dirname, resolve } from "node:path";
import { fileURLToPath } from "node:url";
import { chromium } from "@playwright/test";

const HERE = dirname(fileURLToPath(import.meta.url));
const OUT = resolve(HERE, "../../landing/og.png");
const W = 1200;
const H = 630;

// ---- epicycle geometry — mirrors landing/home.js:9-11 ----
const A = 22, B = 13, K = -4, TAU = Math.PI * 2;
const epi = (t) => [50 + A * Math.cos(t) + B * Math.cos(K * t), 50 + A * Math.sin(t) + B * Math.sin(K * t)];
const epiPath = (n) => {
  let d = "";
  for (let i = 0; i <= n; i++) {
    const p = epi((i / n) * TAU);
    d += (i ? "L" : "M") + p[0].toFixed(2) + " " + p[1].toFixed(2) + " ";
  }
  return d + "Z";
};
const EPI_D = epiPath(260);

const html = `<!DOCTYPE html>
<html lang="en"><head><meta charset="utf-8" />
<link rel="preconnect" href="https://fonts.googleapis.com" />
<link rel="preconnect" href="https://fonts.gstatic.com" crossorigin />
<link href="https://fonts.googleapis.com/css2?family=JetBrains+Mono:wght@400;500&family=Space+Grotesk:wght@400;600;700&display=swap" rel="stylesheet" />
<style>
  :root{
    --bg:#07080d; --ink:#e8ebf2; --ink-2:#aab2c5; --ink-3:#6b7488; --hair:rgba(255,255,255,.07);
    --accent:#a855f7; --accent-2:#22d3ee;
    --mono:'JetBrains Mono',ui-monospace,monospace;
    --disp:'Space Grotesk',var(--mono);
    --grad:linear-gradient(100deg,#ff5d9e,#a855f7,#22d3ee);
  }
  *{box-sizing:border-box;margin:0;padding:0}
  body{width:${W}px;height:${H}px;overflow:hidden;background:var(--bg);color:var(--ink);
    font-family:var(--disp);-webkit-font-smoothing:antialiased;position:relative;
    background-image:linear-gradient(rgba(255,255,255,.02) 1px,transparent 1px),linear-gradient(90deg,rgba(255,255,255,.02) 1px,transparent 1px);
    background-size:38px 38px;}
  body::before{content:'';position:absolute;inset:0;
    background:radial-gradient(46% 80% at 50% -18%,rgba(168,85,247,.22),transparent 66%),radial-gradient(40% 60% at 88% 8%,rgba(34,211,238,.10),transparent 70%);}
  .card{position:relative;height:100%;padding:70px 78px;display:flex;flex-direction:column;justify-content:space-between;}
  .top{display:flex;align-items:center;gap:14px;}
  .wm{font-weight:600;font-size:27px;letter-spacing:-.01em;} .wm .o{color:var(--accent);}
  .eyebrow{margin-left:auto;font-family:var(--mono);font-size:15px;letter-spacing:.2em;text-transform:uppercase;color:var(--ink-3);}
  h1{font-weight:600;font-size:78px;line-height:1.04;letter-spacing:-.024em;}
  .grad{background:var(--grad);-webkit-background-clip:text;background-clip:text;color:transparent;}
  p{margin-top:26px;font-size:29px;line-height:1.42;color:var(--ink-2);max-width:930px;}
  .foot{display:flex;align-items:center;gap:20px;font-family:var(--mono);font-size:19px;color:var(--ink-3);}
  .rule{flex:1;height:1px;background:var(--hair);}
  .host{color:var(--ink-2);}
</style></head>
<body><div class="card">
  <div class="top">
    <svg width="46" height="46" viewBox="0 0 100 100" style="display:block;filter:drop-shadow(0 0 18px rgba(168,85,247,.45))">
      <defs>
        <linearGradient id="g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#ff5d9e"/><stop offset=".5" stop-color="#a855f7"/><stop offset="1" stop-color="#22d3ee"/></linearGradient>
        <radialGradient id="c"><stop offset="0" stop-color="#fff"/><stop offset=".45" stop-color="#a855f7"/><stop offset="1" stop-color="#a855f7" stop-opacity="0"/></radialGradient>
      </defs>
      <circle cx="50" cy="50" r="22" fill="none" stroke="#6b7488" stroke-width=".7" stroke-opacity=".3"/>
      <path d="${EPI_D}" fill="none" stroke="url(#g)" stroke-width="4" stroke-linejoin="round" stroke-linecap="round"/>
      <circle cx="50" cy="50" r="11" fill="url(#c)"/>
    </svg>
    <span class="wm"><span class="o">O</span>rrery</span>
    <span class="eyebrow">Multi-agent git orchestration</span>
  </div>

  <div>
    <h1>Every agent in <span class="grad">orbit</span>.<br>One core to command them.</h1>
    <p>Claude Code, Codex, Cursor and Gemini circle a single console — each on its own git worktree and branch. Dispatch, observe, merge.</p>
  </div>

  <div class="foot">
    <span class="host">orrery.kouji.dev</span>
    <span class="rule"></span>
    <span>Free · Windows &amp; macOS</span>
  </div>
</div></body></html>`;

const browser = await chromium.launch();
const page = await browser.newPage({ viewport: { width: W, height: H }, deviceScaleFactor: 1 });
await page.setContent(html, { waitUntil: "networkidle" });
await page.evaluate(() => document.fonts.ready);
const png = await page.screenshot({ type: "png" });
await browser.close();

writeFileSync(OUT, png);
console.log(`og.png — ${W}x${H}, ${(png.length / 1024).toFixed(1)} KB → ${OUT}`);
