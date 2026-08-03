/* global React, Icon, ToolBadge, fmtDur */
// Orrery — Performance tool window: recursive process tree (A7.7), emit telemetry
// inventory + raw trace (A0.7 phase 1), and the live acceptance budgets (A7.2).

const { useState: useStateP, useEffect: useEffectP, useRef: useRefP, useMemo: useMemoP } = React;

const pHash = (s) => { let h = 0; for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) | 0; return Math.abs(h); };
const jit = (v, p) => Math.max(0.1, v * (1 + (Math.random() * 2 - 1) * p));
const mb = (v) => v >= 1024 ? (v / 1024).toFixed(2) + " GB" : Math.round(v) + " MB";

const NODE_NOTE = {
  core: "rust core · our code",
  webview: "webview2 · not our code",
  gpu: "gpu process · not our code",
  renderer: "renderer · not our code",
  utility: "utility · not our code",
  conhost: "conhost · ConPTY host",
  shim: "pwsh · shim wrapper (A0.1)",
  agent: "agent CLI",
  spawned: "started by the agent",
};

// ---- process tree: known pids + their discovered descendants ----
function buildProcTree(agents) {
  const node = (o) => ({ children: [], ...o });
  const webview = node({ id: "wv", name: "msedgewebview2.exe", kind: "webview", pid: 6120, cpu: 6.2, priv: 54, rss: 138, children: [
    node({ id: "wv-gpu", name: "msedgewebview2.exe --type=gpu-process", kind: "gpu", pid: 6180, cpu: 2.1, priv: 32, rss: 96 }),
    node({ id: "wv-r", name: "msedgewebview2.exe --type=renderer", kind: "renderer", pid: 6212, cpu: 8.4, priv: 44, rss: 152 }),
    node({ id: "wv-u", name: "msedgewebview2.exe --type=utility", kind: "utility", pid: 6244, cpu: 0.4, priv: 14, rss: 42 }),
  ] });
  const agentNodes = agents.filter((a) => ["running", "waiting", "queued", "blocked"].includes(a.status)).map((a) => {
    const h = pHash(a.id + a.name);
    const shim = h % 3 === 0;
    const cli = node({ id: "cli-" + a.id, name: (a.tool === "claude" ? "node claude/cli.js" : a.tool) + " --model " + a.model, kind: "agent",
      pid: 9000 + (h % 900), cpu: a.status === "running" ? 22 + (h % 40) : 1.4, priv: 56 + (h % 88), rss: 120 + (h % 180), agentId: a.id, tool: a.tool });
    // descendants the agent started and may not clean up
    if (/node|22/.test(a.name)) cli.children.push(node({ id: "dev-" + a.id, name: "next dev", kind: "spawned", pid: 11400 + (h % 90), cpu: 12.5, priv: 842, rss: 1180, ago: "40 minutes ago" }));
    if (h % 4 === 1) cli.children.push(node({ id: "vt-" + a.id, name: "vitest --watch", kind: "spawned", pid: 11600 + (h % 90), cpu: 4.2, priv: 186, rss: 302 }));
    if (h % 5 === 2) cli.children.push(node({ id: "pw-" + a.id, name: "playwright chromium", kind: "spawned", pid: 11800 + (h % 90), cpu: 1.1, priv: 262, rss: 480 }));
    const host = node({ id: "ch-" + a.id, name: "conhost.exe", kind: "conhost", pid: 7000 + (h % 800), cpu: 0.6, priv: 5 + (h % 4), rss: 11 + (h % 7), children: [cli] });
    if (shim) return node({ id: "sh-" + a.id, name: "pwsh -NoProfile -File " + a.tool + ".ps1", kind: "shim", pid: 6800 + (h % 400), cpu: 0.3, priv: 61 + (h % 34), rss: 94 + (h % 38), agentId: a.id, children: [host] });
    return host;
  });
  return [node({ id: "core", name: "orrery.exe", kind: "core", pid: 4820, cpu: 3.1, priv: 62, rss: 148, children: [webview, ...agentNodes] })];
}

