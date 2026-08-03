/* global React, Icon, fileName, fileDir, langOf, highlight, getFileLines, AuthorAvatar, Sha, RStateBadge, AddDel, HunkRows, FileDiff, DiffFileHeader, DiffFileList, diffFor, synthDiff, ConflictView, FileHistoryView */
// Orrery — agent-scoped git inspection UI (right-panel history/conflict +
// center diff/blame/file-history/conflict views).

const { useState: useStateAG, useMemo: useMemoAG } = React;
const AGG = () => window.AGENT_GIT;
const GAU = () => window.GIT.authors;

// ============================ blame gutter ================================
function ageBgA(age) { return "color-mix(in oklch, var(--accent), transparent " + (86 + Math.round(age * 11)) + "%)"; }

function FileBlameGutter({ agent, path, onOpenCommit }) {
  const blame = AGG().agentBlame(agent, path);
  const lang = langOf(path);
  const [hover, setHover] = useStateAG(null);
  const rows = blame.map((ln, i) => ({ ...ln, first: i === 0 || blame[i - 1].sha !== ln.sha }));
  return (
    <div className="scroll-y" style={{ flex: 1, position: "relative", background: "var(--bg)" }}>
      <pre style={{ margin: 0, fontFamily: "var(--font-mono)", fontSize: 12, lineHeight: 1.7 }}>
        {rows.map((ln) => {
          const a = GAU()[ln.author] || {};
          return (
            <div key={ln.n} style={{ display: "flex" }}>
              <div onMouseEnter={(e) => { const r = e.currentTarget.getBoundingClientRect(); setHover({ ...ln, msg: (window.GIT.history.find((c) => c.sha === ln.sha) || {}).msg || ln.s, x: r.right, y: r.top, name: a.name }); }}
                onMouseLeave={() => setHover(null)}
                onClick={() => onOpenCommit && onOpenCommit(ln.sha)}
                style={{ flex: "none", width: 196, display: "flex", alignItems: "center", gap: 7, padding: "0 9px 0 0", borderRight: "1px solid var(--hair)", cursor: "pointer", userSelect: "none", background: ageBgA(ln.age) }}>
                <span style={{ flex: "none", width: 3, alignSelf: "stretch", background: a.color || "var(--ink-4)", opacity: 0.25 + (1 - ln.age) * 0.7 }} />
                {ln.first ? <React.Fragment>
                  <AuthorAvatar id={ln.author} size={14} />
                  <span style={{ fontSize: 10, color: "var(--ink-2)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", flex: 1, minWidth: 0 }}>{a.name}</span>
                  <span className="tnum" style={{ fontSize: 9, color: "var(--ink-4)" }}>{ln.rel}</span>
                  <span className="tnum chip" style={{ fontSize: 8, padding: "0 4px" }}>{ln.sha}</span>
                </React.Fragment> : <span style={{ flex: 1 }} />}
              </div>
              <span className="tnum" style={{ width: 44, flex: "none", textAlign: "right", padding: "0 11px 0 0", color: "var(--ink-4)", userSelect: "none" }}>{ln.n}</span>
              <code style={{ flex: 1, whiteSpace: "pre-wrap", wordBreak: "break-word", paddingRight: 14, color: "var(--ink-2)" }}>{highlight(ln.s, lang)}</code>
            </div>
          );
        })}
      </pre>
      {hover && (
        <div style={{ position: "fixed", left: Math.min(hover.x + 8, window.innerWidth - 300), top: hover.y, zIndex: 80, width: 280, background: "var(--elev)", border: "1px solid var(--hair-2)", borderRadius: "var(--r-md)", boxShadow: "var(--shadow)", padding: 11, pointerEvents: "none" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 7 }}>
            <AuthorAvatar id={hover.author} size={20} />
            <span style={{ fontSize: 11, fontWeight: 600, color: "var(--ink)", flex: 1 }}>{hover.name}</span>
            <Sha sha={hover.sha} />
          </div>
          <div style={{ fontSize: 10.5, color: "var(--ink-2)", lineHeight: 1.45, textWrap: "pretty" }}>{hover.msg}</div>
          <div style={{ fontSize: 9, color: "var(--accent-2)", marginTop: 7, display: "flex", alignItems: "center", gap: 5 }}><Icon name="enter" size="sm" style={{ width: 11, height: 11 }} />click → open commit diff</div>
        </div>
      )}
    </div>
  );
}

