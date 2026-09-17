/* global React */
// Orrery — faithful, animated replica of the real app's "Orchestrator" view.
// Self-contained: data + helpers + chrome + overview grid. Exports OrreryAppConsole.
// Visual tokens & .dot/.btn/.surface/.chip/.activity classes come from the host page.

(function () {
  const { useState, useEffect, useRef } = React;
  const e = React.createElement;
  const ORG = "northwind";

  // ---------- icons (subset of the real app's set) ----------
  const ICONS = {
    branch: "M6 4v9M6 13a3 3 0 003 3h3a3 3 0 003-3V9M6 4a2 2 0 100-.01M15 9a2 2 0 100-.01M9 19a2 2 0 100-.01",
    terminal: "M5 6l5 4-5 4M12 16h7", chat: "M4 5h16v10H9l-4 4V5z",
    commit: "M4 12h5m6 0h5M12 9a3 3 0 100 6 3 3 0 000-6z", play: "M7 5l11 7-11 7V5z",
    pause: "M8 5v14M16 5v14", x: "M6 6l12 12M18 6L6 18", chevron: "M9 6l6 6-6 6", chevronD: "M6 9l6 6 6-6",
    folder: "M4 7a2 2 0 012-2h4l2 2h6a2 2 0 012 2v8a2 2 0 01-2 2H6a2 2 0 01-2-2V7z",
    file: "M7 3h7l5 5v13H7V3z M14 3v5h5", clock: "M12 7v5l4 2M12 3a9 9 0 100 18 9 9 0 000-18z",
    bolt: "M13 3L5 14h6l-1 7 8-11h-6l1-7z", search: "M11 4a7 7 0 105 12l4 4M11 4a7 7 0 010 14",
    sun: "M12 4V2m0 20v-2m8-8h2M2 12h2m13.7-5.7l1.4-1.4M4.9 19.1l1.4-1.4m0-11.4L4.9 4.9m14.2 14.2l-1.4-1.4M12 8a4 4 0 100 8 4 4 0 000-8z",
    merge: "M7 4v8a4 4 0 004 4h6M7 4a2 2 0 100-.01M17 16a2 2 0 100 .01M7 12a2 2 0 100 .01",
    layers: "M12 3l9 5-9 5-9-5 9-5zM3 13l9 5 9-5", grid: "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z",
    columns: "M4 4h4v16H4zM10 4h4v16h-4zM16 4h4v16h-4z", timeline: "M4 7h16M4 12h10M4 17h13M19 10v4",
    graph: "M6 6a2 2 0 100-.01M18 6a2 2 0 100-.01M12 18a2 2 0 100-.01M7.5 7.5l3 8M16.5 7.5l-3 8",
    flag: "M5 21V4m0 0h11l-2 4 2 4H5", refresh: "M4 11a8 8 0 0114-5l2 2M20 13a8 8 0 01-14 5l-2-2M18 4v4h-4M6 20v-4h4",
    panelLeft: "M4 5h16v14H4zM9.5 5v14", settings: "M12 9a3 3 0 100 6 3 3 0 000-6zM19.4 15a1.65 1.65 0 00.33 1.82l.05.05a2 2 0 11-2.83 2.83l-.05-.05a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.08a1.65 1.65 0 00-1-1.51 1.65 1.65 0 00-1.82.33l-.05.05a2 2 0 11-2.83-2.83l.05-.05a1.65 1.65 0 00.33-1.82 1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.08a1.65 1.65 0 001.51-1 1.65 1.65 0 00-.33-1.82l-.05-.05a2 2 0 112.83-2.83l.05.05a1.65 1.65 0 001.82.33H9a1.65 1.65 0 001-1.51V3a2 2 0 014 0v.08a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.05-.05a2 2 0 112.83 2.83l-.05.05a1.65 1.65 0 00-.33 1.82V9a1.65 1.65 0 001.51 1H21a2 2 0 010 4h-.08a1.65 1.65 0 00-1.51 1z",
    box: "M3.3 7.5L12 3l8.7 4.5v9L12 21l-8.7-4.5v-9zM3.3 7.5L12 12m0 0l8.7-4.5M12 12v9",
    globe: "M12 3a9 9 0 100 18 9 9 0 000-18zM3 12h18M12 3c2.5 2.5 3.5 6 3.5 9S14.5 18.5 12 21M12 3c-2.5 2.5-3.5 6-3.5 9S9.5 18.5 12 21",
    server: "M4 5h16v5H4zM4 14h16v5H4zM7.5 7.5h.01M7.5 16.5h.01", dots: "M5 12a1 1 0 100-.01M12 12a1 1 0 100-.01M19 12a1 1 0 100-.01",
  };
  function Icon({ name, size, style }) {
    const d = ICONS[name] || ICONS.dots;
    const w = size === "sm" ? 13 : size === "lg" ? 18 : 15;
    return e("svg", { className: "icon", width: w, height: w, viewBox: "0 0 24 24", fill: "none",
      stroke: "currentColor", strokeWidth: 1.7, strokeLinecap: "round", strokeLinejoin: "round", style, "aria-hidden": true },
      e("path", { d }));
  }

  // ---------- helpers ----------
  const STATUS_META = {
    running: { label: "running", color: "var(--st-running)" }, blocked: { label: "blocked", color: "var(--st-blocked)" },
    waiting: { label: "waiting", color: "var(--st-waiting)" }, done: { label: "done", color: "var(--st-done)" },
    idle: { label: "idle", color: "var(--st-idle)" }, queued: { label: "queued", color: "var(--st-idle)" },
  };
  const TOOL = { claude: { name: "Claude Code", accent: "#d98a5b", g: "✳" }, codex: { name: "Codex", accent: "#10a37f", g: "◆" },
    cursor: { name: "Cursor", accent: "#6e9bff", g: "▸" }, gemini: { name: "Gemini", accent: "#8a7cff", g: "✦" } };
  const fmtDur = (sec) => { if (!sec) return "0s"; if (sec < 60) return sec + "s"; const m = Math.floor(sec / 60), s = sec % 60; if (m < 60) return m + "m" + (s ? " " + s + "s" : ""); const h = Math.floor(m / 60); return h + "h " + (m % 60) + "m"; };
  const logColor = (t) => ({ cmd: "var(--ink)", out: "var(--ink-2)", ok: "var(--st-done)", warn: "#f5c451", err: "var(--st-blocked)", sys: "var(--accent-2)" }[t] || "var(--ink-2)");
  const logPrefix = (t) => ({ cmd: "$", out: " ", ok: "✓", warn: "!", err: "✗", sys: "›" }[t] || " ");

  function StatusDot({ status }) { return e("span", { className: "dot " + status }); }
  function StatusPill({ status, filled }) {
    const m = STATUS_META[status] || STATUS_META.idle;
    return e("span", { className: "chip", style: filled
      ? { color: m.color, borderColor: "color-mix(in oklch, " + m.color + ", transparent 60%)", background: "color-mix(in oklch, " + m.color + ", transparent 88%)" }
      : { color: m.color } },
      e(StatusDot, { status }), e("span", { className: "up", style: { fontSize: 9.5, letterSpacing: ".1em" } }, m.label));
  }
  function ToolBadge({ tool, size = 16 }) {
    const m = TOOL[tool] || { name: tool, accent: "var(--ink-3)", g: "•" };
    return e("span", { title: m.name, style: { width: size, height: size, flex: "none", borderRadius: 4, display: "grid", placeItems: "center",
      fontSize: size * 0.62, lineHeight: 1, color: m.accent, background: "color-mix(in oklch, " + m.accent + ", transparent 84%)",
      border: "1px solid color-mix(in oklch, " + m.accent + ", transparent 64%)" } }, m.g);
  }
  function Ring({ value, size = 36, stroke = 3, color }) {
    const r = (size - stroke) / 2, c = 2 * Math.PI * r;
    return e("svg", { width: size, height: size, style: { transform: "rotate(-90deg)" } },
      e("circle", { cx: size / 2, cy: size / 2, r, fill: "none", stroke: "var(--hair)", strokeWidth: stroke }),
      e("circle", { cx: size / 2, cy: size / 2, r, fill: "none", stroke: color || "var(--accent)", strokeWidth: stroke, strokeLinecap: "round",
        strokeDasharray: c, strokeDashoffset: c * (1 - value), style: { transition: "stroke-dashoffset .6s ease" } }));
  }

  // ---------- data (curated from the app's data.js) ----------
  const PROJECTS = [
    { id: "p_pay", name: "payments-service", color: "#a855f7", icon: "box", branch: "main", head: "a3f91c2" },
    { id: "p_web", name: "web-dashboard", color: "#22d3ee", icon: "globe", branch: "main", head: "7d10b4e" },
    { id: "p_infra", name: "infra-terraform", color: "#34e0a1", icon: "server", branch: "main", head: "f02ce91" },
  ];
  const AGENTS = [
    { id: "a1", projectId: "p_pay", tool: "claude", model: "sonnet-4.6", name: "stripe-retry", status: "running",
      task: "Add exponential-backoff retry to the Stripe webhook handler", branch: "agent/stripe-retry",
      commits: 2, elapsed: 412, progress: 0.68, rate: 0.0004, files: [{ add: 96, del: 8 }, { add: 64, del: 12 }, { add: 28, del: 4 }] },
    { id: "b2", projectId: "p_pay", tool: "claude", model: "opus-4.6", name: "jwt-refresh", status: "blocked",
      task: "Migrate auth to short-lived JWT + rotating refresh tokens", branch: "agent/jwt-refresh",
      commits: 4, elapsed: 1284, progress: 0.52, blockReason: "Needs decision: store refresh tokens in Redis or Postgres?", files: [{ add: 88, del: 40 }, { add: 54, del: 56 }] },
    { id: "c3", projectId: "p_pay", tool: "codex", model: "gpt-5.1-codex · high", name: "reconcile-refactor", status: "running",
      task: "Refactor the nightly reconciliation job into idempotent steps", branch: "agent/reconcile-refactor",
      commits: 1, elapsed: 196, progress: 0.34, rate: 0.0009, files: [{ add: 41, del: 22 }, { add: 23, del: 9 }] },
    { id: "d4", projectId: "p_pay", tool: "claude", model: "sonnet-4.6", name: "checkout-tests", status: "done",
      task: "Write integration tests for the checkout → capture flow", branch: "agent/checkout-tests",
      commits: 3, elapsed: 642, progress: 1, files: [{ add: 236, del: 0 }] },
    { id: "e5", projectId: "p_pay", tool: "gemini", model: "2.5-flash", name: "node22", status: "waiting",
      task: "Upgrade runtime to Node 22 and bump all dependencies", branch: "agent/node22",
      commits: 0, elapsed: 38, progress: 0.08, waitReason: "Waiting on CI — 3 checks queued", files: [{ add: 28, del: 28 }] },
    { id: "g7", projectId: "p_web", tool: "claude", model: "sonnet-4.6", name: "settings-redesign", status: "running",
      task: "Rebuild the account settings page with the new design tokens", branch: "agent/settings-redesign",
      commits: 2, elapsed: 308, progress: 0.46, rate: 0.0006, files: [{ add: 60, del: 24 }, { add: 36, del: 16 }] },
    { id: "h8", projectId: "p_web", tool: "codex", model: "gpt-5.1-codex · medium", name: "a11y-audit", status: "waiting",
      task: "Fix WCAG AA contrast + keyboard-nav issues across the dashboard", branch: "agent/a11y-audit",
      commits: 1, elapsed: 122, progress: 0.21, waitReason: "Waiting on review of focus-trap approach", files: [{ add: 22, del: 14 }] },
    { id: "i9", projectId: "p_infra", tool: "gemini", model: "2.5-pro", name: "tf-modules", status: "done",
      task: "Split monolithic main.tf into reusable network/db/cache modules", branch: "agent/tf-modules",
      commits: 5, elapsed: 904, progress: 1, files: [{ add: 180, del: 120 }] },
    { id: "f6", projectId: "p_pay", tool: "cursor", model: "composer-1", name: "idempotency-keys", status: "queued",
      task: "Add idempotency keys to the public payments API", branch: "agent/idempotency-keys",
      commits: 0, elapsed: 0, progress: 0, files: [] },
  ];
  const LOGS = {
    a1: [{ t: "sys", s: "worktree mounted → worktrees/agent-a1 (claude · sonnet-4.6)" }, { t: "cmd", s: "git checkout -b agent/stripe-retry a3f91c2" }, { t: "out", s: "Switched to a new branch 'agent/stripe-retry'" }, { t: "cmd", s: "pnpm install --frozen-lockfile" }, { t: "ok", s: "Packages: +0  done in 1.2s" }, { t: "cmd", s: "pnpm vitest run src/webhooks" }, { t: "out", s: " ✓ retry.test.ts (8)  ✓ stripe.test.ts (5)" }, { t: "ok", s: "Test Files 2 passed  Tests 13 passed" }, { t: "cmd", s: "git commit -am 'feat: retry stripe webhooks'" }, { t: "ok", s: "[agent/stripe-retry 1f8c2a9] 4 files changed, 188(+)" }],
    b2: [{ t: "sys", s: "worktree mounted → worktrees/agent-b2 (claude · opus-4.6)" }, { t: "cmd", s: "pnpm add jose @types/ms" }, { t: "ok", s: "+ jose 5.2.0   + @types/ms 0.7.34" }, { t: "cmd", s: "pnpm vitest run src/auth" }, { t: "out", s: " ✓ jwt.test.ts (11)   ✗ refresh.test.ts (2 failed)" }, { t: "err", s: "AssertionError: refresh token store not configured" }, { t: "sys", s: "⏸ paused — awaiting human decision (storage backend)" }],
    c3: [{ t: "sys", s: "worktree mounted → worktrees/agent-c3 (codex · gpt-5.1-codex high)" }, { t: "cmd", s: "git checkout -b agent/reconcile-refactor a3f91c2" }, { t: "cmd", s: "rg -n 'reconcile' src/jobs" }, { t: "out", s: "src/jobs/reconcile.ts:14: export async function reconcile()" }, { t: "cmd", s: "pnpm tsc --noEmit" }, { t: "ok", s: "No errors. Clean exit 0" }],
    d4: [{ t: "sys", s: "worktree mounted → worktrees/agent-d4 (claude · sonnet-4.6)" }, { t: "cmd", s: "pnpm vitest run test/integration/checkout.test.ts" }, { t: "out", s: " ✓ checkout.test.ts (14 tests) 1.8s" }, { t: "ok", s: "Test Files 1 passed   Tests 14 passed" }, { t: "cmd", s: "git commit -am 'test: checkout capture suite'" }, { t: "ok", s: "[agent/checkout-tests 9b21e07] 3 files, 236(+)" }, { t: "sys", s: "✓ task complete — ready to merge into main" }],
    e5: [{ t: "sys", s: "worktree mounted → worktrees/agent-e5 (gemini · 2.5-flash)" }, { t: "cmd", s: "node -v && echo 'target: v22'" }, { t: "out", s: "v20.11.0" }, { t: "cmd", s: "gh workflow run ci.yml --ref agent/node22" }, { t: "warn", s: "3 checks queued: lint · typecheck · test-matrix" }],
    g7: [{ t: "sys", s: "worktree mounted → worktrees/agent-g7 (claude · sonnet-4.6)" }, { t: "cmd", s: "pnpm dev --filter web-dashboard" }, { t: "out", s: "VITE v5.4.0  ready in 612 ms" }, { t: "cmd", s: "applying design tokens → Settings.tsx" }, { t: "out", s: "rewrote 6 components to token vars" }],
    h8: [{ t: "sys", s: "worktree mounted → worktrees/agent-h8 (codex · gpt-5.1-codex medium)" }, { t: "cmd", s: "pnpm axe ./src --tags wcag2aa" }, { t: "warn", s: "14 contrast violations · 6 missing focus styles" }, { t: "sys", s: "⏸ awaiting review of focus-trap approach" }],
    i9: [{ t: "sys", s: "worktree mounted → worktrees/agent-i9 (gemini · 2.5-pro)" }, { t: "cmd", s: "terraform fmt -recursive && terraform validate" }, { t: "ok", s: "Success! The configuration is valid." }, { t: "cmd", s: "git commit -am 'refactor: split into modules'" }, { t: "ok", s: "[agent/tf-modules 4cf0a2b] 5 files changed" }, { t: "sys", s: "✓ task complete — ready to merge into main" }],
    f6: [{ t: "sys", s: "queued — awaiting worktree allocation" }],
  };
  const projOf = (id) => PROJECTS.find((p) => p.id === id);

  // ---------- streaming mini terminal ----------
  function MiniTerm({ lines }) {
    return e("div", { style: { background: "var(--bg)", border: "1px solid var(--hair)", borderRadius: "var(--r-sm)", padding: "6px 8px", fontSize: 10, lineHeight: 1.6, overflow: "hidden", height: 56 } },
      lines.length ? lines.map((l, i) => e("div", { key: i, className: i === lines.length - 1 && l.live ? "caret" : "",
        style: { whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", color: logColor(l.t) } },
        e("span", { style: { color: "var(--ink-4)", marginRight: 5 } }, logPrefix(l.t)), l.s))
        : e("span", { style: { color: "var(--ink-4)" } }, "no output yet"));
  }

  // ---------- agent card (Grid view) ----------
  function AgentCard({ ag, lines }) {
    const m = STATUS_META[ag.status];
    const proj = projOf(ag.projectId);
    const totAdd = ag.files.reduce((s, f) => s + f.add, 0);
    const totDel = ag.files.reduce((s, f) => s + f.del, 0);
    return e("div", { className: "surface rise oc-card" },
      e("div", { style: { display: "flex", alignItems: "flex-start", gap: 10 } },
        e("div", { style: { position: "relative", flex: "none" } },
          e(Ring, { value: ag.progress, size: 36, stroke: 3, color: m.color }),
          e("span", { className: "tnum", style: { position: "absolute", inset: 0, display: "grid", placeItems: "center", fontSize: 9, color: "var(--ink-2)" } }, Math.round(ag.progress * 100))),
        e("div", { style: { flex: 1, minWidth: 0 } },
          e("div", { style: { display: "flex", alignItems: "center", gap: 7, minWidth: 0 } },
            e("span", { className: "disp", style: { fontSize: 14, fontWeight: 600, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", flex: "0 1 auto" } }, ag.name),
            e("span", { style: { flex: "none" } }, e(StatusPill, { status: ag.status, filled: true }))),
          e("div", { style: { fontSize: 11, color: "var(--ink-3)", marginTop: 2, display: "flex", gap: 6, alignItems: "center", minWidth: 0 } },
            e("span", { style: { display: "flex", alignItems: "center", gap: 4, color: proj.color, minWidth: 0, overflow: "hidden" } },
              e(Icon, { name: proj.icon, size: "sm", style: { width: 11, height: 11, flex: "none" } }),
              e("span", { style: { overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" } }, proj.name)),
            e("span", { style: { color: "var(--ink-4)", flex: "none" } }, "·"),
            e(Icon, { name: "branch", size: "sm", style: { width: 11, height: 11, flex: "none" } }),
            e("span", { style: { overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" } }, ag.branch.replace("agent/", ""))))),
      e("p", { style: { fontSize: 12, color: "var(--ink-2)", lineHeight: 1.5, minHeight: 34, margin: 0 } }, ag.task),
      ag.status === "blocked" ? e("div", { style: { display: "flex", gap: 7, padding: "7px 9px", borderRadius: "var(--r-sm)",
        background: "color-mix(in oklch, var(--st-blocked), transparent 90%)", border: "1px solid color-mix(in oklch, var(--st-blocked), transparent 70%)" } },
        e(Icon, { name: "flag", size: "sm", style: { color: "var(--st-blocked)", flex: "none", marginTop: 1 } }),
        e("span", { style: { fontSize: 11, color: "var(--code-del-ink)", lineHeight: 1.45 } }, ag.blockReason)) : null,
      e(MiniTerm, { lines }),
      e("div", { className: "tnum", style: { display: "flex", alignItems: "center", gap: 10, fontSize: 10.5, color: "var(--ink-3)" } },
        e("span", { style: { display: "flex", gap: 4 } }, e(Icon, { name: "file", size: "sm", style: { width: 11, height: 11 } }), ag.files.length),
        e("span", { style: { color: "var(--code-add-ink)" } }, "+" + totAdd),
        e("span", { style: { color: "var(--code-del-ink)" } }, "−" + totDel),
        e("span", { style: { display: "flex", gap: 4 } }, e(Icon, { name: "commit", size: "sm", style: { width: 11, height: 11 } }), ag.commits),
        e("span", { style: { marginLeft: "auto", display: "flex", gap: 4, color: "var(--ink-4)" } }, e(Icon, { name: "clock", size: "sm", style: { width: 11, height: 11 } }), ag.elapsed ? fmtDur(ag.elapsed) : "—")),
      e("div", { style: { display: "flex", gap: 6 } },
        ag.status === "done" ? e("button", { className: "btn primary", style: { flex: 1, justifyContent: "center" } }, e(Icon, { name: "merge", size: "sm" }), "Merge")
          : ag.status === "blocked" ? e("button", { className: "btn primary", style: { flex: 1, justifyContent: "center" } }, e(Icon, { name: "chat", size: "sm" }), "Answer")
          : ag.status === "queued" ? e("button", { className: "btn ghost-hair", style: { flex: 1, justifyContent: "center" } }, e(Icon, { name: "play", size: "sm" }), "Start now")
          : e("button", { className: "btn ghost-hair", style: { flex: 1, justifyContent: "center" } }, e(Icon, { name: ag.status === "running" ? "pause" : "play", size: "sm" }), ag.status === "running" ? "Pause" : "Resume"),
        e("button", { className: "btn ghost-hair", style: { padding: "5px 9px" } }, e(Icon, { name: "terminal", size: "sm" }), "Open")));
  }

  // ---------- sidebar ----------
  const STATUS_PRIORITY = { blocked: 0, running: 1, waiting: 2, queued: 3, done: 4, idle: 5 };
  function AgentRow({ ag }) {
    const totAdd = ag.files.reduce((s, f) => s + f.add, 0), totDel = ag.files.reduce((s, f) => s + f.del, 0);
    const needs = ag.status === "blocked";
    return e("div", { className: "oc-arow", style: { display: "flex", flexDirection: "column", gap: 3, padding: "6px 10px 7px", position: "relative", borderRadius: "var(--r-md)", margin: "1px 8px 1px 14px" } },
      e("div", { style: { display: "flex", alignItems: "center", gap: 7 } },
        e(StatusDot, { status: ag.status }),
        e("span", { style: { fontSize: 12.5, color: "var(--ink)", fontWeight: 500, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" } }, ag.name),
        needs ? e("span", { style: { width: 5, height: 5, borderRadius: "50%", background: "var(--st-blocked)", flex: "none" } }) : null,
        e(ToolBadge, { tool: ag.tool, size: 14 }),
        e("span", { className: "tnum", style: { marginLeft: "auto", fontSize: 9.5, color: "var(--ink-4)" } }, ag.elapsed ? fmtDur(ag.elapsed) : "—")),
      e("div", { style: { display: "flex", alignItems: "center", gap: 6, paddingLeft: 15 } },
        e(Icon, { name: "branch", size: "sm", style: { color: "var(--ink-4)", width: 11, height: 11, flex: "none" } }),
        e("span", { style: { fontSize: 10.5, color: "var(--ink-3)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" } }, ag.branch.replace("agent/", "")),
        ag.files.length ? e("span", { className: "tnum", style: { marginLeft: "auto", fontSize: 10, display: "flex", gap: 5, flex: "none" } },
          e("span", { style: { color: "var(--code-add-ink)" } }, "+" + totAdd), e("span", { style: { color: "var(--code-del-ink)" } }, "−" + totDel)) : null),
      ag.status === "running" ? e("div", { className: "activity", style: { marginLeft: 15, marginTop: 2 } }) : null);
  }
  function ProjectGroup({ project, agents }) {
    const sorted = [...agents].sort((a, b) => STATUS_PRIORITY[a.status] - STATUS_PRIORITY[b.status]);
    const needs = agents.filter((a) => a.status === "blocked").length;
    return e("div", { style: { marginBottom: 2 } },
      e("div", { className: "proj-row", style: { display: "flex", alignItems: "center", gap: 8, padding: "7px 10px", margin: "0 6px", borderRadius: "var(--r-md)" } },
        e(Icon, { name: "chevronD", size: "sm", style: { width: 11, height: 11, color: "var(--ink-4)", flex: "none" } }),
        e("span", { style: { width: 19, height: 19, flex: "none", borderRadius: 5, display: "grid", placeItems: "center",
          background: "color-mix(in oklch, " + project.color + ", transparent 82%)", border: "1px solid color-mix(in oklch, " + project.color + ", transparent 62%)" } },
          e(Icon, { name: project.icon, size: "sm", style: { width: 12, height: 12, color: project.color } })),
        e("span", { style: { fontSize: 12.5, fontWeight: 600, color: "var(--ink)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" } }, project.name),
        needs ? e("span", { className: "tnum", style: { fontSize: 9, fontWeight: 700, color: "var(--st-blocked)" } }, needs + "!") : null,
        e("span", { className: "tnum", style: { marginLeft: "auto", fontSize: 9.5, color: "var(--ink-4)", flex: "none" } }, agents.length)),
      e("div", { style: { position: "relative" } },
        e("span", { style: { position: "absolute", left: 21, top: 0, bottom: 4, width: 1, background: "var(--hair)" } }),
        sorted.map((ag) => e(AgentRow, { key: ag.id, ag }))));
  }
  function Sidebar() {
    const totalRunning = AGENTS.filter((a) => a.status === "running").length;
    return e("aside", { style: { display: "flex", flexDirection: "column", minHeight: 0, background: "var(--panel)", borderRight: "1px solid var(--hair)" } },
      e("div", { style: { padding: "10px 12px 8px", borderBottom: "1px solid var(--hair)" } },
        e("div", { style: { display: "flex", alignItems: "center", gap: 7, marginBottom: 8 } },
          e(Icon, { name: "layers", size: "sm", style: { color: "var(--accent)" } }),
          e("span", { className: "up", style: { fontSize: 9.5, color: "var(--ink-3)" } }, "Projects"),
          e("span", { className: "chip tnum", style: { fontSize: 9, padding: "0 6px" } }, PROJECTS.length),
          e("span", { className: "chip tnum", style: { marginLeft: "auto", fontSize: 9, padding: "1px 6px" } },
            e("span", { className: "dot running", style: { width: 6, height: 6 } }), totalRunning + "/5"),
          e("button", { className: "pane-btn" }, e(Icon, { name: "refresh", size: "sm", style: { width: 13, height: 13 } })),
          e("button", { className: "pane-btn" }, e(Icon, { name: "panelLeft", size: "sm", style: { width: 14, height: 14 } }))),
        e("div", { style: { display: "flex", alignItems: "center", gap: 7, padding: "5px 8px", background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-sm)" } },
          e(Icon, { name: "search", size: "sm", style: { color: "var(--ink-4)" } }),
          e("span", { style: { flex: 1, color: "var(--ink-4)", fontSize: 11.5 } }, "filter agents…"))),
      e("div", { className: "scroll-y oc-side-scroll", style: { flex: 1, padding: "6px 0" } },
        PROJECTS.map((p) => e(ProjectGroup, { key: p.id, project: p, agents: AGENTS.filter((a) => a.projectId === p.id) }))),
      e("div", { style: { padding: 10, borderTop: "1px solid var(--hair)", display: "flex", gap: 8 } },
        e("button", { className: "btn ghost-hair", style: { flex: 1, justifyContent: "center" } }, e(Icon, { name: "folder", size: "sm" }), "Add project"),
        e("button", { className: "btn primary", style: { padding: "5px 11px" } }, e(Icon, { name: "bolt", size: "sm" }), "Spawn")));
  }

  // ---------- top bar ----------
  const ROSE = (() => { const A = 22, B = 13, K = -4, T = Math.PI * 2; let d = ""; for (let i = 0; i <= 160; i++) { const t = i / 160 * T; d += (i ? "L" : "M") + (50 + A * Math.cos(t) + B * Math.cos(K * t)).toFixed(2) + " " + (50 + A * Math.sin(t) + B * Math.sin(K * t)).toFixed(2) + " "; } return d + "Z"; })();
  function AppLogo({ size = 24 }) {
    return e("svg", { width: size, height: size, viewBox: "0 0 100 100", fill: "none" },
      e("defs", null,
        e("linearGradient", { id: "ocrose", x1: 0, y1: 0, x2: 1, y2: 1 }, e("stop", { offset: 0, stopColor: "#ff5d9e" }), e("stop", { offset: .5, stopColor: "#a855f7" }), e("stop", { offset: 1, stopColor: "#22d3ee" })),
        e("radialGradient", { id: "occore" }, e("stop", { offset: 0, stopColor: "#fff", stopOpacity: .95 }), e("stop", { offset: .4, stopColor: "#a855f7" }), e("stop", { offset: 1, stopColor: "#a855f7", stopOpacity: .2 }))),
      e("path", { d: ROSE, fill: "none", stroke: "url(#ocrose)", strokeWidth: 4.6, strokeLinejoin: "round" }),
      e("circle", { cx: 50, cy: 50, r: 12, fill: "url(#occore)" }));
  }
  function TopBar({ running, onToggleRun }) {
    const tabs = [{ id: "orch", label: "Orchestrator", kind: "orch" }, { id: "a1", label: "stripe-retry", status: "running" }, { id: "b2", label: "jwt-refresh", status: "blocked" }];
    const [active] = useState("orch");
    const winS = { width: 13, height: 13, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor", strokeWidth: 1.8, strokeLinecap: "round", strokeLinejoin: "round" };
    return e("header", { style: { display: "flex", alignItems: "stretch", background: "var(--panel)", borderBottom: "1px solid var(--hair)", height: 44, position: "relative", zIndex: 5 } },
      e("div", { style: { display: "flex", alignItems: "center", gap: 11, flex: "none", width: 248, padding: "0 14px", borderRight: "1px solid var(--hair)" } },
        e(AppLogo, { size: 24 }),
        e("div", { style: { display: "flex", flexDirection: "column", lineHeight: 1.12, minWidth: 0 } },
          e("span", { className: "disp", style: { fontSize: 15, fontWeight: 600, display: "flex", alignItems: "center", gap: 8 } },
            e("span", null, e("span", { style: { color: "var(--accent)" } }, "O"), "rrery"),
            e("span", { style: { display: "inline-flex", alignItems: "center", gap: 5, height: 17, padding: "0 7px", borderRadius: 999, border: "1px solid color-mix(in oklch, var(--accent-2), transparent 58%)", background: "color-mix(in oklch, var(--accent-2), transparent 88%)", fontFamily: "var(--font-mono)" } },
              e("span", { style: { fontSize: 10, color: "var(--ink-2)" } }, "v0.2.3"),
              e("span", { style: { width: 1, height: 9, background: "color-mix(in oklch, var(--accent-2), transparent 50%)" } }),
              e("span", { className: "up", style: { fontSize: 8.5, fontWeight: 700, letterSpacing: ".12em", color: "var(--accent-2)" } }, "BETA"))),
          e("span", { style: { fontSize: 9.5, color: "var(--ink-3)", letterSpacing: ".04em" } }, PROJECTS.length + " projects · " + AGENTS.length + " agents"))),
      e("div", { style: { display: "flex", alignItems: "stretch", flex: 1, minWidth: 0, overflow: "hidden" } },
        tabs.map((tab) => e("div", { key: tab.id, style: { display: "flex", alignItems: "center", gap: 8, padding: "0 13px", whiteSpace: "nowrap", position: "relative", borderRight: "1px solid var(--hair)", background: active === tab.id ? "var(--panel-2)" : "transparent", color: active === tab.id ? "var(--ink)" : "var(--ink-3)" } },
          active === tab.id ? e("span", { style: { position: "absolute", left: 0, right: 0, top: 0, height: 2, background: "linear-gradient(90deg, var(--accent), var(--accent-2))" } }) : null,
          tab.kind === "orch" ? e(Icon, { name: "layers", size: "sm", style: { color: active === tab.id ? "var(--accent)" : "inherit" } }) : e(StatusDot, { status: tab.status }),
          e("span", { style: { fontSize: 12 } }, tab.label),
          tab.kind !== "orch" ? e(Icon, { name: "x", size: "sm", style: { color: "var(--ink-4)" } }) : null))),
      e("div", { style: { display: "flex", alignItems: "center", gap: 8, padding: "0 12px", flex: "none", borderLeft: "1px solid var(--hair)" } },
        e("button", { className: "btn " + (running ? "ghost-hair" : "primary"), onClick: onToggleRun },
          e(Icon, { name: running ? "pause" : "play", size: "sm" }), running ? "Pause all" : "Run all"),
        e("button", { className: "btn ghost-hair", style: { padding: "5px 8px" } }, e(Icon, { name: "sun", size: "sm" })),
        e("button", { className: "btn ghost-hair", style: { padding: "5px 8px" } }, e(Icon, { name: "settings", size: "sm" })),
        e("div", { style: { display: "flex", gap: 2, marginLeft: 4 } },
          ["M6 12h12", "M6 6h12v12H6z", "M6 6l12 12M18 6L6 18"].map((d, i) => e("button", { key: i, className: "oc-winbtn" }, e("svg", winS, e("path", { d })))))));
  }

  // ---------- overview ----------
  function StatBlock({ n, label, color, pulse }) {
    return e("div", { style: { display: "flex", flexDirection: "column", gap: 2, paddingRight: 20 } },
      e("div", { style: { display: "flex", alignItems: "baseline", gap: 6 } },
        e("span", { className: "disp tnum", style: { fontSize: 24, fontWeight: 600, color: color || "var(--ink)", lineHeight: 1 } }, n),
        pulse ? e("span", { className: "dot running", style: { background: color } }) : null),
      e("span", { className: "up", style: { fontSize: 9, color: "var(--ink-3)" } }, label));
  }
  function Overview({ running, onToggleRun, streams }) {
    const [viz, setViz] = useState("grid");
    const count = (s) => AGENTS.filter((a) => a.status === s).length;
    const VIZ = [["grid", "grid", "Grid"], ["kanban", "columns", "Board"], ["graph", "graph", "Graph"], ["timeline", "timeline", "Timeline"]];
    return e("div", { style: { display: "flex", flexDirection: "column", minHeight: 0, background: "var(--panel-2)", flex: 1 } },
      e("div", { style: { display: "flex", alignItems: "center", padding: "14px 18px", borderBottom: "1px solid var(--hair)", background: "var(--panel)" } },
        e("div", { style: { marginRight: 24 } },
          e("h1", { className: "disp", style: { fontSize: 16, fontWeight: 600, letterSpacing: "-.02em", margin: 0 } }, "Orchestrator"),
          e("span", { style: { fontSize: 10.5, color: "var(--ink-3)" } }, AGENTS.length + " agents across " + PROJECTS.length + " projects · " + ORG)),
        e(StatBlock, { n: count("running"), label: "Running", color: "var(--st-running)", pulse: true }),
        e(StatBlock, { n: count("blocked"), label: "Need you", color: "var(--st-blocked)" }),
        e(StatBlock, { n: count("waiting") + count("queued"), label: "Waiting", color: "var(--st-waiting)" }),
        e(StatBlock, { n: count("done"), label: "Done", color: "var(--st-done)" }),
        e("div", { style: { marginLeft: "auto", display: "flex", alignItems: "center", gap: 8 } },
          e("div", { style: { display: "flex", gap: 2, padding: 3, background: "var(--panel-2)", borderRadius: "var(--r-md)", border: "1px solid var(--hair)" } },
            VIZ.map((v) => e("button", { key: v[0], className: "btn", onClick: () => setViz(v[0]),
              style: { padding: "4px 9px", borderRadius: "var(--r-sm)", background: viz === v[0] ? "var(--panel-3)" : "transparent", color: viz === v[0] ? "var(--ink)" : "var(--ink-3)", boxShadow: viz === v[0] ? "0 0 0 1px var(--hair-2)" : "none" } },
              e(Icon, { name: v[1], size: "sm", style: { color: viz === v[0] ? "var(--accent)" : "inherit" } }), v[2]))),
          e("button", { className: "btn primary" }, e(Icon, { name: "bolt", size: "sm" }), "Spawn"))),
      e("div", { className: "scroll-y oc-grid-scroll", style: { flex: 1 } },
        e("div", { style: { display: "grid", gap: 14, padding: 18, gridTemplateColumns: "repeat(auto-fill, minmax(320px, 1fr))", alignContent: "start" } },
          AGENTS.map((ag) => e(AgentCard, { key: ag.id, ag, lines: streams[ag.id] })))));
  }

  // ---------- root ----------
  function OrreryAppConsole() {
    const [tick, setTick] = useState(0);
    const [running, setRunning] = useState(true);
    useEffect(() => {
      const reduce = window.matchMedia && matchMedia("(prefers-reduced-motion:reduce)").matches;
      if (reduce || !running) return;
      const id = setInterval(() => setTick((t) => t + 1), 1100);
      return () => clearInterval(id);
    }, [running]);

    // derive live agent state + streaming log windows
    AGENTS.forEach((a) => {
      if (a._e0 === undefined) { a._e0 = a.elapsed; a._p0 = a.progress; }
      if (running && (a.status === "running" || a.status === "waiting")) a.elapsed = a._e0 + tick;
      if (running && a.status === "running") a.progress = Math.min(0.97, a._p0 + tick * (a.rate ? a.rate * 1100 : 0.004));
    });
    const streams = {};
    AGENTS.forEach((a) => {
      const L = LOGS[a.id] || [];
      if (a.status === "running" && running && L.length > 3) {
        const span = L.length - 2;
        const end = 3 + (tick % span);
        const win = L.slice(Math.max(0, end - 3), end).map((l) => ({ ...l }));
        if (win.length) win[win.length - 1] = { ...win[win.length - 1], live: true };
        streams[a.id] = win;
      } else {
        streams[a.id] = L.slice(-3).map((l) => ({ ...l }));
      }
    });

    return e("div", { className: "oc-app", style: { width: 1340, height: 812, display: "flex", flexDirection: "column", background: "var(--panel-2)", borderRadius: 14, overflow: "hidden", border: "1px solid var(--hair-2)", fontFamily: "var(--font-mono)", color: "var(--ink)" } },
      e(TopBar, { running, onToggleRun: () => setRunning((r) => !r) }),
      e("div", { style: { display: "flex", flex: 1, minHeight: 0 } },
        e("div", { style: { width: 248, flex: "none", display: "flex", minHeight: 0 } }, e(Sidebar)),
        e(Overview, { running, onToggleRun: () => setRunning((r) => !r), streams })));
  }

  window.OrreryAppConsole = OrreryAppConsole;
})();
