/* global React, EpicycleLogo, MiniMark, Wordmark, OrreryConsole, ToolDot */
// Orrery landing — three hero/layout directions. Exports DirA, DirB, DirC to window.

(function () {
  const e = React.createElement;
  const VER = "v0.2.3";

  // shared bits -------------------------------------------------------------
  const Eyebrow = ({ children, mono }) =>
    e("div", { className: "ol-eyebrow" + (mono ? " mono" : "") },
      e("span", { className: "ol-eyebrow-dot" }), children);

  const WinBtn = ({ primary, children, sub }) =>
    e("a", { className: "ol-btn" + (primary ? " primary" : " ghost"), href: "#" },
      primary ? e("svg", { width: 15, height: 15, viewBox: "0 0 24 24", fill: "currentColor", style: { flex: "none" } },
        e("path", { d: "M3 5.5 10.2 4.5V11H3V5.5ZM10.2 12v6.5L3 17.5V12h7.2ZM11.4 4.3 21 3v8.5h-9.6V4.3ZM21 12.5V21l-9.6-1.3V12.5H21Z" })) : null,
      e("span", null, children),
      sub ? e("span", { className: "ol-btn-sub" }, sub) : null);

  const CTArow = () =>
    e("div", { className: "ol-cta-row" },
      e(WinBtn, { primary: true, children: "Download for Windows", sub: VER }),
      e("a", { className: "ol-btn ghost", href: "#" }, e("span", null, "Watch a 90-second run")),
      e("span", { className: "ol-mac-note" }, "macOS coming soon"));

  const TOOLS = [
    { k: "claude", label: "Claude Code", c: "#d98a5b" },
    { k: "codex", label: "Codex", c: "#10a37f" },
    { k: "cursor", label: "Cursor", c: "#6e9bff" },
    { k: "gemini", label: "Gemini", c: "#8a7cff" },
  ];

  const ToolStrip = ({ label }) =>
    e("div", { className: "ol-toolstrip" },
      e("span", { className: "ol-toolstrip-l" }, label || "Drives"),
      TOOLS.map((t, i) => e("span", { key: i, className: "ol-toolstrip-item" },
        e("span", { className: "ol-toolstrip-dot", style: { background: t.c } }), t.label)));

  const FEATURES = [
    { n: "01", t: "Dispatch a fleet", d: "Spin up many agents at once and assign the right tool and model to each task — Claude Code, Codex, Cursor or Gemini, running in parallel across every repo.", c: "#22d3ee" },
    { n: "02", t: "A worktree per agent", d: "Every agent gets its own git worktree and branch. No collisions, no half-finished edits in your tree — you merge only what passes.", c: "#a855f7" },
    { n: "03", t: "Watch every branch live", d: "One console for the whole fleet: running, blocked, waiting, done — with diffs, commits, and the exact moment an agent needs a decision.", c: "#ff5d9e" },
  ];

  const FeatureCards = () =>
    e("div", { className: "ol-feat-grid" },
      FEATURES.map((f, i) => e("div", { key: i, className: "ol-feat" },
        e("div", { className: "ol-feat-n", style: { color: f.c } }, f.n),
        e("div", { className: "ol-feat-t" }, f.t),
        e("div", { className: "ol-feat-d" }, f.d),
        e("div", { className: "ol-feat-bar", style: { background: f.c } }))));

  const CTAband = ({ align = "center" }) =>
    e("div", { className: "ol-band", style: { textAlign: align, alignItems: align === "center" ? "center" : "flex-start" } },
      e("div", { className: "ol-band-mark" }, e(EpicycleLogo, { size: 60, animated: true })),
      e("h2", { className: "ol-band-h" }, "Stop babysitting one agent."),
      e("p", { className: "ol-band-p" }, "Run the whole fleet from a single console — and merge what's ready."),
      e("div", { style: { marginTop: 26 } }, e(CTArow)));

  const Foot = () =>
    e("div", { className: "ol-foot" },
      e("div", { className: "ol-foot-l" }, e(MiniMark, { size: 18 }), e(Wordmark, { size: 14 })),
      e("div", { className: "ol-foot-links" },
        ["Product", "Docs", "Changelog", "GitHub", "Discord"].map((x, i) => e("a", { key: i, href: "#" }, x))),
      e("div", { className: "ol-foot-r" }, "© 2026 Orrery · Windows 10/11 · " + VER));

  // ========================================================================
  // DIRECTION A — Mission Control (centered, app-screenshot hero)
  // ========================================================================
  function DirA() {
    return e("div", { className: "ol-page da", "data-screen-label": "A · Mission Control" },
      e("div", { className: "da-nav" },
        e("div", { className: "da-nav-l" }, e(MiniMark, { size: 22 }), e(Wordmark, { size: 17 })),
        e("div", { className: "da-nav-c" }, ["Product", "Agents", "Docs", "Changelog"].map((x, i) => e("a", { key: i, href: "#" }, x))),
        e("a", { className: "ol-btn ghost sm", href: "#" }, e("span", null, "Download"))),

      e("section", { className: "da-hero" },
        e(Eyebrow, null, "Multi-agent git orchestration"),
        e("h1", { className: "da-h1" }, "Command a fleet of ", e("span", { className: "grad" }, "coding agents"), "."),
        e("p", { className: "da-sub" }, "Orrery runs Claude Code, Codex, Cursor and Gemini across all your repos — each isolated in its own git worktree. Dispatch the work, watch every branch live, merge what's ready."),
        e(CTArow),
        e("div", { className: "da-stage" },
          e("div", { className: "da-stage-glow" }),
          e("div", { className: "da-app" }, e(OrreryConsole, { scale: 1 })))),

      e("section", { className: "da-tool" }, e(ToolStrip, { label: "Orchestrates" })),

      e("section", { className: "ol-section" },
        e("div", { className: "ol-kicker" }, "How it works"),
        e(FeatureCards)),

      e("section", { className: "ol-section center" }, e(CTAband, { align: "center" })),
      e(Foot));
  }

  // ========================================================================
  // DIRECTION B — Orbital (epicycle centerpiece)
  // ========================================================================
  function DirB() {
    // tool nodes placed around the orbit
    const nodes = [
      { ...TOOLS[0], x: 50, y: -2 }, { ...TOOLS[1], x: 102, y: 50 },
      { ...TOOLS[2], x: 50, y: 102 }, { ...TOOLS[3], x: -2, y: 50 },
    ];
    return e("div", { className: "ol-page db", "data-screen-label": "B · Orbital" },
      e("div", { className: "da-nav" },
        e("div", { className: "da-nav-l" }, e(MiniMark, { size: 22 }), e(Wordmark, { size: 17 })),
        e("div", { className: "da-nav-c" }, ["Product", "Agents", "Docs", "Changelog"].map((x, i) => e("a", { key: i, href: "#" }, x))),
        e("a", { className: "ol-btn ghost sm", href: "#" }, e("span", null, "Download"))),

      e("section", { className: "db-hero" },
        e("div", { className: "db-orbit" },
          e("div", { className: "db-ring r1" }),
          e("div", { className: "db-ring r2" }),
          e("div", { className: "db-ring r3" }),
          e("div", { className: "db-spin" },
            nodes.map((n, i) => e("span", { key: i, className: "db-node", style: { left: n.x + "%", top: n.y + "%", "--nc": n.c } },
              e("span", { className: "db-node-dot", style: { background: n.c } }), n.label))),
          e("div", { className: "db-core" }, e(EpicycleLogo, { size: 232, animated: true, strokeW: 2.6 }))),
        e(Eyebrow, null, "Multi-agent git orchestration"),
        e("h1", { className: "db-h1" }, "Every agent in ", e("span", { className: "grad" }, "orbit"), ".", e("br"), "One core to command them."),
        e("p", { className: "db-sub" }, "Claude Code, Codex, Cursor and Gemini circle a single console — each on its own branch, all under your hand. Dispatch, observe, merge."),
        e(CTArow)),

      e("section", { className: "db-stage-sec" },
        e("div", { className: "ol-kicker center" }, "The console"),
        e("div", { className: "db-stage" },
          e("div", { className: "db-stage-glow" }),
          e("div", { className: "db-app" }, e(OrreryConsole, { scale: 0.86 })))),

      e("section", { className: "ol-section" }, e(FeatureCards)),
      e("section", { className: "ol-section center" }, e(CTAband, { align: "center" })),
      e(Foot));
  }

  // ========================================================================
  // DIRECTION C — Split / terminal (asymmetric, left-aligned)
  // ========================================================================
  function DirC() {
    const stats = [["9", "agents live"], ["3", "repos"], ["4", "running now"], ["1", "ready to merge"]];
    return e("div", { className: "ol-page dc", "data-screen-label": "C · Split" },
      e("div", { className: "da-nav dc-nav" },
        e("div", { className: "da-nav-l" }, e(MiniMark, { size: 22 }), e(Wordmark, { size: 17 })),
        e("div", { className: "da-nav-c" }, ["Product", "Agents", "Docs", "Changelog"].map((x, i) => e("a", { key: i, href: "#" }, x))),
        e("a", { className: "ol-btn ghost sm", href: "#" }, e("span", null, "Download"))),

      e("section", { className: "dc-hero" },
        e("div", { className: "dc-hero-l" },
          e(Eyebrow, { mono: true }, "MULTI-AGENT GIT ORCHESTRATION"),
          e("h1", { className: "dc-h1" }, "Your agents,", e("br"), "on ", e("span", { className: "grad" }, "every branch"), ".", e("br"), "In one window."),
          e("p", { className: "dc-sub" }, "Orrery dispatches Claude Code, Codex, Cursor and Gemini into isolated git worktrees across your repos — then streams every diff, status and decision back to a single console."),
          e("div", { className: "ol-cta-row left" },
            e(WinBtn, { primary: true, children: "Download for Windows", sub: VER }),
            e("a", { className: "ol-btn ghost", href: "#" }, e("span", null, "Read the docs"))),
          e("div", { className: "dc-stats" },
            stats.map((s, i) => e("div", { key: i, className: "dc-stat" },
              e("div", { className: "dc-stat-n" }, s[0]),
              e("div", { className: "dc-stat-l" }, s[1])))),
          e("div", { className: "dc-mac" }, "macOS coming soon · Windows 10/11")),
        e("div", { className: "dc-hero-r" },
          e("div", { className: "dc-app-glow" }),
          e("div", { className: "dc-app" }, e(OrreryConsole, { scale: 0.92 })))),

      e("section", { className: "dc-toolsec" }, e(ToolStrip, { label: "Orchestrates" })),

      e("section", { className: "ol-section" },
        e("div", { className: "ol-kicker" }, "Built for parallel work"),
        e(FeatureCards)),

      e("section", { className: "ol-section" }, e(CTAband, { align: "left" })),
      e(Foot));
  }

  Object.assign(window, { DirA, DirB, DirC });
})();