// ===================== right-panel: branch commits ========================
function AgentCommitRow({ c, expanded, selected, selecting, onToggle, onToggleSel, onOpenCommit, onOpenFileHistory }) {
  const a = GAU()[c.author] || {};
  const tot = c.files.reduce((s, f) => ({ a: s.a + f.add, d: s.d + f.del }), { a: 0, d: 0 });
  return (
    <div style={{ margin: "0 6px", borderRadius: "var(--r-sm)", background: selected ? "color-mix(in oklch, var(--accent), transparent 90%)" : "transparent" }}>
      <div style={{ position: "relative", display: "flex", gap: 8, padding: "7px 8px 7px 6px", cursor: "pointer" }}
        onClick={() => onToggle(c.sha)}
        onMouseEnter={(e) => { if (!selected) e.currentTarget.parentNode.style.background = "var(--panel-2)"; }}
        onMouseLeave={(e) => { if (!selected) e.currentTarget.parentNode.style.background = "transparent"; }}>
        <button onClick={(e) => { e.stopPropagation(); onToggleSel(c.sha); }} title="Select for range diff"
          style={{ flex: "none", width: 15, height: 15, marginTop: 1, borderRadius: 4, display: "grid", placeItems: "center", cursor: "pointer", border: "1px solid " + (selected ? "var(--accent)" : "var(--hair-2)"), background: selected ? "var(--accent)" : "transparent", opacity: selecting || selected ? 1 : 0.5 }}>
          {selected && <Icon name="check" size="sm" style={{ width: 10, height: 10, color: "#06070b" }} />}
        </button>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 6 }}>
            <Icon name={expanded ? "chevronD" : "chevron"} size="sm" style={{ width: 11, height: 11, color: "var(--ink-4)", flex: "none" }} />
            <span style={{ fontSize: 11, color: "var(--ink)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{c.msg}</span>
            {c.head && <span className="chip" style={{ fontSize: 8, padding: "0 5px", color: "var(--accent-2)", flex: "none" }}>HEAD</span>}
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 6, marginTop: 4, paddingLeft: 17, fontSize: 9.5, color: "var(--ink-4)" }} className="tnum">
            <AuthorAvatar id={c.author} size={13} />
            <Sha sha={c.sha} dim />
            <span>{c.files.length}f</span>
            <AddDel add={tot.a} del={tot.d} />
            <span style={{ marginLeft: "auto" }}>{c.rel}</span>
          </div>
        </div>
      </div>
      {expanded && (
        <div style={{ paddingLeft: 28, paddingBottom: 4 }}>
          {c.files.map((f) => (
            <div key={f.path} className="rise" style={{ display: "flex", alignItems: "center", gap: 7, padding: "3px 8px 3px 4px", cursor: "pointer", borderRadius: "var(--r-sm)" }}
              onClick={() => onOpenCommit(c.sha, f.path)}
              onMouseEnter={(e) => e.currentTarget.style.background = "var(--panel-2)"}
              onMouseLeave={(e) => e.currentTarget.style.background = "transparent"}>
              <RStateBadge state={f.state} />
              <span style={{ flex: 1, fontSize: 10.5, color: "var(--ink-2)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{fileName(f.path)}</span>
              <AddDel add={f.add} del={f.del} />
              <button className="pane-btn" title="File history" onClick={(e) => { e.stopPropagation(); onOpenFileHistory(f.path); }} style={{ flex: "none" }}>
                <Icon name="clock" size="sm" style={{ width: 12, height: 12 }} />
              </button>
            </div>
          ))}
        </div>
      )}
    </div>
  );
}

function AgentCommitHistory({ ag, onOpenCommit, onOpenRange, onOpenFileHistory }) {
  const history = useMemoAG(() => AGG().agentHistory(ag), [ag.id]);
  const [exp, setExp] = useStateAG(history[0] ? history[0].sha : null);
  const [sel, setSel] = useStateAG([]);
  const toggle = (sha) => setExp((e) => (e === sha ? null : sha));
  const toggleSel = (sha) => setSel((s) => s.includes(sha) ? s.filter((x) => x !== sha) : [...s, sha]);
  const selecting = sel.length > 0;
  return (
    <div>
      <div style={{ display: "flex", alignItems: "center", gap: 7, padding: "4px 14px" }}>
        <span className="up" style={{ fontSize: 9, color: "var(--ink-3)" }}>Commits on this branch</span>
        <span className="tnum" style={{ fontSize: 9, color: "var(--ink-4)" }}>{history.length}</span>
      </div>
      {selecting && (
        <div className="rise" style={{ display: "flex", alignItems: "center", gap: 7, margin: "2px 8px 6px", padding: "6px 9px", borderRadius: "var(--r-sm)", background: "color-mix(in oklch, var(--accent), transparent 90%)", border: "1px solid color-mix(in oklch, var(--accent), transparent 70%)" }}>
          <span style={{ fontSize: 10.5, color: "var(--ink)" }}><b className="tnum">{sel.length}</b> selected</span>
          <button className="btn" onClick={() => setSel([])} style={{ marginLeft: "auto", padding: "2px 7px", fontSize: 10, color: "var(--ink-3)" }}>Clear</button>
          <button className="btn primary" onClick={() => onOpenRange(sel)} style={{ padding: "3px 9px", fontSize: 10.5 }}>
            <Icon name="diff" size="sm" />Diff ({sel.length})
          </button>
        </div>
      )}
      {history.map((c) => (
        <AgentCommitRow key={c.sha} c={c} expanded={exp === c.sha} selected={sel.includes(c.sha)} selecting={selecting}
          onToggle={toggle} onToggleSel={toggleSel} onOpenCommit={onOpenCommit} onOpenFileHistory={onOpenFileHistory} />
      ))}
    </div>
  );
}

