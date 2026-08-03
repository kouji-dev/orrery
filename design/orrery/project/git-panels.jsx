/* global React, Icon, fileName, fileDir, GIT_AUTHORS, GIT_HISTORY, GitActionButton, runAIVariant, estimateCost, fmtTok, fmtUsd */
// Orrery — git tool windows: commit graph (B4.1), branches & remotes (A3.1/A3.2),
// local history (B4.4). Lanes are computed once, rows are virtualized.

const { useState: useStateGP, useEffect: useEffectGP, useRef: useRefGP, useMemo: useMemoGP } = React;

const GITD = () => window.GIT || {};
const AUTHORS = () => GITD().authors || {};

const LANE_COLORS = ["var(--accent)", "var(--accent-2)", "var(--st-done)", "#f5c451", "#ff5d9e", "#3b82f6", "#c084fc"];

// ---------------------------------------------------------------- lane layout
function computeGraph(history) {
  let active = [];
  return history.map((c) => {
    let lane = active.indexOf(c.sha);
    if (lane < 0) { lane = active.indexOf(null); if (lane < 0) { lane = active.length; active.push(null); } }
    const before = active.slice();
    active = active.slice();
    active[lane] = (c.parents && c.parents[0]) || null;
    const extra = [];
    (c.parents || []).slice(1).forEach((p) => {
      let l = active.indexOf(p);
      if (l < 0) { l = active.indexOf(null); if (l < 0) { l = active.length; active.push(null); } active[l] = p; }
      extra.push(l);
    });
    active = active.map((s, i) => (s && active.indexOf(s) !== i ? null : s));
    return { c, lane, before, after: active.slice(), extra };
  });
}

const ROW_H = 30;

function GraphCell({ row, width }) {
  const lanes = Math.max(row.before.length, row.after.length, row.lane + 1);
  const x = (l) => 11 + l * 13;
  const mid = ROW_H / 2;
  const segs = [];
  for (let l = 0; l < lanes; l++) {
    const up = row.before[l], dn = row.after[l];
    const col = LANE_COLORS[l % LANE_COLORS.length];
    if (up) segs.push(<line key={"u" + l} x1={x(l)} y1={0} x2={x(l)} y2={l === row.lane ? mid : ROW_H} stroke={col} strokeWidth="1.6" strokeOpacity=".75" />);
    if (dn && l !== row.lane) segs.push(<line key={"d" + l} x1={x(l)} y1={up ? 0 : mid} x2={x(l)} y2={ROW_H} stroke={col} strokeWidth="1.6" strokeOpacity=".75" />);
  }
  if (row.after[row.lane]) segs.push(<line key="own" x1={x(row.lane)} y1={mid} x2={x(row.lane)} y2={ROW_H} stroke={LANE_COLORS[row.lane % LANE_COLORS.length]} strokeWidth="1.6" strokeOpacity=".75" />);
  row.extra.forEach((l, i) => segs.push(
    <path key={"m" + i} d={`M${x(row.lane)} ${mid} C ${x(row.lane)} ${mid + 10}, ${x(l)} ${mid + 6}, ${x(l)} ${ROW_H}`}
      fill="none" stroke={LANE_COLORS[l % LANE_COLORS.length]} strokeWidth="1.6" strokeOpacity=".8" />));
  const col = LANE_COLORS[row.lane % LANE_COLORS.length];
  return (
    <svg width={width} height={ROW_H} style={{ flex: "none", display: "block" }}>
      {segs}
      {row.c.merge
        ? <circle cx={x(row.lane)} cy={mid} r="4.6" fill="var(--panel)" stroke={col} strokeWidth="2" />
        : <circle cx={x(row.lane)} cy={mid} r="3.6" fill={col} />}
    </svg>
  );
}

