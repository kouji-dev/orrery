/* global React, Icon, fileName, fileDir, langOf */
// Orrery — Find in Files / Replace in Files (roadmap B3.1 · B3.2 · B3.3).
// Streams results as they arrive, scope-selectable, preview-then-apply replace.

const { useState: useStateS, useEffect: useEffectS, useRef: useRefS, useMemo: useMemoS } = React;

// ---- corpus: the repo text, replicated per worktree so scope means something
function searchCorpus(ctx) {
  const FC = window.FILE_CONTENTS || {};
  const base = Object.keys(FC).map((p) => ({ path: p, text: FC[p] }));
  const agents = ctx.agents || [];
  const scoped = [];
  base.forEach((f) => {
    scoped.push({ ...f, worktree: null, agentId: null, projectId: (ctx.scopeAgent && ctx.scopeAgent.projectId) || null });
  });
  agents.forEach((a) => {
    // an agent's worktree diverges only in the files it touched
    (a.files || []).forEach((f) => {
      const src = FC[f.path];
      if (src == null) return;
      scoped.push({ path: f.path, text: src, worktree: a.worktree, agentId: a.id, projectId: a.projectId });
    });
  });
  return scoped;
}

function buildRe(q, { regex, word, caseSensitive }) {
  let src = regex ? q : q.replace(/[.*+?^${}()|[\]\\]/g, "\\$&");
  if (word) src = "\\b(?:" + src + ")\\b";
  try { return new RegExp(src, caseSensitive ? "g" : "gi"); } catch (e) { return null; }
}

function Hl({ text, ranges }) {
  if (!ranges || !ranges.length) return <span>{text}</span>;
  const out = []; let at = 0;
  ranges.forEach(([s, e], i) => {
    if (s > at) out.push(<span key={"p" + i}>{text.slice(at, s)}</span>);
    out.push(<mark key={"m" + i} style={{ background: "color-mix(in oklch, var(--accent), transparent 62%)", color: "var(--ink)", borderRadius: 2, padding: "0 1px" }}>{text.slice(s, e)}</mark>);
    at = e;
  });
  if (at < text.length) out.push(<span key="tail">{text.slice(at)}</span>);
  return <React.Fragment>{out}</React.Fragment>;
}

const SCOPES = [
  { k: "worktree", label: "This worktree" },
  { k: "project", label: "This project" },
  { k: "all", label: "All worktrees" },
  { k: "diff", label: "Current diff" },
];

