/* global React, EpicycleLogo, MiniMark, Wordmark, OrreryConsole */
// Orrery home (Direction B · Orbital) — responsive, interactive landing page.
// Mounts to #root. Depends on orrery-landing-parts.jsx.

(function () {
  const { useState, useEffect, useRef } = React;
  const e = React.createElement;
  const VER = "v0.2.3";
  const reduceMotion = () => window.matchMedia && matchMedia("(prefers-reduced-motion:reduce)").matches;

  const WinGlyph = (p) => e("svg", Object.assign({ viewBox: "0 0 24 24", fill: "currentColor" }, p),
    e("path", { d: "M3 5.5 10.2 4.5V11H3V5.5ZM10.2 12v6.5L3 17.5V12h7.2ZM11.4 4.3 21 3v8.5h-9.6V4.3ZM21 12.5V21l-9.6-1.3V12.5H21Z" }));

  // ---- React-owned scroll reveal (so re-renders never strip the class) ----
  function Reveal(props) {
    const { tag = "div", className = "", delay = 0, style, children } = props;
    const rest = Object.assign({}, props);
    ["tag", "className", "delay", "style", "children"].forEach((k) => delete rest[k]);
    const ref = useRef(null);
    const [shown, setShown] = useState(false);
    useEffect(() => {
      const el = ref.current; if (!el) return;
      if (reduceMotion() || !("IntersectionObserver" in window)) { setShown(true); return; }
      if (el.getBoundingClientRect().top < (window.innerHeight || 800) * 0.96) { setShown(true); return; }
      const io = new IntersectionObserver((ents, o) => {
        ents.forEach((en) => { if (en.isIntersecting) { setShown(true); o.disconnect(); } });
      }, { threshold: 0.12, rootMargin: "0px 0px -6% 0px" });
      io.observe(el);
      return () => io.disconnect();
    }, []);
    return e(tag, Object.assign({
      ref, className: ("reveal " + className + (shown ? " in" : "")).trim(),
      style: Object.assign({ transitionDelay: delay + "ms" }, style),
    }, rest), children);
  }

  const TOOLS = [
    { k: "claude", label: "Claude Code", c: "#d98a5b" },
    { k: "codex", label: "Codex", c: "#10a37f" },
    { k: "cursor", label: "Cursor", c: "#6e9bff" },
    { k: "gemini", label: "Gemini", c: "#8a7cff" },
  ];
  const FEATURES = [
    { n: "01", t: "Dispatch a fleet", d: "Spin up many agents at once and assign the right tool and model to each task — Claude Code, Codex, Cursor or Gemini, running in parallel across every repo.", c: "#22d3ee" },
    { n: "02", t: "A worktree per agent", d: "Every agent gets its own git worktree and branch. No collisions, no half-finished edits in your tree — you merge only what passes.", c: "#a855f7" },
    { n: "03", t: "Watch every branch live", d: "One console for the whole fleet: running, blocked, waiting, done — with diffs, commits, and the exact moment an agent needs a decision.", c: "#ff5d9e" },
  ];

  // ---- responsive app console (self-animating) ----
  function ResponsiveConsole() {
    const ref = useRef(null);
    const [scale, setScale] = useState(0.8);
    useEffect(() => {
      const el = ref.current; if (!el) return;
      const host = el.closest(".console-scroll") || el.parentElement;
      const fit = () => setScale(Math.min(1, host.clientWidth / 1340));
      fit();
      const ro = new ResizeObserver(fit); ro.observe(host);
      window.addEventListener("resize", fit);
      return () => { ro.disconnect(); window.removeEventListener("resize", fit); };
    }, []);
    return e("div", { className: "rc-frame", ref, style: { width: 1340 * scale, height: 812 * scale } },
      e("div", { className: "rc-inner", style: { transform: `scale(${scale})` } },
        e(window.OrreryAppConsole)));
  }

  // ---- download flow ----
  function DownloadToast({ dl, onClose }) {
    if (!dl) return null;
    return e("div", { className: "dl-toast", role: "status" },
      e("div", { className: "dl-ico" }, e(WinGlyph, { width: 18, height: 18 })),
      e("div", { className: "dl-body" },
        e("div", { className: "dl-row" },
          e("span", { className: "dl-name" }, "orrery-setup-0.2.3.exe"),
          e("span", { className: "dl-pct" }, dl.done ? "Ready" : Math.round(dl.progress) + "%")),
        e("div", { className: "dl-bar" }, e("i", { style: { width: (dl.done ? 100 : dl.progress) + "%" } })),
        e("div", { className: "dl-meta" }, dl.done
          ? "SHA-256 a3f91c2…7e0  ·  64.2 MB  ·  Windows 10/11 (x64)"
          : "Downloading from cdn.orrery.dev  ·  64.2 MB")),
      e("button", { className: "dl-x", onClick: onClose, "aria-label": "Dismiss" }, "×"));
  }

  const DlButton = ({ onDownload, big, sub }) =>
    e("a", { className: "ol-btn primary" + (big ? "" : " sm"), href: "#", onClick: (ev) => { ev.preventDefault(); onDownload(); } },
      e(WinGlyph, { width: big ? 15 : 13, height: big ? 15 : 13, style: { flex: "none" } }),
      e("span", null, big ? "Download for Windows" : "Download"),
      e("span", { className: "ol-btn-sub" }, sub || VER));

  // ---- nav ----
  function Nav({ scrolled, onDownload, menuOpen, setMenuOpen }) {
    const links = [["How it works", "#how"], ["Changelog", "changelog.html"]];
    const go = (ev, href) => {
      setMenuOpen(false);
      if (href.startsWith("#") && href.length > 1) {
        const t = document.querySelector(href);
        if (t) { ev.preventDefault(); t.scrollIntoView({ behavior: reduceMotion() ? "auto" : "smooth", block: "start" }); }
      }
    };
    return e("header", { className: "nav" + (scrolled ? " scrolled" : "") },
      e("div", { className: "nav-in" },
        e("a", { className: "nav-brand", href: "#top", onClick: (ev) => go(ev, "#top") },
          e(MiniMark, { size: 24 }), e(Wordmark, { size: 18 })),
        e("nav", { className: "nav-links" },
          links.map((l, i) => e("a", { key: i, href: l[1], onClick: (ev) => go(ev, l[1]) }, l[0]))),
        e("div", { className: "nav-r" },
          e(DlButton, { onDownload }),
          e("button", { className: "nav-burger" + (menuOpen ? " open" : ""), "aria-label": "Menu", onClick: () => setMenuOpen((o) => !o) },
            e("span", null), e("span", null), e("span", null))),
        menuOpen ? e("div", { className: "nav-menu" },
          links.map((l, i) => e("a", { key: i, href: l[1], onClick: (ev) => go(ev, l[1]) }, l[0]))) : null));
  }

  // ---- hero orbit ----
  function Hero({ onDownload }) {
    const nodes = [
      { ...TOOLS[0], x: 50, y: -2 }, { ...TOOLS[1], x: 102, y: 50 },
      { ...TOOLS[2], x: 50, y: 102 }, { ...TOOLS[3], x: -2, y: 50 },
    ];
    const watch = (ev) => { ev.preventDefault(); const t = document.querySelector("#console"); if (t) t.scrollIntoView({ behavior: reduceMotion() ? "auto" : "smooth" }); };
    return e("section", { className: "hero", id: "top" },
      e(Reveal, { className: "orbit" },
        e("div", { className: "ring r1" }), e("div", { className: "ring r2" }), e("div", { className: "ring r3" }),
        e("div", { className: "spin" },
          nodes.map((n, i) => e("span", { key: i, className: "node", style: { left: n.x + "%", top: n.y + "%" } },
            e("span", { className: "node-dot", style: { background: n.c } }), n.label))),
        e("div", { className: "core" }, e(EpicycleLogo, { size: 200, animated: true, strokeW: 2.6 }))),
      e(Reveal, { tag: "div", className: "eyebrow", delay: 60 },
        e("span", { className: "eyebrow-dot" }), "Multi-agent git orchestration"),
      e(Reveal, { tag: "h1", className: "hero-h1", delay: 110 },
        "Every agent in ", e("span", { className: "grad" }, "orbit"), ".", e("br"), "One core to command them."),
      e(Reveal, { tag: "p", className: "hero-sub", delay: 160 },
        "Claude Code, Codex, Cursor and Gemini circle a single console — each on its own branch, all under your hand. Dispatch, observe, merge."),
      e(Reveal, { className: "hero-cta", delay: 210 },
        e("a", { className: "ol-btn primary", href: "#", onClick: (ev) => { ev.preventDefault(); onDownload(); } },
          e(WinGlyph, { width: 15, height: 15, style: { flex: "none" } }),
          e("span", null, "Download for Windows"), e("span", { className: "ol-btn-sub" }, VER)),
        e("a", { className: "ol-btn ghost", href: "#console", onClick: watch }, e("span", null, "Watch a 90-second run")),
        e("span", { className: "mac-note" }, "macOS coming soon")));
  }

  function ToolStrip() {
    return e(Reveal, { tag: "section", className: "toolstrip-sec" },
      e("div", { className: "toolstrip" },
        e("span", { className: "toolstrip-l" }, "Orchestrates"),
        TOOLS.map((t, i) => e("span", { key: i, className: "toolstrip-item" },
          e("span", { className: "toolstrip-dot", style: { background: t.c } }), t.label))));
  }

  function ConsoleSection() {
    return e("section", { className: "console-sec", id: "console" },
      e(Reveal, { tag: "div", className: "kicker center" }, "One console for the fleet"),
      e(Reveal, { tag: "h2", className: "sec-h2 center", delay: 60 }, "Every branch, in a single pane."),
      e(Reveal, { className: "console-scroll", delay: 120 },
        e("div", { className: "console-stage" },
          e("div", { className: "console-glow" }),
          e(ResponsiveConsole))));
  }

  function Features() {
    return e("section", { className: "features-sec", id: "how" },
      e(Reveal, { tag: "div", className: "kicker" }, "How it works"),
      e("div", { className: "feat-grid" },
        FEATURES.map((f, i) => e(Reveal, { key: i, className: "feat", delay: i * 90 },
          e("div", { className: "feat-bar", style: { background: f.c } }),
          e("div", { className: "feat-n", style: { color: f.c } }, f.n),
          e("div", { className: "feat-t" }, f.t),
          e("div", { className: "feat-d" }, f.d)))));
  }

  function Band({ onDownload }) {
    return e("section", { className: "band-sec" },
      e(Reveal, { className: "band" },
        e("div", { className: "band-mark" }, e(EpicycleLogo, { size: 64, animated: true })),
        e("h2", { className: "band-h" }, "Stop babysitting one agent."),
        e("p", { className: "band-p" }, "Run the whole fleet from a single console — and merge what's ready."),
        e("div", { className: "hero-cta", style: { marginTop: 28 } },
          e("a", { className: "ol-btn primary", href: "#", onClick: (ev) => { ev.preventDefault(); onDownload(); } },
            e(WinGlyph, { width: 15, height: 15, style: { flex: "none" } }),
            e("span", null, "Download for Windows"), e("span", { className: "ol-btn-sub" }, VER)),
          e("span", { className: "mac-note" }, "Free during beta · Windows 10/11"))));
  }

  function Foot() {
    return e("footer", { className: "foot" },
      e("div", { className: "foot-l" },
        e(MiniMark, { size: 18 }), e(Wordmark, { size: 14 }),
        e("span", { className: "foot-x" }, "×"),
        e("a", { href: "https://kouji.dev", target: "_blank", rel: "noreferrer", className: "foot-by-link grad" }, "Kouji.dev")),
      e("div", { className: "foot-links" },
        ["Product", "Docs", "Changelog", "GitHub", "Discord"].map((x, i) => e("a", { key: i, href: "#" }, x))),
      e("div", { className: "foot-r" }, "© 2026 Orrery · " + VER));
  }

  // ---- app ----
  function App() {
    const [scrolled, setScrolled] = useState(false);
    const [menuOpen, setMenuOpen] = useState(false);
    const [dl, setDl] = useState(null);
    const dlRef = useRef(null);

    const startDownload = () => {
      if (dl && !dl.done) return;
      if (dlRef.current) clearInterval(dlRef.current);
      setDl({ progress: 0, done: false });
      let p = 0;
      dlRef.current = setInterval(() => {
        p += Math.random() * 15 + 7;
        if (p >= 100) {
          clearInterval(dlRef.current); dlRef.current = null;
          setDl({ progress: 100, done: true });
          setTimeout(() => setDl((d) => (d && d.done ? null : d)), 6500);
        } else setDl({ progress: p, done: false });
      }, 230);
    };

    useEffect(() => {
      const onScroll = () => setScrolled(window.scrollY > 24);
      onScroll();
      window.addEventListener("scroll", onScroll, { passive: true });
      return () => window.removeEventListener("scroll", onScroll);
    }, []);

    return e(React.Fragment, null,
      e(Nav, { scrolled, onDownload: startDownload, menuOpen, setMenuOpen }),
      e("main", null,
        e(Hero, { onDownload: startDownload }),
        e(ToolStrip),
        e(ConsoleSection),
        e(Features),
        e(Band, { onDownload: startDownload })),
      e(Foot),
      e(DownloadToast, { dl, onClose: () => { if (dlRef.current) clearInterval(dlRef.current); dlRef.current = null; setDl(null); } }));
  }

  window.OrreryHomeApp = App;
})();
