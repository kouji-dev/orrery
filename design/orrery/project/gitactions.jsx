/* global React, Icon, fmtDur */
// Orrery — dual-path git actions (roadmap A4). Native is the primary press and
// always free; the AI variant is always offered too and always states its price
// on the row before it runs. Estimates calibrate against recorded actuals.

const { useState: useStateG, useEffect: useEffectG, useRef: useRefG } = React;

// ---- provider rate table (user-editable in Settings ▸ Cost) ----
window.COST_RATES = window.COST_RATES || {
  "opus-4.6": { in: 15, out: 75 }, "sonnet-4.6": { in: 3, out: 15 }, "haiku-4.6": { in: 0.8, out: 4 },
  "gpt-5.3-codex": { in: 2.5, out: 10 }, "o5-mini": { in: 1.1, out: 4.4 },
  "composer-2": { in: 1.5, out: 6 }, "gemini-3-pro": { in: 2, out: 12 }, "gemini-3-flash": { in: 0.3, out: 1.2 },
  default: { in: 3, out: 15 },
};
// budget guard (A4.4)
window.COST_BUDGET = window.COST_BUDGET || { capUsd: 25, confirmAboveUsd: 1.0, spentUsd: 4.62 };
// actuals ledger — estimator switches to the observed median after 3 runs (A4.3)
window.COST_LEDGER = window.COST_LEDGER || [];

const rateOf = (model) => window.COST_RATES[model] || window.COST_RATES.default;

// per-op heuristic: base tokens + a term for the diff it must read
const OP_SPEC = {
  commit: { lo: 1200, hi: 2600, perKb: 260, label: "Commit" },
  push: { lo: 0, hi: 0, perKb: 0, label: "Push" },
  fetch: { lo: 0, hi: 0, perKb: 0, label: "Fetch" },
  pull: { lo: 0, hi: 0, perKb: 0, label: "Pull" },
  merge: { lo: 4500, hi: 9000, perKb: 420, label: "Merge" },
  rebase: { lo: 6000, hi: 13000, perKb: 520, label: "Rebase" },
  conflict: { lo: 2600, hi: 6200, perKb: 700, label: "Resolve conflict" },
  hunk: { lo: 900, hi: 2100, perKb: 520, label: "Resolve hunk" },
  cherrypick: { lo: 3000, hi: 7000, perKb: 450, label: "Cherry-pick" },
  revert: { lo: 2500, hi: 6000, perKb: 400, label: "Revert" },
  squash: { lo: 2200, hi: 5200, perKb: 300, label: "Squash" },
  branch: { lo: 260, hi: 700, perKb: 0, label: "Name branch" },
};

// estimate({op, files, diffBytes, conflicts, model, verbose}) → cost envelope
function estimateCost({ op, files = 0, diffBytes = 0, conflicts = 0, model = "sonnet-4.6", verbose = false }) {
  const spec = OP_SPEC[op] || OP_SPEC.commit;
  const past = window.COST_LEDGER.filter((e) => e.op === op && e.model === model);
  const kb = diffBytes / 1024;
  let lo = spec.lo + spec.perKb * kb + conflicts * 800 + files * 120;
  let hi = spec.hi + spec.perKb * kb * 1.6 + conflicts * 1500 + files * 260;
  let confidence = "heuristic";
  if (past.length >= 3) {
    const med = past.map((e) => e.tokens).sort((a, b) => a - b)[Math.floor(past.length / 2)];
    lo = med * 0.8; hi = med * 1.25; confidence = "calibrated · " + past.length + " runs";
  }
  if (verbose) { lo *= 1.7; hi *= 1.8; }
  const r = rateOf(model);
  const usd = (tok) => (tok * 0.75 * r.in + tok * 0.25 * r.out) / 1e6;
  return { op, model, tokensLow: Math.round(lo), tokensHigh: Math.round(hi), usdLow: usd(lo), usdHigh: usd(hi), confidence };
}

const fmtTok = (n) => n >= 1000 ? (n / 1000).toFixed(n >= 10000 ? 0 : 1).replace(/\.0$/, "") + "k" : String(Math.round(n));
const fmtUsd = (n) => n >= 1 ? "$" + n.toFixed(2) : n >= 0.01 ? "$" + n.toFixed(2) : "$" + n.toFixed(3);