// right-panel conflict card
function AgentConflictCard({ ag, project, onOpenConflict }) {
  const sess = AGG().agentConflict(ag, project);
  return (
    <div style={{ margin: "10px 12px", border: "1px solid color-mix(in oklch, var(--st-blocked), transparent 55%)", borderRadius: "var(--r-md)", overflow: "hidden", background: "color-mix(in oklch, var(--st-blocked), transparent 92%)" }}>
      <div style={{ padding: 11 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 7, marginBottom: 6 }}>
          <Icon name="merge" size="sm" style={{ color: "var(--st-blocked)" }} />
          <span style={{ fontSize: 11.5, fontWeight: 600, color: "var(--ink)" }}>Merge conflicts</span>
          <span className="chip tnum" style={{ marginLeft: "auto", fontSize: 9, padding: "0 6px", color: "var(--st-blocked)", borderColor: "color-mix(in oklch, var(--st-blocked), transparent 55%)" }}>{sess.files.length} files</span>
        </div>
        <div style={{ fontSize: 10.5, color: "var(--ink-3)", lineHeight: 1.5, marginBottom: 9 }}>
          {sess.title} stopped — <b style={{ color: "var(--st-blocked)" }}>{sess.files.reduce((s, f) => s + f.conflicts, 0)} conflicts</b> need resolving before this branch can land.
        </div>
        <button className="btn ghost-hair" disabled style={{ width: "100%", justifyContent: "center", borderColor: "color-mix(in oklch, var(--st-blocked), transparent 65%)", color: "var(--st-blocked)", opacity: 1 }}>
          <Icon name="diff" size="sm" />Resolve in the Diff tab →
        </button>
      </div>
    </div>
  );
}

// ====================== center: commit / range diff =======================
function CommitContextHeader({ left, title, sub }) {
  return (
    <div style={{ padding: "9px 16px", borderBottom: "1px solid var(--hair)", background: "var(--panel)", flex: "none" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8 }}>{left}
        <span style={{ fontSize: 12.5, fontWeight: 600, color: "var(--ink)", textWrap: "pretty" }}>{title}</span>
      </div>
      {sub && <div style={{ marginTop: 5, fontSize: 10, color: "var(--ink-4)", display: "flex", alignItems: "center", gap: 8, flexWrap: "wrap" }} className="tnum">{sub}</div>}
    </div>
  );
}

// diff with an Annotate (blame) toggle — lets you flip any commit/range diff
// into its per-line blame gutter, same affordance as the file viewer.
function DiffOrBlame({ ag, path, diff, addDel, onOpenCommit, view }) {
  const [blame, setBlame] = useStateAG(false);
  React.useEffect(() => { setBlame(false); }, [path]);
  const annotate = (
    <button className={"btn " + (blame ? "" : "ghost-hair")} onClick={() => setBlame((v) => !v)} title="Toggle blame / annotate"
      style={{ padding: "3px 9px", fontSize: 10, color: blame ? "var(--ink)" : "var(--ink-3)",
        background: blame ? "color-mix(in oklch, var(--accent), transparent 86%)" : "transparent",
        border: "1px solid " + (blame ? "color-mix(in oklch, var(--accent), transparent 60%)" : "var(--hair)") }}>
      <Icon name="git" size="sm" style={{ color: blame ? "var(--accent)" : "inherit" }} />Annotate
    </button>
  );
  const right = <React.Fragment>{addDel}{annotate}</React.Fragment>;
  if (blame) {
    return (
      <div style={{ flex: 1, display: "flex", flexDirection: "column", minHeight: 0, background: "var(--bg)" }}>
        <DiffFileHeader path={path} lang={diff && diff.lang} right={right} />
        <FileBlameGutter agent={ag} path={path} onOpenCommit={onOpenCommit} />
      </div>
    );
  }
  return <FileDiff path={path} diff={diff} headerRight={right} agentId={ag.id} view={view} />;
}

