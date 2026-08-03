/* global React */
// Orrery landing — shared parts: EpicycleLogo, Wordmark, Console product mock, atoms.
// Exports to window: EpicycleLogo, Wordmark, OrreryConsole, MiniMark, ToolDot, StatusChip, ICONS.

(function () {
  const { useRef, useEffect, useState } = React;

  // ---- epicycle geometry (shared with the app splash) ----
  const A = 22, B = 13, K = -4, TAU = Math.PI * 2;
  const epi = (t) => [50 + A * Math.cos(t) + B * Math.cos(K * t), 50 + A * Math.sin(t) + B * Math.sin(K * t)];
  function buildPath(N) {
    let d = "";
    for (let i = 0; i <= N; i++) { const t = (i / N) * TAU, p = epi(t); d += (i ? "L" : "M") + p[0].toFixed(2) + " " + p[1].toFixed(2) + " "; }
    return d + "Z";
  }
  const EPI_D = buildPath(260);
  let uid = 0;

  // ---- the rosette mark; animated=true draws + orbits on loop ----
  function EpicycleLogo({ size = 64, animated = false, glow = true, strokeW = 3, style = {} }) {
    const ref = useRef(null);
    const id = useRef(++uid).current;
    useEffect(() => {
      if (!animated) return;
      const svg = ref.current; if (!svg) return;
      const path = svg.querySelector(".p"), epc = svg.querySelector(".e"),
        arm = svg.querySelector(".a"), pen = svg.querySelector(".n");
      const reduce = window.matchMedia && matchMedia("(prefers-reduced-motion:reduce)").matches;
      const L = path.getTotalLength();
      const ease = (x) => 1 - Math.pow(1 - x, 3);
      let raf, t1;
      function still() {
        path.style.strokeDasharray = "none"; path.style.strokeDashoffset = 0;
        epc.style.opacity = 0; arm.style.opacity = 0;
        const s = epi(0); pen.setAttribute("cx", s[0]); pen.setAttribute("cy", s[1]);
      }
      function loop() {
        const DUR = 2400, HOLD = 1200, start = performance.now();
        path.style.strokeDasharray = L; path.style.strokeDashoffset = L;
        epc.style.transition = arm.style.transition = "none"; epc.style.opacity = 1; arm.style.opacity = 1;
        function frame(now) {
          const p = Math.min(1, (now - start) / DUR), e = ease(p);
          path.style.strokeDashoffset = L * (1 - e);
          const t = e * TAU, def = [50 + A * Math.cos(t), 50 + A * Math.sin(t)], pp = epi(t);
          epc.setAttribute("cx", def[0]); epc.setAttribute("cy", def[1]);
          pen.setAttribute("cx", pp[0]); pen.setAttribute("cy", pp[1]);
          arm.setAttribute("x1", def[0]); arm.setAttribute("y1", def[1]); arm.setAttribute("x2", pp[0]); arm.setAttribute("y2", pp[1]);
          if (p < 1) raf = requestAnimationFrame(frame);
          else { epc.style.transition = arm.style.transition = "opacity .35s"; epc.style.opacity = 0; arm.style.opacity = 0; const s = epi(0); pen.setAttribute("cx", s[0]); pen.setAttribute("cy", s[1]); t1 = setTimeout(loop, HOLD); }
        }
        raf = requestAnimationFrame(frame);
      }
      if (reduce) still(); else loop();
      return () => { cancelAnimationFrame(raf); clearTimeout(t1); };
    }, [animated]);

    return (
      React.createElement("svg", {
        ref, width: size, height: size, viewBox: "0 0 100 100", "aria-label": "Orrery",
        style: { filter: glow ? `drop-shadow(0 0 ${size * 0.22}px rgba(168,85,247,.42))` : "none", display: "block", ...style }
      },
        React.createElement("defs", null,
          React.createElement("linearGradient", { id: `g${id}`, x1: 0, y1: 0, x2: 1, y2: 1 },
            React.createElement("stop", { offset: 0, stopColor: "#ff5d9e" }),
            React.createElement("stop", { offset: .5, stopColor: "#a855f7" }),
            React.createElement("stop", { offset: 1, stopColor: "#22d3ee" })),
          React.createElement("radialGradient", { id: `c${id}` },
            React.createElement("stop", { offset: 0, stopColor: "#fff" }),
            React.createElement("stop", { offset: .42, stopColor: "#a855f7" }),
            React.createElement("stop", { offset: 1, stopColor: "#a855f7", stopOpacity: 0 }))),
        React.createElement("circle", { cx: 50, cy: 50, r: 22, fill: "none", stroke: "#6b7488", strokeWidth: .7, strokeOpacity: .3 }),
        React.createElement("path", { className: "p", d: EPI_D, fill: "none", stroke: `url(#g${id})`, strokeWidth: strokeW, strokeLinejoin: "round", strokeLinecap: "round" }),
        React.createElement("circle", { className: "e", r: 13, fill: "none", stroke: "#6b7488", strokeWidth: .7, strokeOpacity: .4, style: { opacity: 0 } }),
        React.createElement("line", { className: "a", stroke: "#22d3ee", strokeWidth: 1.2, strokeOpacity: .6, style: { opacity: 0 } }),
        React.createElement("circle", { cx: 50, cy: 50, r: 8.5, fill: `url(#c${id})`, style: { transformBox: "fill-box", transformOrigin: "50% 50%", animation: "ol-pulse 2.4s ease-in-out infinite" } }),
        React.createElement("circle", { className: "n", r: 4.2, fill: "#22d3ee", cx: epi(0)[0], cy: epi(0)[1] })
      )
    );
  }

  // small flat mark for chips/footers
  function MiniMark({ size = 22, sw = 6 }) {
    const id = useRef(++uid).current;
    return React.createElement("svg", { width: size, height: size, viewBox: "0 0 100 100" },
      React.createElement("defs", null,
        React.createElement("linearGradient", { id: `m${id}`, x1: 0, y1: 0, x2: 1, y2: 1 },
          React.createElement("stop", { offset: 0, stopColor: "#ff5d9e" }),
          React.createElement("stop", { offset: .5, stopColor: "#a855f7" }),
          React.createElement("stop", { offset: 1, stopColor: "#22d3ee" })),
        React.createElement("radialGradient", { id: `mc${id}` },
          React.createElement("stop", { offset: 0, stopColor: "#fff" }),
          React.createElement("stop", { offset: .45, stopColor: "#a855f7" }),
          React.createElement("stop", { offset: 1, stopColor: "#a855f7", stopOpacity: 0 }))),
      React.createElement("path", { d: EPI_D, fill: "none", stroke: `url(#m${id})`, strokeWidth: sw, strokeLinejoin: "round" }),
      React.createElement("circle", { cx: 50, cy: 50, r: 13, fill: `url(#mc${id})` }));
  }

  function Wordmark({ size = 18, color = "#e8ebf2" }) {
    return React.createElement("span", { className: "ol-wm", style: { fontSize: size, color } },
      React.createElement("span", { className: "o" }, "O"), "rrery");
  }

  // ---- tool + status atoms ----
  const TOOLS = {
    claude: { name: "claude", c: "#d98a5b" }, codex: { name: "codex", c: "#10a37f" },
    cursor: { name: "cursor", c: "#6e9bff" }, gemini: { name: "gemini", c: "#8a7cff" },
  };
  const ST = {
    running: { c: "#22d3ee", l: "running" }, blocked: { c: "#ff5d7a", l: "blocked" },
    waiting: { c: "#c084fc", l: "waiting" }, done: { c: "#34e0a1", l: "done" },
    queued: { c: "#6b7488", l: "queued" },
  };
  function ToolDot({ tool }) {
    const t = TOOLS[tool];
    return React.createElement("span", { className: "ol-tool" },
      React.createElement("span", { className: "ol-tool-dot", style: { background: t.c } }), t.name);
  }
  function StatusChip({ status }) {
    const s = ST[status];
    return React.createElement("span", { className: "ol-chip", style: { color: s.c, "--sc": s.c } },
      React.createElement("span", { className: `ol-chip-dot ${status === "running" || status === "waiting" ? "live" : ""}` }), s.l);
  }

  // ---- the console product mock (the "actual app UI") ----
  const fmt = (s) => Math.floor(s / 60) + "m " + String(Math.floor(s % 60)).padStart(2, "0") + "s";
  const AGENTS = [
    { tool: "claude", model: "sonnet-4.6", name: "stripe-retry", task: "Add exponential-backoff retry to the Stripe webhook handler", status: "running", branch: "agent/stripe-retry", add: 188, del: 24, pct: 68, secs: 412, rate: 0.18, t: "6m 52s" },
    { tool: "claude", model: "opus-4.6", name: "jwt-refresh", task: "Migrate auth to short-lived JWT + rotating refresh tokens", status: "blocked", branch: "agent/jwt-refresh", add: 142, del: 96, pct: 52, secs: 1284, t: "21m 24s", note: "Decision: store refresh tokens in Redis or Postgres?" },
    { tool: "codex", model: "gpt-5.1-codex · high", name: "reconcile-refactor", task: "Refactor nightly reconciliation into idempotent steps", status: "running", branch: "agent/reconcile-refactor", add: 64, del: 31, pct: 34, secs: 196, rate: 0.5, t: "3m 16s" },
    { tool: "gemini", model: "2.5-flash", name: "node22", task: "Upgrade runtime to Node 22 and bump all dependencies", status: "waiting", branch: "agent/node22", add: 28, del: 28, pct: 8, secs: 38, t: "0m 38s", note: "Waiting on CI — 3 checks queued" },
    { tool: "claude", model: "sonnet-4.6", name: "settings-redesign", task: "Rebuild account settings with the new design tokens", status: "running", branch: "agent/settings-redesign", add: 96, del: 40, pct: 46, secs: 308, rate: 0.32, t: "5m 08s" },
    { tool: "claude", model: "sonnet-4.6", name: "checkout-tests", task: "Integration tests for the checkout → capture flow", status: "done", branch: "agent/checkout-tests", add: 236, del: 0, pct: 100, secs: 642, t: "10m 42s", note: "Ready to merge → main" },
  ];
  const PROJECTS = [
    { name: "payments-service", org: "northwind", n: 6, dots: ["running", "blocked", "running", "waiting", "done", "queued"] },
    { name: "web-dashboard", org: "northwind", n: 2, dots: ["running", "waiting"] },
    { name: "infra-terraform", org: "northwind", n: 1, dots: ["done"] },
  ];

  function AgentCard({ a }) {
    const s = ST[a.status];
    return React.createElement("div", { className: `ol-card ${a.status === "blocked" ? "blk" : ""}` },
      React.createElement("div", { className: "ol-card-top" },
        React.createElement(ToolDot, { tool: a.tool }),
        React.createElement("span", { className: "ol-agent-name" }, a.name),
        React.createElement(StatusChip, { status: a.status })),
      React.createElement("div", { className: "ol-task" }, a.task),
      a.note ? React.createElement("div", { className: "ol-note", style: { color: s.c, borderColor: s.c + "44", background: s.c + "12" } }, a.note) : null,
      React.createElement("div", { className: "ol-meta" },
        React.createElement("span", { className: "ol-branch" }, "⎇ " + a.branch),
        React.createElement("span", { className: "ol-diff" },
          React.createElement("span", { className: "add" }, "+" + a.add),
          React.createElement("span", { className: "del" }, "−" + a.del)),
        React.createElement("span", { className: "ol-elapsed" }, a.t)),
      React.createElement("div", { className: "ol-prog" },
        React.createElement("i", { style: { width: a.pct + "%", background: a.status === "done" ? s.c : `linear-gradient(90deg, ${s.c}, ${s.c}cc)` } })));
  }

  function OrreryConsole({ scale = 1, w = 1180, h = 724, live = false }) {
    const [tick, setTick] = React.useState(0);
    React.useEffect(() => {
      if (!live) return;
      const reduce = window.matchMedia && matchMedia("(prefers-reduced-motion:reduce)").matches;
      if (reduce) return;
      const id = setInterval(() => setTick((t) => t + 1), 1000);
      return () => clearInterval(id);
    }, [live]);
    const agents = AGENTS.map((a) => {
      if (!live || (a.status !== "running" && a.status !== "waiting")) return a;
      const secs = (a.secs || 0) + tick;
      const pct = a.status === "running" ? Math.min(96, a.pct + tick * (a.rate || 0.2)) : a.pct;
      return Object.assign({}, a, { secs, t: fmt(secs), pct: Math.round(pct) });
    });
    return React.createElement("div", { className: "ol-app-wrap", style: { width: w * scale, height: h * scale } },
      React.createElement("div", { className: "ol-app", style: { width: w, height: h, transform: `scale(${scale})`, transformOrigin: "top left" } },
        // top chrome
        React.createElement("div", { className: "ol-top" },
          React.createElement("div", { className: "ol-top-l" },
            React.createElement(MiniMark, { size: 20 }),
            React.createElement(Wordmark, { size: 14 }),
            React.createElement("span", { className: "ol-crumb" }, "northwind"),
            React.createElement("span", { className: "ol-crumb-sep" }, "/"),
            React.createElement("span", { className: "ol-crumb on" }, "all projects")),
          React.createElement("div", { className: "ol-omni" }, "⌘K  run an agent, jump to a repo…"),
          React.createElement("div", { className: "ol-top-r" },
            React.createElement("span", { className: "ol-legend" },
              React.createElement("span", { className: "d", style: { background: "#22d3ee" } }), "4 running"),
            React.createElement("span", { className: "ol-legend" },
              React.createElement("span", { className: "d", style: { background: "#ff5d7a" } }), "1 blocked"),
            React.createElement("span", { className: "ol-av" }, "KJ"))),
        // body
        React.createElement("div", { className: "ol-body" },
          React.createElement("aside", { className: "ol-side" },
            React.createElement("div", { className: "ol-side-h" }, "Projects"),
            PROJECTS.map((p, i) => React.createElement("div", { key: i, className: "ol-proj" + (i === 0 ? " on" : "") },
              React.createElement("div", { className: "ol-proj-row" },
                React.createElement("span", { className: "ol-proj-name" }, p.name),
                React.createElement("span", { className: "ol-proj-n" }, p.n)),
              React.createElement("div", { className: "ol-proj-dots" },
                p.dots.map((d, j) => React.createElement("span", { key: j, style: { background: ST[d].c } }))))),
            React.createElement("button", { className: "ol-new" }, "+  New agent"),
            React.createElement("div", { className: "ol-side-foot" },
              React.createElement(MiniMark, { size: 14 }),
              "9 agents · 3 repos · 1 host")),
          React.createElement("main", { className: "ol-main" },
            React.createElement("div", { className: "ol-main-h" },
              React.createElement("span", { className: "ol-main-title" }, "Active agents"),
              React.createElement("div", { className: "ol-filters" },
                ["All", "Running", "Blocked", "Waiting", "Done"].map((f, i) =>
                  React.createElement("span", { key: i, className: "ol-filter" + (i === 0 ? " on" : "") }, f)))),
            React.createElement("div", { className: "ol-grid" },
              agents.map((a, i) => React.createElement(AgentCard, { key: i, a }))))),
        // status bar
        React.createElement("div", { className: "ol-statusbar" },
          React.createElement("span", null, React.createElement("span", { className: "sb-live" }), "4 running"),
          React.createElement("span", { className: "sep" }, "·"),
          React.createElement("span", { style: { color: "#ff5d7a" } }, "1 blocked"),
          React.createElement("span", { className: "sep" }, "·"),
          React.createElement("span", { style: { color: "#c084fc" } }, "1 waiting"),
          React.createElement("span", { className: "sb-spacer" }),
          React.createElement("span", null, "main @ a3f91c2"),
          React.createElement("span", { className: "sep" }, "·"),
          React.createElement("span", null, "Orrery v0.2.3"))));
  }

  Object.assign(window, { EpicycleLogo, MiniMark, Wordmark, ToolDot, StatusChip, OrreryConsole });
})();