// deterministic per-agent token ledger (hook events + ccusage sampler, simulated)
function hashId(s) { let h = 0; for (let i = 0; i < s.length; i++) h = (h * 31 + s.charCodeAt(i)) | 0; return Math.abs(h); }
function agentCost(ag) {
  if (!ag) return { tokens: 0, cached: 0, usd: 0, lines: 0, perLine: 0, ctx: 0 };
  const h = hashId(ag.id + ag.name);
  const base = 42000 + (h % 160000);
  const live = (ag.status === "running" ? (ag.elapsed || 0) * 420 : 0);
  const tokens = base + live;
  const cached = Math.round(tokens * (0.32 + (h % 25) / 100));
  const r = rateOf(ag.model);
  const usd = ((tokens - cached) * r.in + cached * r.in * 0.1 + tokens * 0.22 * r.out) / 1e6;
  const lines = (ag.files || []).reduce((s, f) => s + (f.add || 0), 0) + (ag.commits || 0) * 24;
  return { tokens, cached, usd, lines, perLine: lines ? tokens / lines : 0, ctx: Math.min(0.98, ((h % 40) + 22 + (ag.elapsed || 0) / 26) / 100) };
}

function recordActual({ op, model, tokens, usd, estimate, flash }) {
  window.COST_LEDGER.push({ op, model, tokens, usd, at: Date.now() });
  window.COST_BUDGET.spentUsd += usd;
  if (estimate && tokens > estimate.tokensHigh * 1.5 && flash) {
    flash("⚠ " + (OP_SPEC[op] || {}).label + " cost " + fmtTok(tokens) + " tok — " + Math.round(tokens / estimate.tokensHigh * 100 - 100) + "% over estimate");
  }
}

// ------------------------------------------------------------ the split button
// variants: [{ id, label, op, verbose, run }] — each row shows its own estimate
function GitActionButton({ label, icon, kind = "ghost-hair", disabled, title, onNative, variants = [], estimateInput = {}, small, flash, menuAlign = "right" }) {
  const [open, setOpen] = useStateG(false);
  const ref = useRefG(null);
  useEffectG(() => {
    if (!open) return;
    const away = (e) => { if (ref.current && !ref.current.contains(e.target)) setOpen(false); };
    const esc = (e) => { if (e.key === "Escape") setOpen(false); };
    document.addEventListener("mousedown", away); document.addEventListener("keydown", esc);
    return () => { document.removeEventListener("mousedown", away); document.removeEventListener("keydown", esc); };
  }, [open]);
  const budget = window.COST_BUDGET;
  const remaining = Math.max(0, budget.capUsd - budget.spentUsd);
  const pad = small ? "3px 8px" : "5px 10px";
  const fs = small ? 10.5 : "var(--fs-ui)";
  if (!variants.length) {
    return (
      <button className={"btn " + kind} disabled={disabled} title={title || (label + " · native, 0 tokens")} onClick={onNative}
        style={{ padding: pad, fontSize: fs, flex: "none" }}>
        {icon && <Icon name={icon} size="sm" />}{label}
      </button>
    );
  }
  return (
    <div ref={ref} style={{ position: "relative", display: "flex", flex: "none" }}>
      <button className={"btn " + kind} disabled={disabled} title={title || (label + " · native, no tokens")} onClick={onNative}
        style={{ padding: pad, fontSize: fs, borderTopRightRadius: 0, borderBottomRightRadius: 0, borderRight: "none" }}>
        {icon && <Icon name={icon} size="sm" />}{label}
      </button>
      <button className={"btn " + kind} disabled={disabled || !variants.length} title="AI path — shows its price first" onClick={() => setOpen((v) => !v)}
        style={{ padding: small ? "3px 5px" : "5px 6px", borderTopLeftRadius: 0, borderBottomLeftRadius: 0,
          borderLeft: "1px solid " + (kind === "primary" ? "rgba(0,0,0,.25)" : "var(--hair)") }}>
        <Icon name="chevronD" size="sm" style={{ width: 12, height: 12 }} />
      </button>
      {open && (
        <div className="surface rise" style={{ position: "absolute", top: "calc(100% + 5px)", [menuAlign]: 0, zIndex: 40, minWidth: 316, padding: 5, boxShadow: "var(--shadow)", background: "var(--elev)" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "5px 8px 6px" }}>
            <Icon name="spark" size="sm" style={{ color: "var(--accent)" }} />
            <span className="up" style={{ fontSize: 8.5, color: "var(--ink-3)" }}>AI path · spends tokens</span>
            <span className="tnum" style={{ marginLeft: "auto", fontSize: 9.5, color: "var(--ink-4)" }}>{fmtUsd(remaining)} left of {fmtUsd(budget.capUsd)}</span>
          </div>
          {variants.map((v) => {
            const est = estimateCost({ ...estimateInput, op: v.op || estimateInput.op, verbose: v.verbose });
            const overCap = est.usdHigh > remaining;
            return (
              <button key={v.id} className="btn" disabled={overCap} title={overCap ? "would exceed the project budget cap" : est.confidence}
                onClick={() => { setOpen(false); v.run(est); }}
                style={{ width: "100%", justifyContent: "flex-start", padding: "6px 8px", borderRadius: "var(--r-sm)", opacity: overCap ? 0.45 : 1 }}>
                <Icon name={v.icon || "spark"} size="sm" style={{ color: "var(--accent-2)" }} />
                <span style={{ fontSize: 11.5, color: "var(--ink)" }}>{v.label}</span>
                <span className="tnum" style={{ marginLeft: "auto", fontSize: 10, color: "var(--ink-3)", display: "flex", gap: 6, alignItems: "baseline" }}>
                  <span>~{fmtTok(est.tokensHigh)} tok</span>
                  <span style={{ color: "var(--accent-2)" }}>≈{fmtUsd(est.usdHigh)}</span>
                </span>
              </button>
            );
          })}
          <div style={{ padding: "5px 8px 3px", fontSize: 9, color: "var(--ink-4)", borderTop: "1px solid var(--hair)", marginTop: 3 }}>
            native path is instant, deterministic and costs 0 tokens
          </div>
        </div>
      )}
    </div>
  );
}