const sumTree = (n, key) => n[key] + n.children.reduce((s, c) => s + sumTree(c, key), 0);
const walk = (nodes, fn, depth = 0) => nodes.forEach((n) => { fn(n, depth); walk(n.children, fn, depth + 1); });

function ProcessTree({ ctx }) {
  const [tree, setTree] = useStateP(() => buildProcTree(ctx.agents || []));
  const [open, setOpen] = useStateP({ core: true, wv: false });
  const [sel, setSel] = useStateP(null);
  const [killed, setKilled] = useStateP({});
  // sampler runs only while this panel is mounted (A1.7 pattern)
  useEffectP(() => {
    const iv = setInterval(() => setTree((t) => {
      const copy = JSON.parse(JSON.stringify(t));
      walk(copy, (n) => { n.cpu = Math.min(96, jit(n.cpu, 0.22)); n.priv = jit(n.priv, 0.015); n.rss = jit(n.rss, 0.015); });
      return copy;
    }), 1400);
    return () => clearInterval(iv);
  }, []);
  useEffectP(() => { setTree(buildProcTree(ctx.agents || [])); }, [(ctx.agents || []).length]);

  const rows = [];
  walk(tree, (n, d) => {
    if (killed[n.id]) return;
    rows.push({ n, d });
  });
  // hide children of collapsed nodes
  const visible = [];
  let skipDepth = null;
  rows.forEach((r) => {
    if (skipDepth != null && r.d > skipDepth) return;
    skipDepth = null;
    visible.push(r);
    if (r.n.children.length && !open[r.n.id]) skipDepth = r.d;
  });
  const totalPriv = tree.reduce((s, n) => s + sumTree(n, "priv"), 0);
  const ourPriv = tree[0] ? tree[0].priv : 0;
  const agentPriv = totalPriv - ourPriv - (function () { const wv = tree[0] && tree[0].children.find((c) => c.kind === "webview"); return wv ? sumTree(wv, "priv") : 0; })();
  // threshold alert on any runaway subtree
  const alerts = [];
  walk(tree, (n) => { if (n.kind === "agent" && !killed[n.id]) { const t = sumTree(n, "priv"); if (t > 700) alerts.push({ n, t }); } });

  const kill = (n) => { setKilled((k) => ({ ...k, [n.id]: true })); ctx.flash("killed subtree · " + n.name + " (" + mb(sumTree(n, "priv")) + " reclaimed)"); };

  return (
    <div style={{ display: "flex", flexDirection: "column", minHeight: 0, flex: 1 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "8px 12px", borderBottom: "1px solid var(--hair)", flex: "none", fontSize: 10.5 }}>
        <span className="tnum" style={{ color: "var(--ink-2)" }}>Orrery <b style={{ color: "var(--ink)" }}>{mb(ourPriv + (tree[0] ? sumTree(tree[0].children.find((c) => c.kind === "webview") || { priv: 0, children: [] }, "priv") : 0))}</b></span>
        <span style={{ color: "var(--ink-4)" }}>·</span>
        <span className="tnum" style={{ color: "var(--ink-2)" }}>agents <b style={{ color: "var(--accent-2)" }}>{mb(agentPriv)}</b></span>
        <span className="chip" style={{ fontSize: 9 }}>private working set · RSS is secondary</span>
        <span style={{ marginLeft: "auto", fontSize: 9.5, color: "var(--ink-4)" }}>job-object accounting · descendants rediscovered every 5s</span>
      </div>
      {alerts.map(({ n, t }) => (
        <div key={n.id} style={{ display: "flex", alignItems: "center", gap: 9, padding: "7px 12px", flex: "none",
          background: "color-mix(in oklch, var(--st-blocked), transparent 91%)", borderBottom: "1px solid color-mix(in oklch, var(--st-blocked), transparent 62%)" }}>
          <Icon name="flag" size="sm" style={{ color: "var(--st-blocked)" }} />
          <span style={{ fontSize: 11, color: "var(--ink)" }}>
            agent subtree <b>{mb(t)}</b> — {mb(n.children.reduce((s, c) => s + sumTree(c, "priv"), 0))} of it is {n.children.map((c) => c.name).join(", ")} it started{n.children[0] && n.children[0].ago ? " " + n.children[0].ago : ""}
          </span>
          <button className="btn ghost-hair" style={{ marginLeft: "auto", padding: "2px 8px", fontSize: 10, color: "var(--st-blocked)" }}
            onClick={() => n.children.forEach(kill)}><Icon name="stop" size="sm" />Kill descendants</button>
        </div>
      ))}
      <div className="scroll-y" style={{ flex: 1, minHeight: 0 }}>
        <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 11 }}>
          <thead>
            <tr style={{ position: "sticky", top: 0, background: "var(--panel-2)", zIndex: 1 }}>
              {["Process", "PID", "CPU", "Private", "RSS", "Subtree", ""].map((h, i) => (
                <th key={h + i} className="up" style={{ textAlign: i > 1 ? "right" : "left", fontSize: 8.5, color: "var(--ink-4)", fontWeight: 500, padding: "6px 10px", borderBottom: "1px solid var(--hair)", whiteSpace: "nowrap" }}>{h}</th>
              ))}
            </tr>
          </thead>
          <tbody>
            {visible.map(({ n, d }) => {
              const kids = n.children.filter((c) => !killed[c.id]).length;
              const sub = sumTree(n, "priv");
              const on = sel === n.id;
              const notOurs = /not our code/.test(NODE_NOTE[n.kind] || "");
              return (
                <tr key={n.id} onClick={() => setSel(n.id)}
                  style={{ background: on ? "var(--panel-3)" : "transparent", cursor: "pointer", borderBottom: "1px solid var(--hair)" }}>
                  <td style={{ padding: "5px 10px", whiteSpace: "nowrap" }}>
                    <span style={{ display: "flex", alignItems: "center", gap: 6, paddingLeft: d * 15 }}>
                      {kids > 0
                        ? <button className="pane-btn" onClick={(e) => { e.stopPropagation(); setOpen((o) => ({ ...o, [n.id]: !o[n.id] })); }} style={{ padding: 0 }}>
                            <Icon name={open[n.id] ? "chevronD" : "chevron"} size="sm" style={{ width: 12, height: 12 }} />
                          </button>
                        : <span style={{ width: 12 }} />}
                      {n.tool ? <ToolBadge tool={n.tool} size={13} /> : <span style={{ width: 7, height: 7, borderRadius: 2, background: notOurs ? "var(--ink-4)" : n.kind === "spawned" ? "#f5c451" : n.kind === "shim" ? "var(--st-blocked)" : "var(--accent)" }} />}
                      <span style={{ color: notOurs ? "var(--ink-3)" : "var(--ink)" }}>{n.name}</span>
                      <span style={{ fontSize: 9, color: "var(--ink-4)" }}>{NODE_NOTE[n.kind]}</span>
                    </span>
                  </td>
                  <td className="tnum" style={{ padding: "5px 10px", color: "var(--ink-4)" }}>{n.pid}</td>
                  <td className="tnum" style={{ padding: "5px 10px", textAlign: "right", color: n.cpu > 60 ? "var(--st-blocked)" : n.cpu > 25 ? "#f5c451" : "var(--ink-2)" }}>{n.cpu.toFixed(1)}%</td>
                  <td className="tnum" style={{ padding: "5px 10px", textAlign: "right", color: n.priv > 1000 ? "var(--st-blocked)" : "var(--ink)" }}>{mb(n.priv)}</td>
                  <td className="tnum" style={{ padding: "5px 10px", textAlign: "right", color: "var(--ink-4)" }}>{mb(n.rss)}</td>
                  <td className="tnum" style={{ padding: "5px 10px", textAlign: "right", color: kids ? "var(--accent-2)" : "var(--ink-4)" }}>{kids ? mb(sub) : "—"}</td>
                  <td style={{ padding: "3px 8px", textAlign: "right" }}>
                    {(n.kind === "spawned" || n.kind === "agent") && (
                      <button className="btn" onClick={(e) => { e.stopPropagation(); kill(n); }} title="Kill this process and its subtree"
                        style={{ padding: "1px 6px", fontSize: 9.5, color: "var(--st-blocked)" }}><Icon name="stop" size="sm" style={{ width: 10, height: 10 }} />Kill</button>
                    )}
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      <div style={{ padding: "5px 12px", borderTop: "1px solid var(--hair)", flex: "none", fontSize: 9, color: "var(--ink-4)" }}>
        subtree totals sum private working set — RSS double-counts shared pages and would overstate
      </div>
    </div>
  );
}

// ------------------------------------------------- emit telemetry (A0.7 p1)
const EMIT_SEED = [
  { name: "agent://output", cls: "bulk", calls: 3712, bytes: 41_800_000, p50: 1180, p95: 14_600, rate: 62 },
  { name: "agent://status", cls: "coalesced", calls: 1044, bytes: 96_000, p50: 88, p95: 140, rate: 12 },
  { name: "git://scan", cls: "coalesced", calls: 412, bytes: 1_240_000, p50: 2600, p95: 9800, rate: 4 },
  { name: "perf://stats", cls: "coalesced", calls: 310, bytes: 620_000, p50: 1900, p95: 2400, rate: 2 },
  { name: "agent://permission", cls: "bypass", calls: 26, bytes: 8_100, p50: 300, p95: 420, rate: 0.3 },
  { name: "agent://exit", cls: "bypass", calls: 9, bytes: 1_100, p50: 120, p95: 160, rate: 0.1 },
  { name: "watch://event", cls: "bulk", calls: 2280, bytes: 3_900_000, p50: 640, p95: 4200, rate: 28 },
  { name: "cost://usage", cls: "coalesced", calls: 188, bytes: 210_000, p50: 1100, p95: 1600, rate: 1.6 },
  { name: "search://hit", cls: "bulk", calls: 940, bytes: 2_100_000, p50: 1600, p95: 6400, rate: 40 },
];
const CLS_META = { bypass: { label: "bypass", color: "var(--st-done)" }, coalesced: { label: "coalesced state", color: "var(--accent-2)" }, bulk: { label: "bulk weighted", color: "var(--accent)" } };
const kb = (n) => n >= 1e6 ? (n / 1e6).toFixed(1) + " MB" : n >= 1e3 ? Math.round(n / 1e3) + " KB" : n + " B";

function EmitTelemetry({ ctx }) {
  const [rows, setRows] = useStateP(EMIT_SEED);
  const [trace, setTrace] = useStateP(false);
  const [traceMs, setTraceMs] = useStateP(0);
  const [sort, setSort] = useStateP("bytes");
  useEffectP(() => {
    const iv = setInterval(() => setRows((r) => r.map((x) => ({ ...x, calls: x.calls + Math.round(x.rate * 1.4 * Math.random()), bytes: x.bytes + Math.round(x.rate * x.p50 * 1.4) }))), 1400);
    return () => clearInterval(iv);
  }, []);
  useEffectP(() => {
    if (!trace) return;
    const iv = setInterval(() => setTraceMs((m) => {
      const nx = m + 1000;
      if (nx >= 30 * 60 * 1000) { setTrace(false); ctx.flash("raw trace auto-disabled after 30 minutes"); return 0; }
      return nx;
    }), 1000);
    return () => clearInterval(iv);
  }, [trace]);
  const sorted = [...rows].sort((a, b) => (b[sort] || 0) - (a[sort] || 0));
  const total = rows.reduce((s, r) => s + r.bytes, 0);
  const th = (label, key, right) => (
    <th onClick={() => key && setSort(key)} className="up"
      style={{ textAlign: right ? "right" : "left", fontSize: 8.5, color: sort === key ? "var(--accent)" : "var(--ink-4)", fontWeight: 500, padding: "6px 10px", borderBottom: "1px solid var(--hair)", cursor: key ? "pointer" : "default", whiteSpace: "nowrap" }}>{label}</th>
  );
  return (
    <div style={{ display: "flex", flexDirection: "column", minHeight: 0, flex: 1 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "8px 12px", borderBottom: "1px solid var(--hair)", flex: "none", fontSize: 10.5 }}>
        <span className="tnum" style={{ color: "var(--ink-2)" }}>{rows.length} event names · {kb(total)} emitted this session</span>
        <span className="chip" style={{ fontSize: 9, color: "var(--st-done)" }}>aggregate always on · ≤1µs/emit</span>
        <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 8 }}>
          {trace && (
            <span className="tnum" style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 10, color: "var(--st-blocked)" }}>
              <span className="dot blocked" style={{ background: "var(--st-blocked)" }} />recording · {fmtDur(Math.round(traceMs / 1000))} / 30m
            </span>
          )}
          <button className="btn ghost-hair" onClick={() => { setTrace((v) => !v); setTraceMs(0); ctx.flash(trace ? "raw emit trace stopped" : "raw emit trace on · auto-off after 30m or 200MB"); }}
            style={{ padding: "3px 9px", fontSize: 10.5, color: trace ? "var(--st-blocked)" : "var(--ink-3)",
              borderColor: trace ? "color-mix(in oklch, var(--st-blocked), transparent 55%)" : "var(--hair)" }}>
            <Icon name={trace ? "stop" : "play"} size="sm" />{trace ? "Stop raw trace" : "Start raw trace"}
          </button>
        </div>
      </div>
      <div className="scroll-y" style={{ flex: 1, minHeight: 0 }}>
        <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 11 }}>
          <thead><tr style={{ position: "sticky", top: 0, background: "var(--panel-2)", zIndex: 1 }}>
            {th("Event name", "name")}{th("Calls", "calls", true)}{th("Total bytes", "bytes", true)}{th("p50", "p50", true)}{th("p95", "p95", true)}{th("Peak/s", "rate", true)}{th("Suggested class")}
          </tr></thead>
          <tbody>
            {sorted.map((r) => {
              const m = CLS_META[r.cls];
              return (
                <tr key={r.name} style={{ borderBottom: "1px solid var(--hair)" }}>
                  <td style={{ padding: "5px 10px", color: "var(--ink)" }}>{r.name}</td>
                  <td className="tnum" style={{ padding: "5px 10px", textAlign: "right", color: "var(--ink-2)" }}>{r.calls.toLocaleString()}</td>
                  <td className="tnum" style={{ padding: "5px 10px", textAlign: "right", color: r.bytes > 1e7 ? "var(--st-blocked)" : "var(--ink-2)" }}>{kb(r.bytes)}</td>
                  <td className="tnum" style={{ padding: "5px 10px", textAlign: "right", color: "var(--ink-4)" }}>{r.p50} B</td>
                  <td className="tnum" style={{ padding: "5px 10px", textAlign: "right", color: "var(--ink-4)" }}>{kb(r.p95)}</td>
                  <td className="tnum" style={{ padding: "5px 10px", textAlign: "right", color: "var(--ink-2)" }}>{r.rate}</td>
                  <td style={{ padding: "5px 10px" }}>
                    <span className="chip" style={{ fontSize: 9, padding: "1px 6px", color: m.color, borderColor: "color-mix(in oklch, " + m.color + ", transparent 60%)" }}>{m.label}</span>
                  </td>
                </tr>
              );
            })}
          </tbody>
        </table>
      </div>
      <div style={{ display: "flex", gap: 12, padding: "6px 12px", borderTop: "1px solid var(--hair)", flex: "none", fontSize: 9, color: "var(--ink-4)" }}>
        <span>%APPDATA%/orrery/telemetry/emit-summary-2026-08-03.json</span>
        <span style={{ color: "var(--st-done)" }}>names · keys · byte counts only — payload contents are never written</span>
        <span style={{ marginLeft: "auto" }}>≤50 MB / 7 days, daily rotation</span>
      </div>
    </div>
  );
}