function FindInFiles({ tool, ctx, onClose }) {
  const [mode, setMode] = useStateS(tool.mode === "replace" ? "replace" : "find");
  const [q, setQ] = useStateS("");
  const [rep, setRep] = useStateS("");
  const [scope, setScope] = useStateS("worktree");
  const [opts, setOpts] = useStateS({ caseSensitive: false, word: false, regex: false });
  const [hits, setHits] = useStateS([]);          // streamed matches
  const [scanned, setScanned] = useStateS(0);
  const [total, setTotal] = useStateS(0);
  const [busy, setBusy] = useStateS(false);
  const [skip, setSkip] = useStateS({});          // per-match opt-out for replace
  const [applied, setApplied] = useStateS(0);
  const [sel, setSel] = useStateS(0);
  const timer = useRefS(null);
  const inputRef = useRefS(null);
  useEffectS(() => { if (inputRef.current) inputRef.current.focus(); }, []);
  useEffectS(() => () => clearInterval(timer.current), []);
  useEffectS(() => { setMode(tool.mode === "replace" ? "replace" : "find"); }, [tool.mode]);

  const files = useMemoS(() => {
    const all = searchCorpus(ctx);
    const ag = ctx.scopeAgent;
    if (scope === "all") return all;
    if (scope === "project") return all.filter((f) => !f.projectId || !ag || f.projectId === ag.projectId);
    if (scope === "diff") {
      const paths = new Set((ag && ag.files || []).map((f) => f.path));
      return all.filter((f) => !f.worktree && paths.has(f.path));
    }
    return all.filter((f) => !f.worktree || (ag && f.agentId === ag.id));
  }, [ctx.agents, ctx.scopeAgent, scope]);

  // streamed scan: 6 files per 24ms tick, so results visibly arrive
  const run = () => {
    clearInterval(timer.current);
    setHits([]); setScanned(0); setTotal(0); setSkip({}); setApplied(0); setSel(0);
    const re = q ? buildRe(q, opts) : null;
    if (!re || !q) { setBusy(false); return; }
    setBusy(true);
    let i = 0;
    timer.current = setInterval(() => {
      const batch = [];
      let n = 0;
      while (i < files.length && n < 6) {
        const f = files[i++]; n++;
        const lines = f.text.split("\n");
        lines.forEach((s, li) => {
          re.lastIndex = 0;
          let m, ranges = [];
          while ((m = re.exec(s))) { ranges.push([m.index, m.index + (m[0].length || 1)]); if (!m[0].length) re.lastIndex++; if (ranges.length > 40) break; }
          if (ranges.length) batch.push({ id: (f.worktree || "main") + ":" + f.path + ":" + (li + 1), path: f.path, worktree: f.worktree, agentId: f.agentId, line: li + 1, s, ranges });
        });
      }
      if (batch.length) { setHits((h) => h.concat(batch)); setTotal((t) => t + batch.reduce((a, b) => a + b.ranges.length, 0)); }
      setScanned(i);
      if (i >= files.length) { clearInterval(timer.current); setBusy(false); }
    }, 24);
  };
  useEffectS(() => { const t = setTimeout(run, 220); return () => clearTimeout(t); }, [q, opts.caseSensitive, opts.word, opts.regex, scope, files.length]);

  const groups = useMemoS(() => {
    const map = new Map();
    hits.forEach((h) => {
      const key = (h.worktree ? h.worktree + "/" : "") + h.path;
      if (!map.has(key)) map.set(key, { key, path: h.path, worktree: h.worktree, agentId: h.agentId, items: [] });
      map.get(key).items.push(h);
    });
    return Array.from(map.values());
  }, [hits]);

  const flat = useMemoS(() => groups.flatMap((g) => g.items), [groups]);
  const open = (h) => ctx.openFileAt(h.path, h.line);
  const activeCount = flat.filter((h) => !skip[h.id]).length;

  const applyReplace = () => {
    const re = buildRe(q, opts);
    if (!re) return;
    const FC = window.FILE_CONTENTS || {};
    const touched = new Set();
    let count = 0;
    flat.forEach((h) => {
      if (skip[h.id] || h.worktree) return;
      const src = FC[h.path];
      if (src == null) return;
      const lines = src.split("\n");
      re.lastIndex = 0;
      const before = lines[h.line - 1];
      if (before == null) return;
      const after = before.replace(buildRe(q, opts), rep);
      if (after !== before) { lines[h.line - 1] = after; FC[h.path] = lines.join("\n"); touched.add(h.path); count += h.ranges.length; }
    });
    setApplied(count);
    ctx.flash("replaced " + count + " match" + (count === 1 ? "" : "es") + " in " + touched.size + " file" + (touched.size === 1 ? "" : "s"));
    setTimeout(run, 60);
  };

  const optBtn = (k, label, title) => (
    <button className="btn" onClick={() => setOpts((o) => ({ ...o, [k]: !o[k] }))} title={title}
      style={{ padding: "2px 7px", fontSize: 10.5, borderRadius: 4, minWidth: 26, justifyContent: "center",
        border: "1px solid " + (opts[k] ? "color-mix(in oklch, var(--accent), transparent 55%)" : "var(--hair)"),
        color: opts[k] ? "var(--ink)" : "var(--ink-3)", background: opts[k] ? "color-mix(in oklch, var(--accent), transparent 86%)" : "transparent" }}>
      {label}
    </button>
  );

  const inputStyle = { flex: 1, minWidth: 0, background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-sm)", padding: "6px 9px", color: "var(--ink)", fontFamily: "var(--font-mono)", fontSize: 12, outline: "none" };
  const preview = (h) => {
    const re = buildRe(q, opts);
    return re ? h.s.replace(re, rep) : h.s;
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", minHeight: 0, flex: 1 }}>
      {/* query row */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "9px 12px", borderBottom: "1px solid var(--hair)", flex: "none" }}>
        <div style={{ display: "flex", gap: 2, padding: 2, background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-sm)", flex: "none" }}>
          {[["find", "Find"], ["replace", "Replace"]].map(([k, l]) => (
            <button key={k} className="btn" onClick={() => setMode(k)}
              style={{ padding: "3px 9px", borderRadius: 4, fontSize: 10.5, background: mode === k ? "var(--panel-3)" : "transparent", color: mode === k ? "var(--ink)" : "var(--ink-3)", boxShadow: mode === k ? "0 0 0 1px var(--hair-2)" : "none" }}>{l}</button>
          ))}
        </div>
        <input ref={inputRef} value={q} onChange={(e) => setQ(e.target.value)} placeholder="Search in files…" style={inputStyle}
          onKeyDown={(e) => { if (e.key === "Enter") run(); if (e.key === "Escape") onClose(); }}
          onFocus={(e) => (e.target.style.borderColor = "var(--accent)")} onBlur={(e) => (e.target.style.borderColor = "var(--hair)")} />
        <div style={{ display: "flex", gap: 4, flex: "none" }}>
          {optBtn("caseSensitive", "Aa", "Match case")}
          {optBtn("word", "W", "Words")}
          {optBtn("regex", ".*", "Regular expression")}
        </div>
        <select className="osel" value={scope} onChange={(e) => setScope(e.target.value)} style={{ width: 150, flex: "none", padding: "5px 9px", fontSize: 11 }}>
          {SCOPES.map((s) => <option key={s.k} value={s.k}>{s.label}</option>)}
        </select>
      </div>

      {mode === "replace" && (
        <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 12px", borderBottom: "1px solid var(--hair)", flex: "none", background: "var(--panel-2)" }}>
          <Icon name="rename" size="sm" style={{ color: "var(--accent)" }} />
          <input value={rep} onChange={(e) => setRep(e.target.value)} placeholder="Replace with…" style={{ ...inputStyle, background: "var(--panel)" }} />
          <span style={{ fontSize: 10, color: "var(--ink-4)", flex: "none" }}>{activeCount} of {flat.length} selected</span>
          <button className="btn ghost-hair" style={{ flex: "none", fontSize: 11 }} disabled={!flat.length} onClick={() => setSkip({})}>All</button>
          <button className="btn ghost-hair" style={{ flex: "none", fontSize: 11 }} disabled={!flat.length}
            onClick={() => setSkip(Object.fromEntries(flat.map((h) => [h.id, true])))}>None</button>
          <button className="btn primary" style={{ flex: "none" }} disabled={!activeCount || !q}
            onClick={applyReplace}><Icon name="check" size="sm" />Replace {activeCount}</button>
        </div>
      )}

      {/* status row */}
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "6px 12px", borderBottom: "1px solid var(--hair)", flex: "none", fontSize: 10.5, color: "var(--ink-3)" }}>
        {busy
          ? <span style={{ display: "flex", alignItems: "center", gap: 7, color: "var(--accent-2)" }}><span className="dot running" style={{ background: "var(--st-running)" }} />streaming results… {scanned}/{files.length} files</span>
          : <span className="tnum">{q ? total + " match" + (total === 1 ? "" : "es") + " in " + groups.length + " file" + (groups.length === 1 ? "" : "s") : "type to search " + files.length + " indexed files"}</span>}
        {applied > 0 && <span style={{ color: "var(--st-done)" }}>{applied} replaced</span>}
        <span className="chip" style={{ marginLeft: "auto", fontSize: 9 }}>ripgrep · {SCOPES.find((s) => s.k === scope).label.toLowerCase()}</span>
        {busy && <button className="btn ghost-hair" style={{ padding: "2px 8px", fontSize: 10 }} onClick={() => { clearInterval(timer.current); setBusy(false); }}><Icon name="stop" size="sm" />Stop</button>}
      </div>

      {/* results */}
      <div className="scroll-y" style={{ flex: 1, minHeight: 0, padding: "4px 0" }}>
        {!q && <div style={{ padding: "16px 14px", fontSize: 11.5, color: "var(--ink-4)" }}>results stream in as files are scanned — click a line to open it at that position</div>}
        {q && !busy && !groups.length && <div style={{ padding: "16px 14px", fontSize: 11.5, color: "var(--ink-4)" }}>no matches for “{q}”</div>}
        {groups.map((g) => (
          <div key={g.key} style={{ marginBottom: 2 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 7, padding: "5px 12px", position: "sticky", top: 0, background: "var(--panel)", zIndex: 1, borderBottom: "1px solid var(--hair)" }}>
              <Icon name="file" size="sm" style={{ color: "var(--ink-3)" }} />
              <span style={{ fontSize: 11.5 }}>{fileName(g.path)}</span>
              <span style={{ fontSize: 9.5, color: "var(--ink-4)", flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{fileDir(g.path)}</span>
              {g.worktree && <span className="chip" style={{ fontSize: 9, padding: "1px 6px", color: "var(--accent-2)" }}>{g.worktree}</span>}
              <span className="tnum" style={{ fontSize: 9.5, color: "var(--ink-4)" }}>{g.items.reduce((a, b) => a + b.ranges.length, 0)}</span>
            </div>
            {g.items.map((h) => {
              const off = skip[h.id];
              const i = flat.indexOf(h);
              return (
                <div key={h.id} onClick={() => { setSel(i); open(h); }} onMouseEnter={() => setSel(i)}
                  style={{ display: "flex", alignItems: "flex-start", gap: 8, padding: "3px 12px 3px 26px", cursor: "pointer", fontSize: 11.5, lineHeight: 1.55,
                    background: sel === i ? "var(--panel-3)" : "transparent", opacity: off ? 0.45 : 1 }}>
                  {mode === "replace" && (
                    <button onClick={(e) => { e.stopPropagation(); setSkip((s) => ({ ...s, [h.id]: !s[h.id] })); }} title="Include this match"
                      style={{ flex: "none", width: 13, height: 13, marginTop: 3, borderRadius: 3, display: "grid", placeItems: "center", cursor: "pointer",
                        border: "1px solid " + (off ? "var(--hair-2)" : "var(--accent)"), background: off ? "transparent" : "var(--accent)" }}>
                      {!off && <Icon name="check" size="sm" style={{ width: 9, height: 9, color: "#06070b" }} />}
                    </button>
                  )}
                  <span className="tnum" style={{ flex: "none", width: 34, textAlign: "right", color: "var(--ink-4)", fontSize: 10.5 }}>{h.line}</span>
                  <span style={{ flex: 1, minWidth: 0, whiteSpace: "pre-wrap", wordBreak: "break-word", color: "var(--ink-2)" }}>
                    <Hl text={h.s.replace(/^\s+/, "")} ranges={h.ranges.map(([s, e]) => { const trim = h.s.length - h.s.replace(/^\s+/, "").length; return [Math.max(0, s - trim), Math.max(0, e - trim)]; })} />
                  </span>
                  {mode === "replace" && rep !== "" && !off && (
                    <span style={{ flex: 1, minWidth: 0, whiteSpace: "pre-wrap", wordBreak: "break-word", color: "var(--code-add-ink)", background: "var(--code-add-bg)", borderRadius: 3, padding: "0 4px" }}>{preview(h).replace(/^\s+/, "")}</span>
                  )}
                </div>
              );
            })}
          </div>
        ))}
      </div>
    </div>
  );
}

window.TOOL_PANELS.search = { kind: "search", label: "Find", icon: "search", Component: FindInFiles };
Object.assign(window, { FindInFiles });