function AgentCommitDiffView({ ag, sha, initPath }) {
  const history = useMemoAG(() => AGG().agentHistory(ag), [ag.id]);
  const c = history.find((x) => x.sha === sha) || history[0];
  const [selPath, setSelPath] = useStateAG((initPath && c.files.some((f) => f.path === initPath) ? initPath : (c.files[0] ? c.files[0].path : null)));
  const a = GAU()[c.author] || {};
  const f = c.files.find((x) => x.path === selPath);
  const diff = f ? diffFor(sha, f) : null;
  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", minHeight: 0, background: "var(--panel-2)" }}>
      <CommitContextHeader left={<><Icon name="commit" size="sm" style={{ color: "var(--accent-2)" }} /></>} title={c.msg}
        sub={<><AuthorAvatar id={c.author} size={15} /><span style={{ color: "var(--ink-2)" }}>{a.name}</span><Sha sha={c.sha} /><span>committed {c.rel} ago</span></>} />
      <div style={{ flex: 1, display: "grid", gridTemplateColumns: "232px 1fr", minHeight: 0 }}>
        <div style={{ minHeight: 0, borderRight: "1px solid var(--hair)", background: "var(--panel)" }}>
          <DiffFileList files={c.files} selPath={selPath} onSelect={setSelPath} title="Files in commit" />
        </div>
        {selPath ? <DiffOrBlame key={selPath} ag={ag} path={selPath} diff={diff} view={"commit:" + c.sha} addDel={f && <AddDel add={f.add} del={f.del} gap={6} />} />
          : <div style={{ display: "grid", placeItems: "center", color: "var(--ink-4)", background: "var(--bg)" }}>select a file</div>}
      </div>
    </div>
  );
}

function AgentRangeDiffView({ ag, shas }) {
  const history = useMemoAG(() => AGG().agentHistory(ag), [ag.id]);
  const commits = shas.map((s) => history.find((c) => c.sha === s)).filter(Boolean);
  const files = useMemoAG(() => {
    const m = {};
    commits.forEach((c) => c.files.forEach((f) => {
      const e = m[f.path] || { path: f.path, add: 0, del: 0, state: f.state };
      e.add += f.add; e.del += f.del; if (f.state === "A" && e.state !== "A") e.state = "M";
      m[f.path] = e;
    }));
    return Object.values(m).sort((a, b) => a.path.localeCompare(b.path));
  }, [shas.join()]);
  const [selPath, setSelPath] = useStateAG(files[0] ? files[0].path : null);
  const f = files.find((x) => x.path === selPath);
  const diff = f ? synthDiff(f) : null;
  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", minHeight: 0, background: "var(--panel-2)" }}>
      <CommitContextHeader left={<Icon name="diff" size="sm" style={{ color: "var(--accent)" }} />} title={"Range diff · " + commits.length + " commits"}
        sub={commits.map((c, i) => <React.Fragment key={c.sha}>{i > 0 && <span style={{ color: "var(--ink-4)" }}>·</span>}<span className="chip" style={{ fontSize: 9, padding: "1px 6px" }}>{c.sha}</span></React.Fragment>)} />
      <div style={{ flex: 1, display: "grid", gridTemplateColumns: "232px 1fr", minHeight: 0 }}>
        <div style={{ minHeight: 0, borderRight: "1px solid var(--hair)", background: "var(--panel)" }}>
          <DiffFileList files={files} selPath={selPath} onSelect={setSelPath} title="Range files" />
        </div>
        {selPath ? <DiffOrBlame key={selPath} ag={ag} path={selPath} diff={diff} view={"range:" + shas.join("+")} addDel={f && <AddDel add={f.add} del={f.del} gap={6} />} />
          : <div style={{ display: "grid", placeItems: "center", color: "var(--ink-4)", background: "var(--bg)" }}>select a file</div>}
      </div>
    </div>
  );
}

// ====================== center dispatcher =================================
function AgentGitView({ ag, project, gitView, onFlash, onOpenCommit }) {
  if (gitView.kind === "commit") return <AgentCommitDiffView ag={ag} sha={gitView.sha} initPath={gitView.path} />;
  if (gitView.kind === "range") return <AgentRangeDiffView ag={ag} shas={gitView.shas} />;
  if (gitView.kind === "filehistory") return <FileHistoryView fh={AGG().agentFileHistory(ag, gitView.path)} />;
  if (gitView.kind === "conflict") return <ConflictView session={AGG().agentConflict(ag, project)} onFlash={onFlash} />;
  return null;
}

Object.assign(window, { FileBlameGutter, AgentCommitHistory, AgentConflictCard, AgentGitView });
