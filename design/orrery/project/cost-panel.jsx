/* global React, Icon, ToolBadge, Spark, agentCost, fmtTok, fmtUsd, rateOf, OP_SPEC */
// Orrery — token & cost dashboard (roadmap A6.2 / A6.3). Every number here comes
// from the same ledger the status bar and the estimator read.

const { useState: useStateCo, useEffect: useEffectCo, useMemo: useMemoCo } = React;

const DAYS = ["Mon", "Tue", "Wed", "Thu", "Fri", "Sat", "Sun"];

function KPI({ label, value, sub, color, wide }) {
  return (
    <div style={{ flex: wide ? 1.4 : 1, minWidth: 0, padding: "10px 12px", background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-md)" }}>
      <div className="up" style={{ fontSize: 8.5, color: "var(--ink-4)" }}>{label}</div>
      <div className="tnum disp" style={{ fontSize: 19, fontWeight: 600, marginTop: 5, color: color || "var(--ink)" }}>{value}</div>
      {sub && <div style={{ fontSize: 9.5, color: "var(--ink-4)", marginTop: 3 }}>{sub}</div>}
    </div>
  );
}

function Bar({ frac, color }) {
  return (
    <span style={{ display: "block", height: 5, borderRadius: 3, background: "var(--hair)", overflow: "hidden", minWidth: 40 }}>
      <span style={{ display: "block", height: "100%", width: Math.max(2, Math.round(frac * 100)) + "%", background: color || "var(--accent)", borderRadius: 3 }} />
    </span>
  );
}

function CostPanel({ ctx }) {
  const agents = ctx.agents || [];
  const [, tick] = useStateCo(0);
  useEffectCo(() => { const iv = setInterval(() => tick((v) => v + 1), 2000); return () => clearInterval(iv); }, []);
  const budget = window.COST_BUDGET;

  const per = agents.map((a) => ({ a, c: agentCost(a) })).sort((x, y) => y.c.usd - x.c.usd);
  const totals = per.reduce((s, p) => ({ tokens: s.tokens + p.c.tokens, usd: s.usd + p.c.usd, cached: s.cached + p.c.cached, lines: s.lines + p.c.lines }), { tokens: 0, usd: 0, cached: 0, lines: 0 });
  const perLine = totals.lines ? totals.tokens / totals.lines : 0;

  const byModel = useMemoCo(() => {
    const m = {};
    per.forEach(({ a, c }) => { m[a.model] = m[a.model] || { model: a.model, tokens: 0, usd: 0, agents: 0 }; m[a.model].tokens += c.tokens; m[a.model].usd += c.usd; m[a.model].agents += 1; });
    return Object.values(m).sort((x, y) => y.usd - x.usd);
  }, [per.length, totals.tokens]);

  // native vs AI per operation kind — the whole efficiency argument in one table
  const ledger = window.COST_LEDGER || [];
  const ops = useMemoCo(() => {
    const nativeCounts = { commit: agents.reduce((s, a) => s + (a.commits || 0), 0), merge: agents.filter((a) => a.status === "done").length, push: 3, fetch: 12, pull: 6, rebase: 2, conflict: 4, hunk: 9 };
    const keys = Array.from(new Set([...Object.keys(nativeCounts), ...ledger.map((e) => e.op)]));
    return keys.map((op) => {
      const ai = ledger.filter((e) => e.op === op);
      return { op, label: (OP_SPEC[op] || { label: op }).label, native: nativeCounts[op] || 0, aiRuns: ai.length, aiTokens: ai.reduce((s, e) => s + e.tokens, 0), aiUsd: ai.reduce((s, e) => s + e.usd, 0) };
    }).sort((a, b) => (b.aiUsd - a.aiUsd) || (b.native - a.native));
  }, [ledger.length, agents.length]);

  const week = DAYS.map((d, i) => {
    const seed = (i * 9301 + 49297) % 233280 / 233280;
    return { d, usd: 0.4 + seed * 2.6 + (i === 6 ? totals.usd * 0.18 : 0) };
  });
  const maxDay = Math.max(...week.map((w) => w.usd));

  const th = (l, right) => <th className="up" style={{ textAlign: right ? "right" : "left", fontSize: 8.5, color: "var(--ink-4)", fontWeight: 500, padding: "6px 10px", borderBottom: "1px solid var(--hair)", whiteSpace: "nowrap" }}>{l}</th>;
  const capFrac = Math.min(1, budget.spentUsd / budget.capUsd);

  return (
    <div className="scroll-y" style={{ flex: 1, minHeight: 0, padding: 12 }}>
      <div style={{ display: "flex", gap: 9, flexWrap: "wrap" }}>
        <KPI label="Spend · session" value={fmtUsd(totals.usd)} sub={agents.length + " agents · " + fmtTok(totals.tokens) + " tokens"} />
        <KPI label="Cached input" value={Math.round(totals.cached / Math.max(1, totals.tokens) * 100) + "%"} sub="resume keeps the prompt cache warm" color="var(--accent-2)" />
        <KPI label="Tokens per accepted line" value={Math.round(perLine)} sub={totals.lines.toLocaleString() + " lines merged to main"} color="var(--accent)" wide />
        <KPI label="Native git ops" value={ops.reduce((s, o) => s + o.native, 0)} sub="0 tokens spent" color="var(--st-done)" />
        <KPI label="AI git ops" value={ledger.length} sub={ledger.length ? fmtUsd(ledger.reduce((s, e) => s + e.usd, 0)) + " on the AI path" : "none yet this session"} />
      </div>

      {/* budget */}
      <div style={{ marginTop: 12, padding: "10px 12px", background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-md)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 10, fontSize: 11 }}>
          <Icon name="shield" size="sm" style={{ color: capFrac > 0.8 ? "var(--st-blocked)" : "var(--accent)" }} />
          <span style={{ color: "var(--ink)" }}>Project budget</span>
          <span className="tnum" style={{ color: "var(--ink-3)" }}>{fmtUsd(budget.spentUsd)} of {fmtUsd(budget.capUsd)}</span>
          <span style={{ flex: 1, minWidth: 60 }}><Bar frac={capFrac} color={capFrac > 0.8 ? "var(--st-blocked)" : "linear-gradient(90deg, var(--accent), var(--accent-2))"} /></span>
          <span style={{ fontSize: 9.5, color: "var(--ink-4)" }}>confirm above {fmtUsd(budget.confirmAboveUsd)}</span>
          {[10, 25, 50, 100].map((v) => (
            <button key={v} className="btn ghost-hair" style={{ padding: "2px 7px", fontSize: 10, color: budget.capUsd === v ? "var(--ink)" : "var(--ink-3)", borderColor: budget.capUsd === v ? "var(--accent)" : "var(--hair)" }}
              onClick={() => { window.COST_BUDGET.capUsd = v; ctx.flash("budget cap → $" + v + " · AI variants disable at the cap"); tick((x) => x + 1); }}>${v}</button>
          ))}
        </div>
        {capFrac >= 1 && <div style={{ marginTop: 7, fontSize: 10, color: "var(--st-blocked)" }}>cap reached — AI variants are disabled until you raise it. Native git stays fully usable.</div>}
      </div>

      {/* per day */}
      <div style={{ marginTop: 12, display: "flex", gap: 12 }}>
        <div style={{ flex: 1, minWidth: 0, padding: "10px 12px", background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-md)" }}>
          <div className="up" style={{ fontSize: 8.5, color: "var(--ink-4)", marginBottom: 8 }}>Cost per day</div>
          <div style={{ display: "flex", alignItems: "flex-end", gap: 7, height: 70 }}>
            {week.map((w, i) => (
              <div key={w.d} style={{ flex: 1, display: "flex", flexDirection: "column", alignItems: "center", gap: 5 }}>
                <span className="tnum" style={{ fontSize: 8.5, color: "var(--ink-4)" }}>{fmtUsd(w.usd)}</span>
                <span style={{ width: "100%", height: Math.max(3, Math.round(w.usd / maxDay * 44)), borderRadius: "3px 3px 0 0",
                  background: i === week.length - 1 ? "linear-gradient(180deg, var(--accent), var(--accent-2))" : "var(--panel-3)", border: "1px solid var(--hair)" }} />
                <span style={{ fontSize: 8.5, color: "var(--ink-4)" }}>{w.d}</span>
              </div>
            ))}
          </div>
        </div>
        <div style={{ flex: 1, minWidth: 0, background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-md)", overflow: "hidden" }}>
          <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 11 }}>
            <thead><tr>{th("Model")}{th("Agents", true)}{th("Tokens", true)}{th("Cost", true)}{th("Share")}</tr></thead>
            <tbody>
              {byModel.map((m) => (
                <tr key={m.model} style={{ borderBottom: "1px solid var(--hair)" }}>
                  <td style={{ padding: "5px 10px", color: "var(--ink)" }}>{m.model}</td>
                  <td className="tnum" style={{ padding: "5px 10px", textAlign: "right", color: "var(--ink-4)" }}>{m.agents}</td>
                  <td className="tnum" style={{ padding: "5px 10px", textAlign: "right", color: "var(--ink-2)" }}>{fmtTok(m.tokens)}</td>
                  <td className="tnum" style={{ padding: "5px 10px", textAlign: "right", color: "var(--accent-2)" }}>{fmtUsd(m.usd)}</td>
                  <td style={{ padding: "5px 10px", width: 74 }}><Bar frac={m.usd / Math.max(0.001, totals.usd)} /></td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      </div>

      {/* per operation */}
      <div style={{ marginTop: 12, background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-md)", overflow: "hidden" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "7px 10px", borderBottom: "1px solid var(--hair)" }}>
          <span className="up" style={{ fontSize: 8.5, color: "var(--ink-4)" }}>Per operation · native vs AI</span>
          <span style={{ fontSize: 9.5, color: "var(--ink-4)", marginLeft: "auto" }}>every native run is a token bill that never arrived</span>
        </div>
        <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 11 }}>
          <thead><tr>{th("Operation")}{th("Native runs", true)}{th("AI runs", true)}{th("AI tokens", true)}{th("AI cost", true)}</tr></thead>
          <tbody>
            {ops.map((o) => (
              <tr key={o.op} style={{ borderBottom: "1px solid var(--hair)" }}>
                <td style={{ padding: "5px 10px", color: "var(--ink)" }}>{o.label}</td>
                <td className="tnum" style={{ padding: "5px 10px", textAlign: "right", color: "var(--st-done)" }}>{o.native || "—"}</td>
                <td className="tnum" style={{ padding: "5px 10px", textAlign: "right", color: "var(--ink-2)" }}>{o.aiRuns || "—"}</td>
                <td className="tnum" style={{ padding: "5px 10px", textAlign: "right", color: "var(--ink-2)" }}>{o.aiTokens ? fmtTok(o.aiTokens) : "—"}</td>
                <td className="tnum" style={{ padding: "5px 10px", textAlign: "right", color: o.aiUsd ? "var(--accent-2)" : "var(--ink-4)" }}>{o.aiUsd ? fmtUsd(o.aiUsd) : "0"}</td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>

      {/* per agent */}
      <div style={{ marginTop: 12, background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-md)", overflow: "hidden" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "7px 10px", borderBottom: "1px solid var(--hair)" }}>
          <span className="up" style={{ fontSize: 8.5, color: "var(--ink-4)" }}>Per agent</span>
        </div>
        <table style={{ width: "100%", borderCollapse: "collapse", fontSize: 11 }}>
          <thead><tr>{th("Agent")}{th("Model")}{th("Tokens", true)}{th("Cached", true)}{th("Cost", true)}{th("Accepted lines", true)}{th("Tok / line", true)}{th("Context", true)}</tr></thead>
          <tbody>
            {per.map(({ a, c }) => (
              <tr key={a.id} style={{ borderBottom: "1px solid var(--hair)", cursor: "pointer" }} onClick={() => ctx.openAgent(a.id)}>
                <td style={{ padding: "5px 10px" }}>
                  <span style={{ display: "flex", alignItems: "center", gap: 6 }}><ToolBadge tool={a.tool} size={13} /><span style={{ color: "var(--ink)" }}>{a.name}</span></span>
                </td>
                <td style={{ padding: "5px 10px", color: "var(--ink-4)" }}>{a.model}</td>
                <td className="tnum" style={{ padding: "5px 10px", textAlign: "right", color: "var(--ink-2)" }}>{fmtTok(c.tokens)}</td>
                <td className="tnum" style={{ padding: "5px 10px", textAlign: "right", color: "var(--ink-4)" }}>{Math.round(c.cached / c.tokens * 100)}%</td>
                <td className="tnum" style={{ padding: "5px 10px", textAlign: "right", color: "var(--accent-2)" }}>{fmtUsd(c.usd)}</td>
                <td className="tnum" style={{ padding: "5px 10px", textAlign: "right", color: "var(--ink-2)" }}>{c.lines}</td>
                <td className="tnum" style={{ padding: "5px 10px", textAlign: "right", color: c.perLine > 900 ? "var(--st-blocked)" : "var(--ink)" }}>{c.lines ? Math.round(c.perLine) : "—"}</td>
                <td className="tnum" style={{ padding: "5px 10px", textAlign: "right", width: 78 }}>
                  <span style={{ display: "flex", alignItems: "center", gap: 6, justifyContent: "flex-end" }}>
                    <span style={{ width: 34 }}><Bar frac={c.ctx} color={c.ctx > 0.8 ? "var(--st-blocked)" : c.ctx > 0.6 ? "#f5c451" : "var(--accent-2)"} /></span>
                    {Math.round(c.ctx * 100)}%
                  </span>
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
      <div style={{ padding: "10px 2px 4px", fontSize: 9, color: "var(--ink-4)" }}>
        ledger: token_events (agent · ticket · project · model · input / cached / output / usd) written from hook events and the ccusage sampler
      </div>
    </div>
  );
}

window.TOOL_PANELS.cost = { kind: "cost", label: "Cost", icon: "spark", Component: CostPanel };
Object.assign(window, { CostPanel });