// run an AI variant: streams into the agent terminal, then records the actual
function runAIVariant({ agent, op, label, est, flash, appendLog, onDone }) {
  const model = agent ? agent.model : "sonnet-4.6";
  flash && flash(label + " · " + agent.name + " — AI path, est " + fmtTok(est.tokensHigh) + " tok");
  appendLog && appendLog(agent.id, [
    { t: "sys", s: "▶ " + label + " (AI path) — estimate " + fmtTok(est.tokensLow) + "–" + fmtTok(est.tokensHigh) + " tok · " + fmtUsd(est.usdHigh) },
    { t: "cmd", s: "git " + op + "  # driven by " + model },
  ]);
  const jitter = 0.7 + Math.random() * 1.1;
  const tokens = Math.round(est.tokensHigh * jitter);
  const r = rateOf(model);
  const usd = (tokens * 0.75 * r.in + tokens * 0.25 * r.out) / 1e6;
  setTimeout(() => {
    appendLog && appendLog(agent.id, [{ t: "ok", s: "✓ " + label + " complete · " + fmtTok(tokens) + " tok · " + fmtUsd(usd) }]);
    recordActual({ op, model, tokens, usd, estimate: est, flash });
    onDone && onDone({ tokens, usd });
  }, 1400);
}

// ---- disclosure surfaces (A4.4) ----
// live per-agent token counter for the status bar
function LiveTokenMeter({ agent }) {
  const [, tick] = useStateG(0);
  useEffectG(() => { const iv = setInterval(() => tick((v) => v + 1), 1200); return () => clearInterval(iv); }, []);
  if (!agent) return null;
  const c = agentCost(agent);
  const live = agent.status === "running";
  return (
    <span title={"tokens for " + agent.name + " · " + Math.round(c.cached / c.tokens * 100) + "% cached"}
      style={{ display: "flex", gap: 5, alignItems: "center", color: live ? "var(--ink-2)" : "inherit" }} className="tnum">
      <Icon name="spark" size="sm" style={{ width: 11, height: 11, color: live ? "var(--accent)" : "var(--ink-4)" }} />
      {fmtTok(c.tokens)} tok
      <span style={{ color: "var(--ink-4)" }}>·</span>
      <span style={{ color: "var(--accent-2)" }}>{fmtUsd(c.usd)}</span>
    </span>
  );
}

// cumulative cost + context fill for an agent card (A4.4 / A5.8)
function AgentCostChip({ agent, showCtx = true }) {
  const c = agentCost(agent);
  const pct = Math.round(c.ctx * 100);
  const warm = pct >= 80 ? "var(--st-blocked)" : pct >= 60 ? "#f5c451" : "var(--accent-2)";
  return (
    <span style={{ display: "flex", alignItems: "center", gap: 7, fontSize: 9.5 }} className="tnum">
      <span title="cumulative tokens · cost" style={{ display: "flex", gap: 4, alignItems: "center", color: "var(--ink-3)" }}>
        <Icon name="spark" size="sm" style={{ width: 10, height: 10, color: "var(--accent)" }} />{fmtTok(c.tokens)}
        <span style={{ color: "var(--accent-2)" }}>{fmtUsd(c.usd)}</span>
      </span>
      {showCtx && (
        <span title={"context window " + pct + "% full"} style={{ display: "flex", gap: 4, alignItems: "center", color: "var(--ink-4)" }}>
          <span style={{ width: 26, height: 4, borderRadius: 3, background: "var(--hair)", overflow: "hidden", display: "block" }}>
            <span style={{ display: "block", height: "100%", width: pct + "%", background: warm, borderRadius: 3 }} />
          </span>
          {pct}%
        </span>
      )}
    </span>
  );
}

Object.assign(window, { estimateCost, GitActionButton, runAIVariant, recordActual, agentCost, LiveTokenMeter, AgentCostChip, fmtTok, fmtUsd, OP_SPEC, rateOf });