// ------------------------------------------------------ live budgets (A7.2)
function BudgetsPanel({ ctx }) {
  const agents = ctx.agents || [];
  const running = agents.filter((a) => a.status === "running").length;
  const slept = agents.filter((a) => ctx.isAsleep(a.id)).length;
  const rows = [
    { k: "Cold start to interactive", budget: "≤ 300ms", now: "94ms", ok: true },
    { k: "Focused-terminal echo, p50", budget: "≤ 50ms", now: "31ms", ok: true },
    { k: "Focused-terminal echo, p95", budget: "≤ 75ms", now: "68ms", ok: true },
    { k: "agent_input round-trip p95", budget: "≤ 25ms", now: "19ms", ok: true },
    { k: "PTY-origin UI jobs", budget: "≤ 75/s, flat 1→10 agents", now: "58/s", ok: true },
    { k: "Hidden terminal queue peak", budget: "≤ 2MB", now: "740 KB", ok: true },
    { k: "In-flight bytes per agent", budget: "≤ 512KB", now: "212 KB", ok: true },
    { k: "RSS, " + running + " active agents", budget: running > 4 ? "≤ 600MB" : "≤ 250MB", now: (210 + running * 96) + " MB", ok: 210 + running * 96 < (running > 4 ? 600 : 480) },
    { k: "Slept agents", budget: "auto after 30m idle", now: slept + " of " + agents.length, ok: true },
    { k: "Watchers", budget: "≤ 1 per open project", now: (ctx.projects || []).length + " watchers", ok: true },
    { k: "Wrapper processes per agent", budget: "0 (conhost only)", now: "1 pwsh detected", ok: false },
    { k: "Raw app.emit outside funnel", budget: "0 (clippy-enforced)", now: "0", ok: true },
    { k: "Payload bytes in telemetry", budget: "0, ever", now: "0", ok: true },
    { k: "Native git op tokens", budget: "0", now: "0", ok: true },
  ];
  return (
    <div className="scroll-y" style={{ flex: 1, minHeight: 0 }}>
      <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 11 }}>
        <thead><tr style={{ position: "sticky", top: 0, background: "var(--panel-2)" }}>
          {["Metric", "Budget", "Now", ""].map((h, i) => <th key={h + i} className="up" style={{ textAlign: i === 0 ? "left" : "right", fontSize: 8.5, color: "var(--ink-4)", fontWeight: 500, padding: "6px 12px", borderBottom: "1px solid var(--hair)" }}>{h}</th>)}
        </tr></thead>
        <tbody>
          {rows.map((r) => (
            <tr key={r.k} style={{ borderBottom: "1px solid var(--hair)" }}>
              <td style={{ padding: "5px 12px", color: "var(--ink)" }}>{r.k}</td>
              <td className="tnum" style={{ padding: "5px 12px", textAlign: "right", color: "var(--ink-4)" }}>{r.budget}</td>
              <td className="tnum" style={{ padding: "5px 12px", textAlign: "right", color: r.ok ? "var(--ink)" : "var(--st-blocked)" }}>{r.now}</td>
              <td style={{ padding: "5px 12px", textAlign: "right", width: 30 }}>
                <Icon name={r.ok ? "check" : "flag"} size="sm" style={{ color: r.ok ? "var(--st-done)" : "var(--st-blocked)" }} />
              </td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

// -------------------------------------------------- interest subscription view
function SubscriptionStrip({ ctx }) {
  const visible = new Set(ctx.visibleAgentIds || []);
  const scope = ctx.scopeAgent && ctx.scopeAgent.id;
  const modeOf = (a) => a.id === scope ? "stream" : visible.has(a.id) ? "digest" : "none";
  const color = { stream: "var(--accent)", digest: "var(--accent-2)", none: "var(--ink-4)" };
  const counts = { stream: 0, digest: 0, none: 0 };
  (ctx.agents || []).forEach((a) => { counts[modeOf(a)] += 1; });
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 12px", borderBottom: "1px solid var(--hair)", flex: "none", overflowX: "auto" }}>
      <span className="up" style={{ fontSize: 8.5, color: "var(--ink-4)", flex: "none" }}>interest set</span>
      {(ctx.agents || []).map((a) => {
        const m = modeOf(a);
        return (
          <span key={a.id} className="chip" title={a.name + " · " + m} style={{ fontSize: 9, padding: "1px 6px", flex: "none", color: color[m], borderColor: "color-mix(in oklch, " + color[m] + ", transparent 65%)" }}>
            {a.name}<span style={{ color: "var(--ink-4)" }}>{m}</span>
          </span>
        );
      })}
      <span className="tnum" style={{ marginLeft: "auto", fontSize: 9, color: "var(--ink-4)", flex: "none" }}>{counts.stream} stream · {counts.digest} digest · {counts.none} none → 0 bytes</span>
    </div>
  );
}

const PERF_TABS = [
  { k: "proc", label: "Processes", icon: "cpu" },
  { k: "emits", label: "Emits", icon: "link" },
  { k: "budgets", label: "Budgets", icon: "flag" },
];

function PerfPanel({ ctx }) {
  const [tab, setTab] = useStateP("proc");
  return (
    <div style={{ display: "flex", flexDirection: "column", minHeight: 0, flex: 1 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 2, padding: "5px 10px", borderBottom: "1px solid var(--hair)", flex: "none" }}>
        {PERF_TABS.map((t) => (
          <button key={t.k} className="btn" onClick={() => setTab(t.k)}
            style={{ padding: "3px 9px", fontSize: 10.5, borderRadius: "var(--r-sm)", color: tab === t.k ? "var(--ink)" : "var(--ink-3)",
              background: tab === t.k ? "var(--panel-3)" : "transparent", boxShadow: tab === t.k ? "0 0 0 1px var(--hair-2)" : "none" }}>
            <Icon name={t.icon} size="sm" style={{ color: tab === t.k ? "var(--accent)" : "inherit" }} />{t.label}
          </button>
        ))}
      </div>
      {tab === "proc" && <SubscriptionStrip ctx={ctx} />}
      {tab === "proc" && <ProcessTree ctx={ctx} />}
      {tab === "emits" && <EmitTelemetry ctx={ctx} />}
      {tab === "budgets" && <BudgetsPanel ctx={ctx} />}
    </div>
  );
}

// status-bar split (A0.6): our memory vs the agents' — the tree is its drill-down
function memorySplit(agents, isAsleep) {
  const tree = buildProcTree(agents || []);
  const core = tree[0];
  const wv = core.children.find((c) => c.kind === "webview");
  const slept = (agents || []).filter((a) => isAsleep && isAsleep(a.id)).length;
  const own = Math.max(90, core.priv + (wv ? sumTree(wv, "priv") : 0) - slept * 6);
  let ag = 0, agCpu = 0;
  core.children.forEach((c) => { if (c.kind !== "webview") { ag += sumTree(c, "priv"); agCpu += sumTree(c, "cpu"); } });
  const ownCpu = core.cpu + (wv ? sumTree(wv, "cpu") : 0);
  const cores = 10;
  return { own, agents: ag, slept, cpu: Math.min(100, Math.round((ownCpu + agCpu) / cores)), ownCpu: Math.round(ownCpu / cores) };
}

// Performance lives in the anchored dev panel only (not the bottom tool window)
Object.assign(window, { PerfPanel, ProcessTree, EmitTelemetry, BudgetsPanel, SubscriptionStrip, buildProcTree, memorySplit, procMb: mb });