function AuthorChip({ id }) {
  const a = AUTHORS()[id] || { short: (id || "?").slice(0, 2).toUpperCase(), color: "var(--ink-3)", name: id, kind: "human" };
  return (
    <span title={a.name + (a.kind === "agent" ? " · agent" : "")} style={{ display: "flex", alignItems: "center", gap: 5, flex: "none", fontSize: 10, color: "var(--ink-3)" }}>
      <span style={{ width: 15, height: 15, borderRadius: a.kind === "agent" ? 4 : "50%", display: "grid", placeItems: "center", fontSize: 8, fontWeight: 700,
        color: a.color, background: "color-mix(in oklch, " + a.color + ", transparent 84%)", border: "1px solid color-mix(in oklch, " + a.color + ", transparent 62%)" }}>{a.short}</span>
      {a.name}
    </span>
  );
}

function CommitGraphPanel({ ctx }) {
  const history = GITD().history || [];
  const [author, setAuthor] = useStateGP("");
  const [path, setPath] = useStateGP("");
  const [q, setQ] = useStateGP("");
  const [when, setWhen] = useStateGP("all");
  const [sel, setSel] = useStateGP([]);
  const [scrollTop, setScrollTop] = useStateGP(0);
  const [vh, setVh] = useStateGP(320);
  const scroller = useRefGP(null);

  const filtered = useMemoGP(() => history.filter((c) => {
    if (author && c.author !== author) return false;
    if (path && !(c.files || []).some((f) => f.path.toLowerCase().includes(path.toLowerCase()))) return false;
    if (q && !((c.msg || "") + " " + (c.body || "") + " " + c.sha).toLowerCase().includes(q.toLowerCase())) return false;
    if (when === "today" && !/^(\d+[mh]|now)$/.test(c.rel)) return false;
    if (when === "week" && /w$/.test(c.rel)) return false;
    return true;
  }), [history, author, path, q, when]);
  const rows = useMemoGP(() => computeGraph(filtered), [filtered]);
  const maxLanes = rows.reduce((m, r) => Math.max(m, r.before.length, r.after.length, r.lane + 1), 1);
  const gw = 16 + maxLanes * 13;

  useEffectGP(() => {
    const el = scroller.current; if (!el) return;
    const ro = new ResizeObserver(() => setVh(el.clientHeight));
    ro.observe(el); setVh(el.clientHeight);
    return () => ro.disconnect();
  }, []);
  // virtualized window
  const first = Math.max(0, Math.floor(scrollTop / ROW_H) - 6);
  const last = Math.min(rows.length, Math.ceil((scrollTop + vh) / ROW_H) + 6);
  const slice = rows.slice(first, last);

  const toggle = (sha, shift) => setSel((s) => {
    if (shift) { const nx = s.includes(sha) ? s.filter((x) => x !== sha) : [...s, sha]; return nx.slice(-2); }
    return [sha];
  });
  const authors = Array.from(new Set(history.map((c) => c.author)));

  return (
    <div style={{ display: "flex", flexDirection: "column", minHeight: 0, flex: 1 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 12px", borderBottom: "1px solid var(--hair)", flex: "none" }}>
        <input value={q} onChange={(e) => setQ(e.target.value)} placeholder="Filter by message or sha…"
          style={{ flex: 1, minWidth: 90, background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-sm)", padding: "5px 9px", color: "var(--ink)", fontFamily: "var(--font-mono)", fontSize: 11.5, outline: "none" }} />
        <input value={path} onChange={(e) => setPath(e.target.value)} placeholder="path…"
          style={{ width: 150, flex: "none", background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-sm)", padding: "5px 9px", color: "var(--ink)", fontFamily: "var(--font-mono)", fontSize: 11.5, outline: "none" }} />
        <select className="osel" value={author} onChange={(e) => setAuthor(e.target.value)} style={{ width: 132, flex: "none", padding: "4px 9px", fontSize: 11 }}>
          <option value="">All authors</option>
          {authors.map((a) => <option key={a} value={a}>{(AUTHORS()[a] || { name: a }).name}</option>)}
        </select>
        <select className="osel" value={when} onChange={(e) => setWhen(e.target.value)} style={{ width: 108, flex: "none", padding: "4px 9px", fontSize: 11 }}>
          <option value="all">Any date</option><option value="today">Today</option><option value="week">This week</option>
        </select>
        {sel.length === 2
          ? <button className="btn primary" style={{ flex: "none" }} onClick={() => ctx.openRange(sel)}><Icon name="diff" size="sm" />Compare range</button>
          : <span style={{ fontSize: 9.5, color: "var(--ink-4)", flex: "none" }}>shift-click two commits to diff a range</span>}
      </div>

      <div ref={scroller} className="scroll-y" style={{ flex: 1, minHeight: 0 }} onScroll={(e) => setScrollTop(e.currentTarget.scrollTop)}>
        <div style={{ height: rows.length * ROW_H, position: "relative" }}>
          <div style={{ position: "absolute", top: first * ROW_H, left: 0, right: 0 }}>
            {slice.map((r) => {
              const c = r.c;
              const on = sel.includes(c.sha);
              const refs = c.merge && r === rows[0] ? ["main"] : [];
              return (
                <div key={c.sha} onClick={(e) => toggle(c.sha, e.shiftKey)} onDoubleClick={() => ctx.openCommit(c.sha)}
                  style={{ height: ROW_H, display: "flex", alignItems: "center", gap: 9, padding: "0 12px 0 0", cursor: "pointer",
                    background: on ? "var(--panel-3)" : "transparent", borderLeft: "2px solid " + (on ? "var(--accent)" : "transparent") }}
                  onMouseEnter={(e) => { if (!on) e.currentTarget.style.background = "var(--panel-2)"; }}
                  onMouseLeave={(e) => { if (!on) e.currentTarget.style.background = "transparent"; }}>
                  <GraphCell row={r} width={gw} />
                  {refs.map((rf) => <span key={rf} className="chip" style={{ fontSize: 9, padding: "1px 6px", flex: "none", color: "var(--st-done)", borderColor: "color-mix(in oklch, var(--st-done), transparent 60%)" }}><Icon name="branch" size="sm" style={{ width: 10, height: 10 }} />{rf}</span>)}
                  <span style={{ flex: 1, minWidth: 0, fontSize: 11.5, color: "var(--ink)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                    {c.merge && <Icon name="merge" size="sm" style={{ width: 11, height: 11, color: "var(--ink-3)", marginRight: 5 }} />}{c.msg}
                  </span>
                  <AuthorChip id={c.author} />
                  <span className="tnum" style={{ flex: "none", width: 56, fontSize: 10, color: "var(--ink-4)", textAlign: "right" }}>{c.rel}</span>
                  <span className="tnum" style={{ flex: "none", width: 58, fontSize: 10, color: "var(--accent-2)" }}>{c.sha}</span>
                </div>
              );
            })}
          </div>
        </div>
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "5px 12px", borderTop: "1px solid var(--hair)", flex: "none", fontSize: 9.5, color: "var(--ink-4)" }}>
        <span className="tnum">{ctx.project ? ctx.project.name + " · " : ""}{filtered.length} of {history.length} commits · {maxLanes} lanes · virtualized ({slice.length} rows mounted)</span>
        <span style={{ marginLeft: "auto" }}>double-click a commit to open its diff</span>
      </div>
    </div>
  );
}

// ------------------------------------------------------- branches & remotes
window.GIT_REMOTES = window.GIT_REMOTES || [
  { name: "origin", url: "git@github.com:northwind/payments-service.git", fetched: "2m ago" },
  { name: "upstream", url: "https://github.com/stripe-samples/payments.git", fetched: "3d ago" },
];

function aheadBehind(name) {
  let h = 0; for (let i = 0; i < name.length; i++) h = (h * 33 + name.charCodeAt(i)) | 0;
  h = Math.abs(h);
  return { ahead: h % 7, behind: (h >> 3) % 4 };
}

function BranchesPanel({ ctx }) {
  const ag = ctx.scopeAgent;
  const project = ctx.project || (ctx.projects || []).find((p) => p.id === (ag && ag.projectId)) || (ctx.projects || [])[0];
  const [remotes, setRemotes] = useStateGP(window.GIT_REMOTES);
  const [fetching, setFetching] = useStateGP(0);      // 0..1 progress
  const [newBranch, setNewBranch] = useStateGP("");
  const [from, setFrom] = useStateGP(project ? project.branch : "main");
  const [adding, setAdding] = useStateGP(false);
  const [remoteName, setRemoteName] = useStateGP("");
  const [remoteUrl, setRemoteUrl] = useStateGP("");
  const [current, setCurrent] = useStateGP(ag ? ag.branch : (project ? project.branch : "main"));
  const [extra, setExtra] = useStateGP([]);
  const branches = (project ? project.branches : ["main"]).concat(extra, ag ? [ag.branch] : []).filter((b, i, a) => a.indexOf(b) === i);

  const doFetch = (remote) => {
    setFetching(0.02);
    const iv = setInterval(() => setFetching((f) => {
      const nx = f + 0.06 + Math.random() * 0.13;
      if (nx >= 1) { clearInterval(iv); setTimeout(() => setFetching(0), 550); ctx.flash("fetched " + remote + " · 4 refs updated, 1 pruned"); return 1; }
      return nx;
    }), 90);
  };
  const rowBtn = (icon, label, onClick, danger) => (
    <button className="btn" onClick={(e) => { e.stopPropagation(); onClick(); }} title={label}
      style={{ padding: "2px 6px", fontSize: 10, color: danger ? "var(--st-blocked)" : "var(--ink-3)" }}>
      <Icon name={icon} size="sm" style={{ width: 11, height: 11 }} />{label}
    </button>
  );

  return (
    <div style={{ display: "grid", gridTemplateColumns: "300px 1fr", minHeight: 0, flex: 1 }}>
      {/* remotes */}
      <div style={{ display: "flex", flexDirection: "column", minHeight: 0, borderRight: "1px solid var(--hair)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 7, padding: "8px 12px", borderBottom: "1px solid var(--hair)", flex: "none" }}>
          <span className="up" style={{ fontSize: 9, color: "var(--ink-3)" }}>Remotes</span>
          <button className="btn ghost-hair" style={{ marginLeft: "auto", padding: "2px 7px", fontSize: 10 }} onClick={() => setAdding((v) => !v)}>
            <Icon name="plus" size="sm" />Add
          </button>
        </div>
        {adding && (
          <div style={{ padding: 10, borderBottom: "1px solid var(--hair)", display: "grid", gap: 6, background: "var(--panel-2)" }}>
            <input value={remoteName} onChange={(e) => setRemoteName(e.target.value)} placeholder="name" style={{ background: "var(--panel)", border: "1px solid var(--hair)", borderRadius: "var(--r-sm)", padding: "5px 8px", color: "var(--ink)", fontFamily: "var(--font-mono)", fontSize: 11, outline: "none" }} />
            <input value={remoteUrl} onChange={(e) => setRemoteUrl(e.target.value)} placeholder="git@… or https://…" style={{ background: "var(--panel)", border: "1px solid var(--hair)", borderRadius: "var(--r-sm)", padding: "5px 8px", color: "var(--ink)", fontFamily: "var(--font-mono)", fontSize: 11, outline: "none" }} />
            <button className="btn primary" disabled={!remoteName.trim() || !remoteUrl.trim()} style={{ justifyContent: "center", fontSize: 11 }}
              onClick={() => { const r = { name: remoteName.trim(), url: remoteUrl.trim(), fetched: "never" }; window.GIT_REMOTES = [...remotes, r]; setRemotes(window.GIT_REMOTES); setAdding(false); setRemoteName(""); setRemoteUrl(""); ctx.flash("added remote " + r.name); }}>
              Add remote
            </button>
          </div>
        )}
        <div className="scroll-y" style={{ flex: 1, padding: "4px 0" }}>
          {remotes.map((r) => (
            <div key={r.name} style={{ padding: "8px 12px", borderBottom: "1px solid var(--hair)" }}>
              <div style={{ display: "flex", alignItems: "center", gap: 7 }}>
                <Icon name="globe" size="sm" style={{ color: "var(--accent-2)" }} />
                <span style={{ fontSize: 11.5, color: "var(--ink)" }}>{r.name}</span>
                <span className="tnum" style={{ marginLeft: "auto", fontSize: 9.5, color: "var(--ink-4)" }}>{r.fetched}</span>
              </div>
              <div style={{ fontSize: 9.5, color: "var(--ink-4)", marginTop: 4, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{r.url}</div>
              <div style={{ display: "flex", gap: 4, marginTop: 6 }}>
                {rowBtn("refresh", "Fetch", () => doFetch(r.name))}
                {rowBtn("archive", "Prune", () => ctx.flash("pruned " + r.name + " · 2 stale refs removed"))}
                {rowBtn("rename", "Set URL", () => ctx.flash("remote " + r.name + " URL editable inline"))}
                {rowBtn("trash", "Remove", () => { window.GIT_REMOTES = remotes.filter((x) => x.name !== r.name); setRemotes(window.GIT_REMOTES); ctx.flash("removed remote " + r.name); }, true)}
              </div>
            </div>
          ))}
        </div>
        {fetching > 0 && (
          <div style={{ padding: "8px 12px", borderTop: "1px solid var(--hair)", flex: "none" }}>
            <div style={{ display: "flex", fontSize: 9.5, color: "var(--ink-3)", marginBottom: 5 }}>
              <span>receiving objects…</span><span className="tnum" style={{ marginLeft: "auto" }}>{Math.round(fetching * 100)}%</span>
            </div>
            <div className="meter"><i style={{ width: Math.round(fetching * 100) + "%" }} /></div>
          </div>
        )}
      </div>

      {/* branches */}
      <div style={{ display: "flex", flexDirection: "column", minHeight: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 12px", borderBottom: "1px solid var(--hair)", flex: "none" }}>
          <span className="up" style={{ fontSize: 9, color: "var(--ink-3)" }}>Branches · {project ? project.name : ""}</span>
          <div style={{ marginLeft: "auto", display: "flex", gap: 7, alignItems: "center" }}>
            <GitActionButton label="Fetch" icon="refresh" small flash={ctx.flash} onNative={() => doFetch("origin")} variants={[]} />
            <GitActionButton label="Pull" icon="stage" small flash={ctx.flash} onNative={() => { doFetch("origin"); ctx.flash("pulled origin/" + current + " · fast-forward"); }} variants={[]} />
            <GitActionButton label="New branch" icon="plus" kind="primary" small flash={ctx.flash}
              onNative={() => { const nm = newBranch.trim() || "feature/" + Math.random().toString(36).slice(2, 7); setExtra((x) => [...x, nm]); setCurrent(nm); ctx.flash("created + checked out " + nm + " from " + from); }}
              variants={[{ id: "name", label: "Name branch from the task", op: "branch", icon: "spark", run: (est) => { const nm = "agent/" + (ag ? ag.name : "task") + "-followup"; setExtra((x) => [...x, nm]); ctx.flash("AI named branch → " + nm + " · " + fmtTok(est.tokensHigh) + " tok"); } }]}
              estimateInput={{ op: "branch", model: ag ? ag.model : "sonnet-4.6" }} />
          </div>
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "7px 12px", borderBottom: "1px solid var(--hair)", flex: "none", background: "var(--panel-2)" }}>
          <input value={newBranch} onChange={(e) => setNewBranch(e.target.value)} placeholder="new branch name…"
            style={{ flex: 1, background: "var(--panel)", border: "1px solid var(--hair)", borderRadius: "var(--r-sm)", padding: "5px 9px", color: "var(--ink)", fontFamily: "var(--font-mono)", fontSize: 11.5, outline: "none" }} />
          <span style={{ fontSize: 10, color: "var(--ink-4)" }}>from</span>
          <select className="osel" value={from} onChange={(e) => setFrom(e.target.value)} style={{ width: 160, flex: "none", padding: "4px 9px", fontSize: 11 }}>
            {branches.map((b) => <option key={b} value={b}>{b}</option>)}
          </select>
        </div>
        <div className="scroll-y" style={{ flex: 1, padding: "4px 0" }}>
          {branches.map((b) => {
            const { ahead, behind } = aheadBehind(b);
            const on = b === current;
            return (
              <div key={b} style={{ display: "flex", alignItems: "center", gap: 9, padding: "7px 12px", borderBottom: "1px solid var(--hair)",
                background: on ? "color-mix(in oklch, var(--accent), transparent 92%)" : "transparent" }}>
                <Icon name="branch" size="sm" style={{ color: on ? "var(--accent)" : "var(--ink-4)" }} />
                <span style={{ fontSize: 11.5, color: on ? "var(--ink)" : "var(--ink-2)" }}>{b}</span>
                {on && <span className="chip" style={{ fontSize: 8.5, padding: "0 5px", color: "var(--accent)", borderColor: "color-mix(in oklch, var(--accent), transparent 58%)" }}>HEAD</span>}
                <span className="tnum" style={{ display: "flex", gap: 7, fontSize: 9.5, color: "var(--ink-4)" }}>
                  {ahead > 0 && <span style={{ color: "var(--st-done)" }}>↑{ahead}</span>}
                  {behind > 0 && <span style={{ color: "#f5c451" }}>↓{behind}</span>}
                  {!ahead && !behind && <span>in sync</span>}
                </span>
                <div style={{ marginLeft: "auto", display: "flex", gap: 3 }}>
                  {!on && rowBtn("enter", "Checkout", () => { setCurrent(b); ctx.flash("checked out " + b); })}
                  {rowBtn("link", "Upstream", () => ctx.flash("set upstream origin/" + b))}
                  {!on && rowBtn("merge", "Merge in", () => ctx.flash("merged " + b + " → " + current))}
                  {rowBtn("rename", "Rename", () => ctx.flash("rename " + b))}
                  {!on && rowBtn("trash", "Delete", () => { setExtra((x) => x.filter((n) => n !== b)); ctx.flash("deleted branch " + b); }, true)}
                </div>
              </div>
            );
          })}
        </div>
      </div>
    </div>
  );
}

// ----------------------------------------------------------- local history
// snapshots are derived from the worktree the watcher already tracks
function snapshotsFor(ag) {
  if (!ag) return [];
  const files = ag.files || [];
  const out = [];
  const kinds = [["agent turn", "agent"], ["external edit", "edit"], ["pre-command", "guard"], ["commit", "commit"]];
  const n = Math.max(4, Math.min(14, files.length * 3));
  for (let i = 0; i < n; i++) {
    const [label, kind] = kinds[i % kinds.length];
    const touched = files.length ? files.slice(0, 1 + ((i * 3) % Math.max(1, files.length))) : [];
    out.push({
      id: "snap" + i, label, kind,
      when: i === 0 ? "just now" : i < 3 ? i * 4 + "m ago" : i < 7 ? i * 11 + "m ago" : Math.round(i * 0.7) + "h ago",
      bytes: 12000 + ((i * 7919) % 90000),
      files: touched.map((f) => ({ path: f.path, add: Math.max(1, Math.round((f.add || 6) / (i + 1))), del: Math.round((f.del || 2) / (i + 2)) })),
    });
  }
  return out;
}

function LocalHistoryPanel({ ctx }) {
  const ag = ctx.scopeAgent;
  const snaps = useMemoGP(() => snapshotsFor(ag), [ag && ag.id, ag && ag.files]);
  const [sel, setSel] = useStateGP(0);
  const s = snaps[sel];
  const totalMb = (snaps.reduce((a, b) => a + b.bytes, 0) / 1e6).toFixed(1);
  const kindColor = { agent: "var(--accent)", edit: "var(--accent-2)", guard: "#f5c451", commit: "var(--st-done)" };
  if (!ag) return <div style={{ padding: 18, fontSize: 11.5, color: "var(--ink-4)" }}>select an agent to see its worktree history</div>;
  return (
    <div style={{ display: "grid", gridTemplateColumns: "280px 1fr", minHeight: 0, flex: 1 }}>
      <div style={{ display: "flex", flexDirection: "column", minHeight: 0, borderRight: "1px solid var(--hair)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 7, padding: "8px 12px", borderBottom: "1px solid var(--hair)", flex: "none" }}>
          <Icon name="clock" size="sm" style={{ color: "var(--accent)" }} />
          <span className="up" style={{ fontSize: 9, color: "var(--ink-3)" }}>Snapshots</span>
          <span className="tnum" style={{ marginLeft: "auto", fontSize: 9.5, color: "var(--ink-4)" }}>{snaps.length}</span>
        </div>
        <div className="scroll-y" style={{ flex: 1, padding: "4px 0" }}>
          {snaps.map((sn, i) => (
            <div key={sn.id} onClick={() => setSel(i)}
              style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 12px", cursor: "pointer", borderLeft: "2px solid " + (i === sel ? "var(--accent)" : "transparent"), background: i === sel ? "var(--panel-3)" : "transparent" }}>
              <span style={{ width: 7, height: 7, borderRadius: 2, flex: "none", background: kindColor[sn.kind] }} />
              <span style={{ fontSize: 11.5, color: i === sel ? "var(--ink)" : "var(--ink-2)" }}>{sn.label}</span>
              <span className="tnum" style={{ marginLeft: "auto", fontSize: 9.5, color: "var(--ink-4)" }}>{sn.when}</span>
            </div>
          ))}
        </div>
        <div style={{ padding: "7px 12px", borderTop: "1px solid var(--hair)", fontSize: 9, color: "var(--ink-4)", flex: "none" }} className="tnum">
          {totalMb} MB of 200 MB cap · pruned after 14 days
        </div>
      </div>
      <div style={{ display: "flex", flexDirection: "column", minHeight: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 9, padding: "8px 12px", borderBottom: "1px solid var(--hair)", flex: "none" }}>
          <span style={{ fontSize: 11.5, color: "var(--ink)" }}>{s ? s.label : ""}</span>
          <span className="tnum" style={{ fontSize: 9.5, color: "var(--ink-4)" }}>{s ? s.when + " · " + (s.bytes / 1000).toFixed(0) + " KB" : ""}</span>
          <div style={{ marginLeft: "auto", display: "flex", gap: 7 }}>
            <button className="btn ghost-hair" style={{ fontSize: 11 }} onClick={() => ctx.flash("showing diff against " + (s ? s.when : ""))}><Icon name="diff" size="sm" />Show diff</button>
            <button className="btn ghost-hair" style={{ fontSize: 11, color: "var(--st-blocked)" }}
              onClick={() => ctx.flash("reverted worktree to snapshot · " + (s ? s.when : ""))}><Icon name="discard" size="sm" />Revert to this point</button>
          </div>
        </div>
        <div className="scroll-y" style={{ flex: 1, padding: "4px 0" }}>
          {(s ? s.files : []).length === 0 && <div style={{ padding: 16, fontSize: 11.5, color: "var(--ink-4)" }}>no file changes captured in this snapshot</div>}
          {(s ? s.files : []).map((f) => (
            <div key={f.path} style={{ display: "flex", alignItems: "center", gap: 9, padding: "6px 14px", borderBottom: "1px solid var(--hair)" }}>
              <Icon name="file" size="sm" style={{ color: "var(--ink-4)" }} />
              <span style={{ fontSize: 11.5 }}>{fileName(f.path)}</span>
              <span style={{ fontSize: 9.5, color: "var(--ink-4)", flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{fileDir(f.path)}</span>
              <span className="tnum" style={{ fontSize: 9.5, display: "flex", gap: 5 }}>
                <span style={{ color: "var(--code-add-ink)" }}>+{f.add}</span>{f.del > 0 && <span style={{ color: "var(--code-del-ink)" }}>−{f.del}</span>}
              </span>
              <button className="btn" style={{ padding: "2px 6px", fontSize: 10 }} onClick={() => ctx.openFileAt(f.path)}><Icon name="enter" size="sm" style={{ width: 11, height: 11 }} />Open</button>
              <button className="btn" style={{ padding: "2px 6px", fontSize: 10, color: "var(--st-blocked)" }} onClick={() => ctx.flash("reverted " + fileName(f.path) + " to " + s.when)}><Icon name="discard" size="sm" style={{ width: 11, height: 11 }} />Revert</button>
            </div>
          ))}
        </div>
      </div>
    </div>
  );
}

window.TOOL_PANELS.graph = { kind: "graph", label: "Git Graph", icon: "graph", Component: CommitGraphPanel };
window.TOOL_PANELS.branches = { kind: "branches", label: "Branches", icon: "branch", Component: BranchesPanel };
window.TOOL_PANELS.history = { kind: "history", label: "Local History", icon: "clock", Component: LocalHistoryPanel };
Object.assign(window, { CommitGraphPanel, BranchesPanel, LocalHistoryPanel, computeGraph, snapshotsFor });
