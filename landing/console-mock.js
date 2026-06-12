/* Orrery landing — static, faithful render of the app's "Orchestrator" view.
   Ported from the design handoff (orrery-console.jsx) to dependency-free DOM.
   Visual tokens & .oc-app .dot/.btn/.surface/.chip/.activity classes live in index.html. */
(function () {
  "use strict";
  const ORG = "northwind";

  const ICONS = {
    branch: "M6 4v9M6 13a3 3 0 003 3h3a3 3 0 003-3V9M6 4a2 2 0 100-.01M15 9a2 2 0 100-.01M9 19a2 2 0 100-.01",
    terminal: "M5 6l5 4-5 4M12 16h7", chat: "M4 5h16v10H9l-4 4V5z",
    commit: "M4 12h5m6 0h5M12 9a3 3 0 100 6 3 3 0 000-6z", play: "M7 5l11 7-11 7V5z",
    pause: "M8 5v14M16 5v14", x: "M6 6l12 12M18 6L6 18", chevronD: "M6 9l6 6 6-6",
    folder: "M4 7a2 2 0 012-2h4l2 2h6a2 2 0 012 2v8a2 2 0 01-2 2H6a2 2 0 01-2-2V7z",
    file: "M7 3h7l5 5v13H7V3z M14 3v5h5", clock: "M12 7v5l4 2M12 3a9 9 0 100 18 9 9 0 000-18z",
    bolt: "M13 3L5 14h6l-1 7 8-11h-6l1-7z", search: "M11 4a7 7 0 105 12l4 4M11 4a7 7 0 010 14",
    sun: "M12 4V2m0 20v-2m8-8h2M2 12h2m13.7-5.7l1.4-1.4M4.9 19.1l1.4-1.4m0-11.4L4.9 4.9m14.2 14.2l-1.4-1.4M12 8a4 4 0 100 8 4 4 0 000-8z",
    merge: "M7 4v8a4 4 0 004 4h6M7 4a2 2 0 100-.01M17 16a2 2 0 100 .01M7 12a2 2 0 100 .01",
    layers: "M12 3l9 5-9 5-9-5 9-5zM3 13l9 5 9-5", grid: "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z",
    columns: "M4 4h4v16H4zM10 4h4v16h-4zM16 4h4v16h-4z", timeline: "M4 7h16M4 12h10M4 17h13M19 10v4",
    graph: "M6 6a2 2 0 100-.01M18 6a2 2 0 100-.01M12 18a2 2 0 100-.01M7.5 7.5l3 8M16.5 7.5l-3 8",
    flag: "M5 21V4m0 0h11l-2 4 2 4H5", refresh: "M4 11a8 8 0 0114-5l2 2M20 13a8 8 0 01-14 5l-2-2M18 4v4h-4M6 20v-4h4",
    panelLeft: "M4 5h16v14H4zM9.5 5v14",
    settings: "M12 9a3 3 0 100 6 3 3 0 000-6zM19.4 15a1.65 1.65 0 00.33 1.82l.05.05a2 2 0 11-2.83 2.83l-.05-.05a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.08a1.65 1.65 0 00-1-1.51 1.65 1.65 0 00-1.82.33l-.05.05a2 2 0 11-2.83-2.83l.05-.05a1.65 1.65 0 00.33-1.82 1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.08a1.65 1.65 0 001.51-1 1.65 1.65 0 00-.33-1.82l-.05-.05a2 2 0 112.83-2.83l.05.05a1.65 1.65 0 001.82.33H9a1.65 1.65 0 001-1.51V3a2 2 0 014 0v.08a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.05-.05a2 2 0 112.83 2.83l-.05.05a1.65 1.65 0 00-.33 1.82V9a1.65 1.65 0 001.51 1H21a2 2 0 010 4h-.08a1.65 1.65 0 00-1.51 1z",
    box: "M3.3 7.5L12 3l8.7 4.5v9L12 21l-8.7-4.5v-9zM3.3 7.5L12 12m0 0l8.7-4.5M12 12v9",
    globe: "M12 3a9 9 0 100 18 9 9 0 000-18zM3 12h18M12 3c2.5 2.5 3.5 6 3.5 9S14.5 18.5 12 21M12 3c-2.5 2.5-3.5 6-3.5 9S9.5 18.5 12 21",
    server: "M4 5h16v5H4zM4 14h16v5H4zM7.5 7.5h.01M7.5 16.5h.01",
  };
  const icon = (name, w = 15, style = "") =>
    `<svg class="icon" width="${w}" height="${w}" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.7" stroke-linecap="round" stroke-linejoin="round" style="${style}" aria-hidden="true"><path d="${ICONS[name] || ICONS.box}"/></svg>`;

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

  const statusDot = (s) => `<span class="dot ${s}"></span>`;
  const statusPill = (s) => {
    const m = STATUS_META[s] || STATUS_META.idle;
    return `<span class="chip" style="color:${m.color};border-color:color-mix(in oklch, ${m.color}, transparent 60%);background:color-mix(in oklch, ${m.color}, transparent 88%)">${statusDot(s)}<span class="up" style="font-size:9.5px;letter-spacing:.1em">${m.label}</span></span>`;
  };
  const toolBadge = (tool, size = 16) => {
    const m = TOOL[tool] || { name: tool, accent: "var(--ink-3)", g: "•" };
    return `<span title="${m.name}" style="width:${size}px;height:${size}px;flex:none;border-radius:4px;display:grid;place-items:center;font-size:${size * 0.62}px;line-height:1;color:${m.accent};background:color-mix(in oklch, ${m.accent}, transparent 84%);border:1px solid color-mix(in oklch, ${m.accent}, transparent 64%)">${m.g}</span>`;
  };
  const ring = (value, size, stroke, color) => {
    const r = (size - stroke) / 2, c = 2 * Math.PI * r;
    return `<svg width="${size}" height="${size}" style="transform:rotate(-90deg)"><circle cx="${size / 2}" cy="${size / 2}" r="${r}" fill="none" stroke="var(--hair)" stroke-width="${stroke}"/><circle cx="${size / 2}" cy="${size / 2}" r="${r}" fill="none" stroke="${color}" stroke-width="${stroke}" stroke-linecap="round" stroke-dasharray="${c}" stroke-dashoffset="${c * (1 - value)}"/></svg>`;
  };

  const PROJECTS = [
    { id: "p_pay", name: "payments-service", color: "#a855f7", icon: "box" },
    { id: "p_web", name: "web-dashboard", color: "#22d3ee", icon: "globe" },
    { id: "p_infra", name: "infra-terraform", color: "#34e0a1", icon: "server" },
  ];
  const AGENTS = [
    { id: "a1", projectId: "p_pay", tool: "claude", model: "sonnet-4.6", name: "stripe-retry", status: "running", task: "Add exponential-backoff retry to the Stripe webhook handler", branch: "agent/stripe-retry", commits: 2, elapsed: 412, progress: 0.68, files: [{ add: 96, del: 8 }, { add: 64, del: 12 }, { add: 28, del: 4 }] },
    { id: "b2", projectId: "p_pay", tool: "claude", model: "opus-4.6", name: "jwt-refresh", status: "blocked", task: "Migrate auth to short-lived JWT + rotating refresh tokens", branch: "agent/jwt-refresh", commits: 4, elapsed: 1284, progress: 0.52, blockReason: "Needs decision: store refresh tokens in Redis or Postgres?", files: [{ add: 88, del: 40 }, { add: 54, del: 56 }] },
    { id: "c3", projectId: "p_pay", tool: "codex", model: "gpt-5.1-codex · high", name: "reconcile-refactor", status: "running", task: "Refactor the nightly reconciliation job into idempotent steps", branch: "agent/reconcile-refactor", commits: 1, elapsed: 196, progress: 0.34, files: [{ add: 41, del: 22 }, { add: 23, del: 9 }] },
    { id: "d4", projectId: "p_pay", tool: "claude", model: "sonnet-4.6", name: "checkout-tests", status: "done", task: "Write integration tests for the checkout → capture flow", branch: "agent/checkout-tests", commits: 3, elapsed: 642, progress: 1, files: [{ add: 236, del: 0 }] },
    { id: "e5", projectId: "p_pay", tool: "gemini", model: "2.5-flash", name: "node22", status: "waiting", task: "Upgrade runtime to Node 22 and bump all dependencies", branch: "agent/node22", commits: 0, elapsed: 38, progress: 0.08, waitReason: "Waiting on CI — 3 checks queued", files: [{ add: 28, del: 28 }] },
    { id: "g7", projectId: "p_web", tool: "claude", model: "sonnet-4.6", name: "settings-redesign", status: "running", task: "Rebuild the account settings page with the new design tokens", branch: "agent/settings-redesign", commits: 2, elapsed: 308, progress: 0.46, files: [{ add: 60, del: 24 }, { add: 36, del: 16 }] },
    { id: "h8", projectId: "p_web", tool: "codex", model: "gpt-5.1-codex · medium", name: "a11y-audit", status: "waiting", task: "Fix WCAG AA contrast + keyboard-nav issues across the dashboard", branch: "agent/a11y-audit", commits: 1, elapsed: 122, progress: 0.21, waitReason: "Waiting on review of focus-trap approach", files: [{ add: 22, del: 14 }] },
    { id: "i9", projectId: "p_infra", tool: "gemini", model: "2.5-pro", name: "tf-modules", status: "done", task: "Split monolithic main.tf into reusable network/db/cache modules", branch: "agent/tf-modules", commits: 5, elapsed: 904, progress: 1, files: [{ add: 180, del: 120 }] },
    { id: "f6", projectId: "p_pay", tool: "cursor", model: "composer-1", name: "idempotency-keys", status: "queued", task: "Add idempotency keys to the public payments API", branch: "agent/idempotency-keys", commits: 0, elapsed: 0, progress: 0, files: [] },
  ];
  const LOGS = {
    a1: [{ t: "cmd", s: "pnpm vitest run src/webhooks" }, { t: "out", s: " ✓ retry.test.ts (8)  ✓ stripe.test.ts (5)" }, { t: "ok", s: "Test Files 2 passed  Tests 13 passed", live: true }],
    b2: [{ t: "out", s: " ✗ refresh.test.ts (2 failed)" }, { t: "err", s: "AssertionError: refresh token store not configured" }, { t: "sys", s: "⏸ paused — awaiting human decision" }],
    c3: [{ t: "cmd", s: "rg -n 'reconcile' src/jobs" }, { t: "out", s: "src/jobs/reconcile.ts:14: export async function reconcile()" }, { t: "ok", s: "tsc --noEmit · clean exit 0", live: true }],
    d4: [{ t: "ok", s: "[agent/checkout-tests 9b21e07] 3 files, 236(+)" }, { t: "sys", s: "✓ task complete — ready to merge into main" }],
    e5: [{ t: "cmd", s: "gh workflow run ci.yml --ref agent/node22" }, { t: "warn", s: "3 checks queued: lint · typecheck · test-matrix" }],
    g7: [{ t: "out", s: "VITE v5.4.0  ready in 612 ms" }, { t: "cmd", s: "applying design tokens → Settings.tsx" }, { t: "out", s: "rewrote 6 components to token vars", live: true }],
    h8: [{ t: "warn", s: "14 contrast violations · 6 missing focus styles" }, { t: "sys", s: "⏸ awaiting review of focus-trap approach" }],
    i9: [{ t: "ok", s: "Success! The configuration is valid." }, { t: "sys", s: "✓ task complete — ready to merge into main" }],
    f6: [{ t: "sys", s: "queued — awaiting worktree allocation" }],
  };
  const projOf = (id) => PROJECTS.find((p) => p.id === id);
  const tot = (files, k) => files.reduce((s, f) => s + f[k], 0);

  const miniTerm = (lines) => {
    const body = lines.length
      ? lines.map((l, i) => `<div class="${i === lines.length - 1 && l.live ? "caret" : ""}" style="white-space:nowrap;overflow:hidden;text-overflow:ellipsis;color:${logColor(l.t)}"><span style="color:var(--ink-4);margin-right:5px">${logPrefix(l.t)}</span>${l.s}</div>`).join("")
      : `<span style="color:var(--ink-4)">no output yet</span>`;
    return `<div style="background:var(--bg);border:1px solid var(--hair);border-radius:var(--r-sm);padding:6px 8px;font-size:10px;line-height:1.6;overflow:hidden;height:56px">${body}</div>`;
  };

  function agentCard(ag) {
    const m = STATUS_META[ag.status], proj = projOf(ag.projectId);
    const totAdd = tot(ag.files, "add"), totDel = tot(ag.files, "del");
    const action = ag.status === "done"
      ? `<button class="btn primary" style="flex:1;justify-content:center">${icon("merge", 13)}Merge</button>`
      : ag.status === "blocked"
      ? `<button class="btn primary" style="flex:1;justify-content:center">${icon("chat", 13)}Answer</button>`
      : ag.status === "queued"
      ? `<button class="btn ghost-hair" style="flex:1;justify-content:center">${icon("play", 13)}Start now</button>`
      : `<button class="btn ghost-hair" style="flex:1;justify-content:center">${icon(ag.status === "running" ? "pause" : "play", 13)}${ag.status === "running" ? "Pause" : "Resume"}</button>`;
    return `<div class="surface oc-card">
      <div style="display:flex;align-items:flex-start;gap:10px">
        <div style="position:relative;flex:none">${ring(ag.progress, 36, 3, m.color)}<span class="tnum" style="position:absolute;inset:0;display:grid;place-items:center;font-size:9px;color:var(--ink-2)">${Math.round(ag.progress * 100)}</span></div>
        <div style="flex:1;min-width:0">
          <div style="display:flex;align-items:center;gap:7px;min-width:0">
            <span class="disp" style="font-size:14px;font-weight:600;white-space:nowrap;overflow:hidden;text-overflow:ellipsis;flex:0 1 auto">${ag.name}</span>
            <span style="flex:none">${statusPill(ag.status)}</span>
          </div>
          <div style="font-size:11px;color:var(--ink-3);margin-top:2px;display:flex;gap:6px;align-items:center;min-width:0">
            <span style="display:flex;align-items:center;gap:4px;color:${proj.color};min-width:0;overflow:hidden">${icon(proj.icon, 11)}<span style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${proj.name}</span></span>
            <span style="color:var(--ink-4);flex:none">·</span>${icon("branch", 11)}
            <span style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${ag.branch.replace("agent/", "")}</span>
          </div>
        </div>
      </div>
      <p style="font-size:12px;color:var(--ink-2);line-height:1.5;min-height:34px;margin:0">${ag.task}</p>
      ${ag.status === "blocked" ? `<div style="display:flex;gap:7px;padding:7px 9px;border-radius:var(--r-sm);background:color-mix(in oklch, var(--st-blocked), transparent 90%);border:1px solid color-mix(in oklch, var(--st-blocked), transparent 70%)">${icon("flag", 13, "color:var(--st-blocked);flex:none;margin-top:1px")}<span style="font-size:11px;color:var(--code-del-ink);line-height:1.45">${ag.blockReason}</span></div>` : ""}
      ${miniTerm(LOGS[ag.id] || [])}
      <div class="tnum" style="display:flex;align-items:center;gap:10px;font-size:10.5px;color:var(--ink-3)">
        <span style="display:flex;gap:4px">${icon("file", 11)}${ag.files.length}</span>
        <span style="color:var(--code-add-ink)">+${totAdd}</span>
        <span style="color:var(--code-del-ink)">−${totDel}</span>
        <span style="display:flex;gap:4px">${icon("commit", 11)}${ag.commits}</span>
        <span style="margin-left:auto;display:flex;gap:4px;color:var(--ink-4)">${icon("clock", 11)}${ag.elapsed ? fmtDur(ag.elapsed) : "—"}</span>
      </div>
      <div style="display:flex;gap:6px">${action}<button class="btn ghost-hair" style="padding:5px 9px">${icon("terminal", 13)}Open</button></div>
    </div>`;
  }

  const STATUS_PRIORITY = { blocked: 0, running: 1, waiting: 2, queued: 3, done: 4, idle: 5 };
  function agentRow(ag) {
    const totAdd = tot(ag.files, "add"), totDel = tot(ag.files, "del");
    return `<div class="oc-arow" style="display:flex;flex-direction:column;gap:3px;padding:6px 10px 7px;position:relative;border-radius:var(--r-md);margin:1px 8px 1px 14px">
      <div style="display:flex;align-items:center;gap:7px">${statusDot(ag.status)}
        <span style="font-size:12.5px;color:var(--ink);font-weight:500;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${ag.name}</span>
        ${ag.status === "blocked" ? `<span style="width:5px;height:5px;border-radius:50%;background:var(--st-blocked);flex:none"></span>` : ""}
        ${toolBadge(ag.tool, 14)}
        <span class="tnum" style="margin-left:auto;font-size:9.5px;color:var(--ink-4)">${ag.elapsed ? fmtDur(ag.elapsed) : "—"}</span>
      </div>
      <div style="display:flex;align-items:center;gap:6px;padding-left:15px">${icon("branch", 11, "color:var(--ink-4);flex:none")}
        <span style="font-size:10.5px;color:var(--ink-3);overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${ag.branch.replace("agent/", "")}</span>
        ${ag.files.length ? `<span class="tnum" style="margin-left:auto;font-size:10px;display:flex;gap:5px;flex:none"><span style="color:var(--code-add-ink)">+${totAdd}</span><span style="color:var(--code-del-ink)">−${totDel}</span></span>` : ""}
      </div>
      ${ag.status === "running" ? `<div class="activity" style="margin-left:15px;margin-top:2px"></div>` : ""}
    </div>`;
  }
  function projectGroup(p) {
    const ags = AGENTS.filter((a) => a.projectId === p.id).sort((a, b) => STATUS_PRIORITY[a.status] - STATUS_PRIORITY[b.status]);
    const needs = ags.filter((a) => a.status === "blocked").length;
    return `<div style="margin-bottom:2px">
      <div class="proj-row" style="display:flex;align-items:center;gap:8px;padding:7px 10px;margin:0 6px;border-radius:var(--r-md)">
        ${icon("chevronD", 11, "color:var(--ink-4);flex:none")}
        <span style="width:19px;height:19px;flex:none;border-radius:5px;display:grid;place-items:center;background:color-mix(in oklch, ${p.color}, transparent 82%);border:1px solid color-mix(in oklch, ${p.color}, transparent 62%)">${icon(p.icon, 12, "color:" + p.color)}</span>
        <span style="font-size:12.5px;font-weight:600;color:var(--ink);overflow:hidden;text-overflow:ellipsis;white-space:nowrap">${p.name}</span>
        ${needs ? `<span class="tnum" style="font-size:9px;font-weight:700;color:var(--st-blocked)">${needs}!</span>` : ""}
        <span class="tnum" style="margin-left:auto;font-size:9.5px;color:var(--ink-4);flex:none">${ags.length}</span>
      </div>
      <div style="position:relative"><span style="position:absolute;left:21px;top:0;bottom:4px;width:1px;background:var(--hair)"></span>${ags.map(agentRow).join("")}</div>
    </div>`;
  }

  const ROSE = (() => { const A = 22, B = 13, K = -4, T = Math.PI * 2; let d = ""; for (let i = 0; i <= 160; i++) { const t = i / 160 * T; d += (i ? "L" : "M") + (50 + A * Math.cos(t) + B * Math.cos(K * t)).toFixed(2) + " " + (50 + A * Math.sin(t) + B * Math.sin(K * t)).toFixed(2) + " "; } return d + "Z"; })();
  const appLogo = (size) => `<svg width="${size}" height="${size}" viewBox="0 0 100 100" fill="none"><defs><linearGradient id="ocrose" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#ff5d9e"/><stop offset=".5" stop-color="#a855f7"/><stop offset="1" stop-color="#22d3ee"/></linearGradient><radialGradient id="occore"><stop offset="0" stop-color="#fff" stop-opacity=".95"/><stop offset=".4" stop-color="#a855f7"/><stop offset="1" stop-color="#a855f7" stop-opacity=".2"/></radialGradient></defs><path d="${ROSE}" fill="none" stroke="url(#ocrose)" stroke-width="4.6" stroke-linejoin="round"/><circle cx="50" cy="50" r="12" fill="url(#occore)"/></svg>`;

  function topBar() {
    const tabs = [{ label: "Orchestrator", kind: "orch", on: true }, { label: "stripe-retry", status: "running" }, { label: "jwt-refresh", status: "blocked" }];
    const tab = (t) => `<div style="display:flex;align-items:center;gap:8px;padding:0 13px;white-space:nowrap;position:relative;border-right:1px solid var(--hair);background:${t.on ? "var(--panel-2)" : "transparent"};color:${t.on ? "var(--ink)" : "var(--ink-3)"}">${t.on ? `<span style="position:absolute;left:0;right:0;top:0;height:2px;background:linear-gradient(90deg, var(--accent), var(--accent-2))"></span>` : ""}${t.kind === "orch" ? icon("layers", 13, t.on ? "color:var(--accent)" : "") : statusDot(t.status)}<span style="font-size:12px">${t.label}</span>${t.kind !== "orch" ? icon("x", 13, "color:var(--ink-4)") : ""}</div>`;
    const winBtns = ["M6 12h12", "M6 6h12v12H6z", "M6 6l12 12M18 6L6 18"].map((d) => `<button class="oc-winbtn"><svg width="13" height="13" viewBox="0 0 24 24" fill="none" stroke="currentColor" stroke-width="1.8" stroke-linecap="round" stroke-linejoin="round"><path d="${d}"/></svg></button>`).join("");
    return `<header style="display:flex;align-items:stretch;background:var(--panel);border-bottom:1px solid var(--hair);height:44px;position:relative;z-index:5">
      <div style="display:flex;align-items:center;gap:11px;flex:none;width:248px;padding:0 14px;border-right:1px solid var(--hair)">${appLogo(24)}
        <div style="display:flex;flex-direction:column;line-height:1.12;min-width:0">
          <span class="disp" style="font-size:15px;font-weight:600;display:flex;align-items:center;gap:8px"><span><span style="color:var(--accent)">O</span>rrery</span>
            <span style="display:inline-flex;align-items:center;gap:5px;height:17px;padding:0 7px;border-radius:999px;border:1px solid color-mix(in oklch, var(--accent-2), transparent 58%);background:color-mix(in oklch, var(--accent-2), transparent 88%);font-family:var(--font-mono)"><span style="font-size:10px;color:var(--ink-2)">v0.2.5</span><span style="width:1px;height:9px;background:color-mix(in oklch, var(--accent-2), transparent 50%)"></span><span class="up" style="font-size:8.5px;font-weight:700;letter-spacing:.12em;color:var(--accent-2)">BETA</span></span></span>
          <span style="font-size:9.5px;color:var(--ink-3);letter-spacing:.04em">${PROJECTS.length} projects · ${AGENTS.length} agents</span>
        </div>
      </div>
      <div style="display:flex;align-items:stretch;flex:1;min-width:0;overflow:hidden">${tabs.map(tab).join("")}</div>
      <div style="display:flex;align-items:center;gap:8px;padding:0 12px;flex:none;border-left:1px solid var(--hair)">
        <button class="btn ghost-hair">${icon("pause", 13)}Pause all</button>
        <button class="btn ghost-hair" style="padding:5px 8px">${icon("sun", 13)}</button>
        <button class="btn ghost-hair" style="padding:5px 8px">${icon("settings", 13)}</button>
        <div style="display:flex;gap:2px;margin-left:4px">${winBtns}</div>
      </div>
    </header>`;
  }

  function sidebar() {
    const running = AGENTS.filter((a) => a.status === "running").length;
    return `<aside style="display:flex;flex-direction:column;min-height:0;background:var(--panel);border-right:1px solid var(--hair);width:248px;flex:none">
      <div style="padding:10px 12px 8px;border-bottom:1px solid var(--hair)">
        <div style="display:flex;align-items:center;gap:7px;margin-bottom:8px">${icon("layers", 13, "color:var(--accent)")}<span class="up" style="font-size:9.5px;color:var(--ink-3)">Projects</span><span class="chip tnum" style="font-size:9px;padding:0 6px">${PROJECTS.length}</span><span class="chip tnum" style="margin-left:auto;font-size:9px;padding:1px 6px"><span class="dot running" style="width:6px;height:6px"></span>${running}/5</span><button class="pane-btn">${icon("refresh", 13)}</button><button class="pane-btn">${icon("panelLeft", 14)}</button></div>
        <div style="display:flex;align-items:center;gap:7px;padding:5px 8px;background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-sm)">${icon("search", 13, "color:var(--ink-4)")}<span style="flex:1;color:var(--ink-4);font-size:11.5px">filter agents…</span></div>
      </div>
      <div class="scroll-y oc-side-scroll" style="flex:1;padding:6px 0">${PROJECTS.map(projectGroup).join("")}</div>
      <div style="padding:10px;border-top:1px solid var(--hair);display:flex;gap:8px"><button class="btn ghost-hair" style="flex:1;justify-content:center">${icon("folder", 13)}Add project</button><button class="btn primary" style="padding:5px 11px">${icon("bolt", 13)}Spawn</button></div>
    </aside>`;
  }

  function overview() {
    const count = (s) => AGENTS.filter((a) => a.status === s).length;
    const stat = (n, label, color, pulse) => `<div style="display:flex;flex-direction:column;gap:2px;padding-right:20px"><div style="display:flex;align-items:baseline;gap:6px"><span class="disp tnum" style="font-size:24px;font-weight:600;color:${color};line-height:1">${n}</span>${pulse ? `<span class="dot running" style="background:${color}"></span>` : ""}</div><span class="up" style="font-size:9px;color:var(--ink-3)">${label}</span></div>`;
    const VIZ = [["grid", "Grid", true], ["columns", "Board"], ["graph", "Graph"], ["timeline", "Timeline"]];
    const vizBtn = (v) => `<button class="btn" style="padding:4px 9px;border-radius:var(--r-sm);background:${v[2] ? "var(--panel-3)" : "transparent"};color:${v[2] ? "var(--ink)" : "var(--ink-3)"};box-shadow:${v[2] ? "0 0 0 1px var(--hair-2)" : "none"}">${icon(v[0], 13, v[2] ? "color:var(--accent)" : "")}${v[1]}</button>`;
    return `<div style="display:flex;flex-direction:column;min-height:0;background:var(--panel-2);flex:1">
      <div style="display:flex;align-items:center;padding:14px 18px;border-bottom:1px solid var(--hair);background:var(--panel)">
        <div style="margin-right:24px"><h1 class="disp" style="font-size:16px;font-weight:600;letter-spacing:-.02em;margin:0">Orchestrator</h1><span style="font-size:10.5px;color:var(--ink-3)">${AGENTS.length} agents across ${PROJECTS.length} projects · ${ORG}</span></div>
        ${stat(count("running"), "Running", "var(--st-running)", true)}${stat(count("blocked"), "Need you", "var(--st-blocked)")}${stat(count("waiting") + count("queued"), "Waiting", "var(--st-waiting)")}${stat(count("done"), "Done", "var(--st-done)")}
        <div style="margin-left:auto;display:flex;align-items:center;gap:8px"><div style="display:flex;gap:2px;padding:3px;background:var(--panel-2);border-radius:var(--r-md);border:1px solid var(--hair)">${VIZ.map(vizBtn).join("")}</div><button class="btn primary">${icon("bolt", 13)}Spawn</button></div>
      </div>
      <div class="scroll-y oc-grid-scroll" style="flex:1"><div style="display:grid;gap:14px;padding:18px;grid-template-columns:repeat(auto-fill, minmax(320px, 1fr));align-content:start">${AGENTS.map(agentCard).join("")}</div></div>
    </div>`;
  }

  // mounts the 1340×812 console into `el` (callers scale it via .rc-inner transform)
  window.renderOrreryConsole = function (el) {
    el.innerHTML = `<div class="oc-app" style="width:1340px;height:812px;display:flex;flex-direction:column;background:var(--panel-2);border-radius:14px;overflow:hidden;border:1px solid var(--hair-2);font-family:var(--font-mono);color:var(--ink)">${topBar()}<div style="display:flex;flex:1;min-height:0">${sidebar()}${overview()}</div></div>`;
  };
})();
