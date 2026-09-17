/* global React, Icon, StatusDot, StatusPill, STATUS_META, ToolBadge, toolMeta, fmtDur, fileName, fileDir, logColor, logPrefix, getFileLines, highlight, langOf, buildTree, DIFFS, FileBlameGutter, AgentGitView, HunkRows, AddDel */
// ORCHESTRA center — single-agent workspace: diff / file viewer / terminal

const { useState: useStateW, useRef: useRefW, useEffect: useEffectW } = React;

function Terminal({ ag, lines, streaming }) {
  const ref = useRefW(null);
  useEffectW(() => { if (ref.current) ref.current.scrollTop = ref.current.scrollHeight; }, [lines]);
  return (
    <div ref={ref} className="scroll-y" style={{ flex: 1, background: "var(--bg)", padding: "12px 16px", fontSize: 12, lineHeight: 1.7 }}>
      <div style={{ color: "var(--ink-4)", marginBottom: 8, fontSize: 10.5 }}>── session: {ag.worktree} · {ag.branch} ──</div>
      {lines.map((l, i) => (
        <div key={i} style={{ display: "flex", gap: 9, color: logColor(l.t), whiteSpace: "pre-wrap", wordBreak: "break-word" }}>
          <span style={{ color: l.t === "cmd" ? "var(--accent)" : "var(--ink-4)", flex: "none", userSelect: "none", width: 10, textAlign: "center" }}>{logPrefix(l.t)}</span>
          <span style={{ flex: 1 }}>{l.s}</span>
        </div>
      ))}
      {streaming && (
        <div style={{ display: "flex", gap: 9, color: "var(--accent)" }}>
          <span style={{ width: 10, textAlign: "center", color: "var(--accent)" }}>$</span>
          <span className="caret" />
        </div>
      )}
    </div>
  );
}

// ---------- file viewer (line-numbered + blame/annotate gutter) ----------
function FileViewer({ path, agent, onOpenCommit }) {
  const [blame, setBlame] = useStateW(false);
  const lines = getFileLines(path, agent);
  const lang = langOf(path);
  const changed = (agent.files || []).find((f) => f.path === path);
  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", minHeight: 0, background: "var(--bg)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 14px", background: "var(--panel)", borderBottom: "1px solid var(--hair)", fontSize: 11.5, flex: "none" }}>
        <Icon name="file" size="sm" style={{ color: changed ? (changed.state === "A" ? "var(--code-add-ink)" : "var(--accent-2)") : "var(--ink-3)" }} />
        <span style={{ color: "var(--ink-4)" }}>{fileDir(path)}</span><span style={{ marginLeft: -6 }}>{fileName(path)}</span>
        {changed && <span className="chip" style={{ fontSize: 9, padding: "1px 6px", color: changed.state === "A" ? "var(--code-add-ink)" : "var(--accent-2)" }}>{changed.state === "A" ? "added" : "modified"} +{changed.add}{changed.del ? " −" + changed.del : ""}</span>}
        <button className={"btn " + (blame ? "" : "ghost-hair")} onClick={() => setBlame((v) => !v)} title="Toggle blame / annotate"
          style={{ marginLeft: "auto", padding: "4px 9px", fontSize: 10.5, color: blame ? "var(--ink)" : "var(--ink-3)",
            background: blame ? "color-mix(in oklch, var(--accent), transparent 86%)" : "transparent",
            border: "1px solid " + (blame ? "color-mix(in oklch, var(--accent), transparent 60%)" : "var(--hair)") }}>
          <Icon name="git" size="sm" style={{ color: blame ? "var(--accent)" : "inherit" }} />Annotate
        </button>
        <span className="chip" style={{ fontSize: 9.5 }}>{lang}</span>
        <span className="tnum" style={{ fontSize: 9.5, color: "var(--ink-4)" }}>{lines.length} lines</span>
      </div>
      {blame
        ? <FileBlameGutter agent={agent} path={path} onOpenCommit={onOpenCommit} />
        : <div className="scroll-y" style={{ flex: 1 }}>
            <pre style={{ margin: 0, fontFamily: "var(--font-mono)", fontSize: 12, lineHeight: 1.65 }}>
              {lines.map((ln, i) => (
                <div key={i} style={{ display: "flex", background: ln.add ? "var(--code-add-bg)" : "transparent" }}>
                  <span className="tnum" style={{ width: 48, flex: "none", textAlign: "right", padding: "0 12px 0 0", color: "var(--ink-4)", userSelect: "none" }}>{ln.n}</span>
                  {ln.add && <span style={{ width: 14, flex: "none", textAlign: "center", color: "var(--code-add-ink)", userSelect: "none" }}>+</span>}
                  <code style={{ flex: 1, whiteSpace: "pre-wrap", wordBreak: "break-word", paddingRight: 16, color: "var(--ink-2)", paddingLeft: ln.add ? 0 : 14 }}>{highlight(ln.s, lang)}</code>
                </div>
              ))}
            </pre>
          </div>}
    </div>
  );
}

// ---------- changed-files list (flat or tree), selectable ----------
function StateBadge({ state }) {
  return (
    <span style={{ flex: "none", width: 14, height: 14, borderRadius: 3, display: "grid", placeItems: "center", fontSize: 9, fontWeight: 700,
      color: state === "A" ? "var(--code-add-ink)" : state === "D" ? "var(--code-del-ink)" : "var(--accent-2)",
      background: state === "A" ? "var(--code-add-bg)" : state === "D" ? "var(--code-del-bg)" : "color-mix(in oklch, var(--accent-2), transparent 86%)" }}>
      {state}
    </span>
  );
}

function TreeRows({ nodes, depth, selPath, onSelect, fileMap }) {
  return nodes.map((node) => {
    const pad = 8 + depth * 13;
    if (node.dir) {
      return (
        <div key={node.path}>
          <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "4px 8px", paddingLeft: pad }}>
            <Icon name="folderOpen" size="sm" style={{ width: 13, height: 13, color: "var(--ink-4)", flex: "none" }} />
            <span style={{ fontSize: 11, color: "var(--ink-3)" }}>{node.name}</span>
          </div>
          <TreeRows nodes={node.children} depth={depth + 1} selPath={selPath} onSelect={onSelect} fileMap={fileMap} />
        </div>
      );
    }
    const f = fileMap[node.path];
    const active = selPath === node.path;
    return (
      <div key={node.path} onClick={() => onSelect(node.path)}
        style={{ display: "flex", alignItems: "center", gap: 7, padding: "4px 10px", paddingLeft: pad + 4, cursor: "pointer", margin: "1px 6px", borderRadius: "var(--r-sm)", background: active ? "var(--panel-3)" : "transparent" }}
        onMouseEnter={(e) => { if (!active) e.currentTarget.style.background = "var(--panel-2)"; }}
        onMouseLeave={(e) => { if (!active) e.currentTarget.style.background = "transparent"; }}>
        <StateBadge state={node.state} />
        <span style={{ flex: 1, fontSize: 11.5, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", color: active ? "var(--ink)" : "var(--ink-2)" }}>{node.name}</span>
        {f && <span className="tnum" style={{ fontSize: 9.5, display: "flex", gap: 4, flex: "none" }}>
          <span style={{ color: "var(--code-add-ink)" }}>+{f.add}</span>{f.del > 0 && <span style={{ color: "var(--code-del-ink)" }}>−{f.del}</span>}
        </span>}
      </div>
    );
  });
}

function ChangedFiles({ files, selPath, onSelect, mode }) {
  if (mode === "tree") {
    const stateMap = {}; const fileMap = {};
    files.forEach((f) => { stateMap[f.path] = f.state; fileMap[f.path] = f; });
    const tree = buildTree(files.map((f) => f.path), stateMap);
    return <TreeRows nodes={tree} depth={0} selPath={selPath} onSelect={onSelect} fileMap={fileMap} />;
  }
  return files.map((f) => {
    const active = selPath === f.path;
    return (
      <div key={f.path} onClick={() => onSelect(f.path)}
        style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 12px", cursor: "pointer", margin: "1px 6px", borderRadius: "var(--r-sm)", background: active ? "var(--panel-3)" : "transparent" }}
        onMouseEnter={(e) => { if (!active) e.currentTarget.style.background = "var(--panel-2)"; }}
        onMouseLeave={(e) => { if (!active) e.currentTarget.style.background = "transparent"; }}>
        <StateBadge state={f.state} />
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ fontSize: 11.5, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", color: active ? "var(--ink)" : "var(--ink)" }}>{fileName(f.path)}</div>
          {fileDir(f.path) && <div style={{ fontSize: 9.5, color: "var(--ink-4)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{fileDir(f.path)}</div>}
        </div>
        <span className="tnum" style={{ fontSize: 9.5, display: "flex", gap: 4, flex: "none" }}>
          <span style={{ color: "var(--code-add-ink)" }}>+{f.add}</span>{f.del > 0 && <span style={{ color: "var(--code-del-ink)" }}>−{f.del}</span>}
        </span>
      </div>
    );
  });
}

// hunk/line selection over the agent's primary-file diff → partial commit/discard
function HunkSelectDiff({ diff, ag, onAct }) {
  const hi = (h) => diff.hunks.indexOf(h);
  const changedOf = (h) => h.lines.map((ln, li) => ({ ln, li })).filter((x) => x.ln.k !== " ");
  const [sel, setSel] = useStateW(() => { const s = {}; diff.hunks.forEach((h, j) => h.lines.forEach((ln, li) => { if (ln.k !== " ") s[j + ":" + li] = true; })); return s; });
  const hunkState = (h) => { const ch = changedOf(h); const on = ch.filter((x) => sel[hi(h) + ":" + x.li]).length; return on === 0 ? "none" : on === ch.length ? "all" : "some"; };
  const toggleLine = (h, li) => setSel((s) => ({ ...s, [hi(h) + ":" + li]: !s[hi(h) + ":" + li] }));
  const toggleHunk = (h) => { const want = hunkState(h) !== "all"; setSel((s) => { const n = { ...s }; changedOf(h).forEach((x) => { n[hi(h) + ":" + x.li] = want; }); return n; }); };
  let lineN = 0, hunkN = 0;
  diff.hunks.forEach((h) => { const on = changedOf(h).filter((x) => sel[hi(h) + ":" + x.li]).length; if (on) { hunkN++; lineN += on; } });
  const lead = {
    hunkHead: (h) => { const st = hunkState(h); return (
      <button onClick={(e) => { e.stopPropagation(); toggleHunk(h); }} title="Select hunk"
        style={{ flex: "none", width: 15, height: 15, marginRight: 9, borderRadius: 4, display: "grid", placeItems: "center", cursor: "pointer", border: "1px solid " + (st === "none" ? "var(--hair-2)" : "var(--accent)"), background: st === "all" ? "var(--accent)" : st === "some" ? "color-mix(in oklch, var(--accent), transparent 55%)" : "transparent" }}>
        {st === "all" && <Icon name="check" size="sm" style={{ width: 10, height: 10, color: "#06070b" }} />}
        {st === "some" && <span style={{ width: 7, height: 2, borderRadius: 2, background: "#06070b" }} />}
      </button>); },
    lineHead: (h, ln, li) => { if (ln.k === " ") return <span style={{ width: 24, flex: "none" }} />; const on = sel[hi(h) + ":" + li]; return (
      <button onClick={(e) => { e.stopPropagation(); toggleLine(h, li); }}
        style={{ flex: "none", width: 14, height: 14, margin: "3px 10px 0 8px", borderRadius: 3, display: "grid", placeItems: "center", cursor: "pointer", border: "1px solid " + (on ? "var(--accent)" : "var(--hair-2)"), background: on ? "var(--accent)" : "transparent" }}>
        {on && <Icon name="check" size="sm" style={{ width: 9, height: 9, color: "#06070b" }} />}
      </button>); },
  };
  const lineStyle = (h, ln, li) => { if (ln.k === " ") return "transparent"; return sel[hi(h) + ":" + li] ? (ln.k === "+" ? "var(--code-add-bg)" : "var(--code-del-bg)") : "transparent"; };
  return (
    <div style={{ display: "flex", flexDirection: "column", minHeight: 0, flex: 1, background: "var(--bg)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 14px", background: "var(--panel)", borderBottom: "1px solid var(--hair)", fontSize: 11.5, flex: "none" }}>
        <Icon name="file" size="sm" style={{ color: "var(--ink-3)" }} />
        <span style={{ color: "var(--ink-4)" }}>{fileDir(diff.file)}</span><span style={{ marginLeft: -6 }}>{fileName(diff.file)}</span>
        <span className="chip" style={{ fontSize: 9, padding: "1px 6px", color: "var(--accent)", borderColor: "color-mix(in oklch, var(--accent), transparent 55%)" }}>partial commit</span>
        <span className="chip" style={{ marginLeft: "auto", fontSize: 9.5 }}>{diff.lang}</span>
      </div>
      <div className="scroll-y" style={{ flex: 1, minHeight: 0 }}>
        <HunkRows hunks={diff.hunks} lang={diff.lang} lead={lead} lineStyle={lineStyle} onLineClick={(h, ln, li) => ln.k !== " " && toggleLine(h, li)} />
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "10px 14px", borderTop: "1px solid var(--hair)", background: "var(--panel)", flex: "none" }}>
        <span style={{ fontSize: 11, color: "var(--ink)" }}><b className="tnum" style={{ color: "var(--accent)" }}>{hunkN}</b> {hunkN === 1 ? "hunk" : "hunks"} selected</span>
        <span className="tnum" style={{ fontSize: 10, color: "var(--ink-4)" }}>{lineN} lines</span>
        <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
          <button className="btn ghost-hair" disabled={!hunkN} onClick={() => onAct(ag.id, "discard")} style={{ color: "var(--st-blocked)" }}><Icon name="discard" size="sm" />Discard selected</button>
          <button className="btn primary" disabled={!hunkN} onClick={() => onAct(ag.id, "commit")}><Icon name="commit" size="sm" />Commit {hunkN} {hunkN === 1 ? "hunk" : "hunks"}</button>
        </div>
      </div>
    </div>
  );
}

function DiffBody({ ag, path, partial, onAct, onOpenCommit }) {
  const diff = DIFFS[ag.id];
  // show the rich hunk diff for the agent's primary file; otherwise the file view
  if (diff && diff.file === path) {
    if (partial) return <HunkSelectDiff diff={diff} ag={ag} onAct={onAct} />;
    return (
      <div className="scroll-y" style={{ background: "var(--bg)", flex: 1 }}>
        <div style={{ position: "sticky", top: 0, display: "flex", alignItems: "center", gap: 8, padding: "8px 14px", background: "var(--panel)", borderBottom: "1px solid var(--hair)", fontSize: 11.5, zIndex: 1 }}>
          <Icon name="file" size="sm" style={{ color: "var(--ink-3)" }} />
          <span style={{ color: "var(--ink-4)" }}>{fileDir(diff.file)}</span><span style={{ marginLeft: -6 }}>{fileName(diff.file)}</span>
          <span className="chip" style={{ marginLeft: "auto", fontSize: 9.5 }}>{diff.lang}</span>
        </div>
        {diff.hunks.map((h, hi) => (
          <div key={hi}>
            <div style={{ padding: "5px 14px", fontSize: 11, color: "var(--accent-2)", background: "color-mix(in oklch, var(--accent-2), transparent 93%)" }}>{h.meta}</div>
            {h.lines.map((ln, li) => (
              <div key={li} style={{ display: "flex", fontSize: 12, lineHeight: 1.7, background: ln.k === "+" ? "var(--code-add-bg)" : ln.k === "-" ? "var(--code-del-bg)" : "transparent" }}>
                <span className="tnum" style={{ width: 44, flex: "none", textAlign: "right", padding: "0 10px 0 0", color: "var(--ink-4)", userSelect: "none" }}>{ln.n}</span>
                <span style={{ width: 16, flex: "none", textAlign: "center", userSelect: "none", color: ln.k === "+" ? "var(--code-add-ink)" : ln.k === "-" ? "var(--code-del-ink)" : "var(--ink-4)" }}>{ln.k}</span>
                <span style={{ flex: 1, whiteSpace: "pre-wrap", wordBreak: "break-word", paddingRight: 14, color: ln.k === "+" ? "var(--code-add-ink)" : ln.k === "-" ? "var(--code-del-ink)" : "var(--ink-2)" }}>{ln.s || " "}</span>
              </div>
            ))}
          </div>
        ))}
      </div>
    );
  }
  return <FileViewer path={path} agent={ag} onOpenCommit={onOpenCommit} />;
}

// conflicted-file row inside the Diff pane's file list
function ConflictFileRow({ f, rc, active, onSelect }) {
  const done = rc === f.conflicts;
  return (
    <div onClick={() => onSelect(f.path)}
      style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 12px", cursor: "pointer", margin: "1px 6px", borderRadius: "var(--r-sm)",
        background: active ? "var(--panel-3)" : "transparent", boxShadow: active ? "inset 0 0 0 1px color-mix(in oklch, var(--st-blocked), transparent 55%)" : "none" }}
      onMouseEnter={(e) => { if (!active) e.currentTarget.style.background = "var(--panel-2)"; }}
      onMouseLeave={(e) => { if (!active) e.currentTarget.style.background = "transparent"; }}>
      <span style={{ flex: "none", width: 15, height: 15, borderRadius: "50%", display: "grid", placeItems: "center",
        background: done ? "var(--st-done)" : "color-mix(in oklch, var(--st-blocked), transparent 80%)", border: done ? "none" : "1px solid color-mix(in oklch, var(--st-blocked), transparent 50%)" }}>
        {done ? <Icon name="check" size="sm" style={{ width: 10, height: 10, color: "#06070b" }} /> : <span style={{ width: 5, height: 5, borderRadius: "50%", background: "var(--st-blocked)" }} />}
      </span>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: 11.5, color: active ? "var(--ink)" : "var(--ink-2)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{fileName(f.path)}</div>
        {fileDir(f.path) && <div style={{ fontSize: 9.5, color: "var(--ink-4)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{fileDir(f.path)}</div>}
      </div>
      <span className="tnum" style={{ fontSize: 9.5, color: done ? "var(--st-done)" : "var(--st-blocked)", flex: "none" }}>{rc}/{f.conflicts}</span>
    </div>
  );
}

function DiffView({ ag, project, treeMode, setTreeMode, onAct, onOpenCommit, onFlash }) {
  const conflict = (window.AGENT_GIT && window.AGENT_GIT.agentHasConflict(ag)) ? window.AGENT_GIT.agentConflict(ag, project) : null;
  const conflictPaths = conflict ? conflict.files.map((f) => f.path) : [];
  const [resMap, setResMap] = useStateW({});
  const [selPath, setSelPath] = useStateW(conflict ? conflict.files[0].path : (ag.files[0] ? ag.files[0].path : null));
  const [partial, setPartial] = useStateW(false);
  useEffectW(() => {
    setResMap({}); setPartial(false);
    setSelPath(conflict ? conflict.files[0].path : (ag.files[0] ? ag.files[0].path : null));
  }, [ag.id]);

  const isConflictSel = conflict && conflictPaths.includes(selPath);
  const hasHunks = !isConflictSel && DIFFS[ag.id] && DIFFS[ag.id].file === selPath;

  const resolvedCount = (f) => { const rm = resMap[f.path] || {}; return f.segments.filter((s, i) => s.type === "conflict" && rm[i] && rm[i].res).length; };
  const totalConf = conflict ? conflict.files.reduce((s, f) => s + f.conflicts, 0) : 0;
  const totalResolved = conflict ? conflict.files.reduce((s, f) => s + resolvedCount(f), 0) : 0;
  const allDone = conflict && totalResolved === totalConf;
  const resolve = (path, idx, res) => setResMap((m) => ({ ...m, [path]: { ...(m[path] || {}), [idx]: { res, custom: null } } }));
  const editResult = (path, idx, custom) => setResMap((m) => ({ ...m, [path]: { ...(m[path] || {}), [idx]: { ...(m[path] || {})[idx], custom } } }));
  const acceptAll = (path, side, idxs) => { setResMap((m) => { const fm = { ...(m[path] || {}) }; idxs.forEach((ci) => { fm[ci] = { res: side, custom: null }; }); return { ...m, [path]: fm }; }); onFlash && onFlash("accepted " + side + " — " + fileName(path)); };
  const activeConflictFile = isConflictSel ? conflict.files.find((f) => f.path === selPath) : null;

  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", minHeight: 0 }}>
      {/* merge banner — only when this branch has an in-progress conflicted merge */}
      {conflict && (
        <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "8px 16px", flex: "none",
          background: "color-mix(in oklch, var(--st-blocked), transparent 91%)", borderBottom: "1px solid color-mix(in oklch, var(--st-blocked), transparent 60%)" }}>
          <Icon name="merge" size="sm" style={{ color: "var(--st-blocked)" }} />
          <span style={{ fontSize: 11.5, fontWeight: 600, color: "var(--ink)" }}>Merge {conflict.theirs} → {conflict.ours}</span>
          <span style={{ fontSize: 10.5, color: allDone ? "var(--st-done)" : "var(--st-blocked)" }}>{allDone ? "all conflicts resolved" : "paused — resolve conflicts below"}</span>
          <div className="meter" style={{ width: 120, marginLeft: 4 }}><i style={{ width: (totalResolved / totalConf * 100) + "%", background: allDone ? "var(--st-done)" : "linear-gradient(90deg, var(--accent), var(--accent-2))" }} /></div>
          <span className="tnum" style={{ fontSize: 10, color: "var(--ink-4)" }}>{totalResolved}/{totalConf}</span>
          <div style={{ marginLeft: "auto", display: "flex", gap: 7 }}>
            <button className={"btn " + (allDone ? "primary" : "ghost-hair")} disabled={!allDone} onClick={() => onFlash && onFlash("committed merge — " + conflict.theirs + " → " + conflict.ours)} style={{ padding: "4px 11px", fontSize: 11 }}>
              <Icon name="commit" size="sm" />Commit merge
            </button>
            <button className="btn ghost-hair" onClick={() => onFlash && onFlash("aborted merge — working tree restored")} style={{ padding: "4px 10px", fontSize: 11, color: "var(--st-blocked)" }}>
              <Icon name="discard" size="sm" />Abort
            </button>
          </div>
        </div>
      )}

      <div style={{ flex: 1, display: "grid", gridTemplateColumns: "246px 1fr", minHeight: 0 }}>
        <div style={{ display: "flex", flexDirection: "column", minHeight: 0, borderRight: "1px solid var(--hair)", background: "var(--panel)" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 10px 8px 14px", borderBottom: "1px solid var(--hair)", flex: "none" }}>
            <span className="up" style={{ fontSize: 9, color: "var(--ink-3)" }}>Files</span>
            {/* tree / flat toggle */}
            <div style={{ marginLeft: "auto", display: "flex", gap: 2, padding: 2, background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-sm)" }}>
              {[{ k: "tree", icon: "graph" }, { k: "flat", icon: "dots" }].map((o) => (
                <button key={o.k} className="btn" onClick={() => setTreeMode(o.k === "tree")} title={o.k === "tree" ? "Tree view" : "Flattened view"}
                  style={{ padding: "3px 7px", borderRadius: 4, gap: 5, fontSize: 10,
                    background: (treeMode === (o.k === "tree")) ? "var(--panel-3)" : "transparent",
                    color: (treeMode === (o.k === "tree")) ? "var(--ink)" : "var(--ink-3)",
                    boxShadow: (treeMode === (o.k === "tree")) ? "0 0 0 1px var(--hair-2)" : "none" }}>
                  <Icon name={o.icon} size="sm" style={{ width: 12, height: 12, color: (treeMode === (o.k === "tree")) ? "var(--accent)" : "inherit" }} />
                  {o.k === "tree" ? "Tree" : "Flat"}
                </button>
              ))}
            </div>
          </div>
          {/* partial-commit toggle */}
          <button className="btn" onClick={() => setPartial((v) => !v)} disabled={!hasHunks} title={hasHunks ? "Select hunks & lines to commit" : "Open the diffed file to select hunks"}
            style={{ margin: "8px 10px 4px", justifyContent: "center", fontSize: 10.5, borderRadius: "var(--r-sm)", flex: "none",
              border: "1px solid " + (partial ? "color-mix(in oklch, var(--accent), transparent 55%)" : "var(--hair)"),
              color: partial ? "var(--ink)" : "var(--ink-3)", background: partial ? "color-mix(in oklch, var(--accent), transparent 88%)" : "transparent", opacity: hasHunks ? 1 : 0.5 }}>
            <Icon name="stage" size="sm" style={{ color: partial ? "var(--accent)" : "inherit" }} />{partial ? "Selecting hunks" : "Partial commit"}
          </button>
          <div className="scroll-y" style={{ flex: 1, padding: "6px 0" }}>
            {/* conflicted files first, when a merge is in progress */}
            {conflict && <>
              <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "4px 14px 4px" }}>
                <Icon name="merge" size="sm" style={{ width: 12, height: 12, color: "var(--st-blocked)" }} />
                <span className="up" style={{ fontSize: 9, color: "var(--st-blocked)" }}>Conflicts · {conflict.files.length}</span>
                <span className="tnum" style={{ fontSize: 9, color: "var(--ink-4)", marginLeft: "auto" }}>{totalConf - totalResolved} left</span>
              </div>
              {conflict.files.map((f) => <ConflictFileRow key={f.path} f={f} rc={resolvedCount(f)} active={selPath === f.path} onSelect={setSelPath} />)}
              <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "10px 14px 4px", borderTop: "1px solid var(--hair)", marginTop: 4 }}>
                <span className="up" style={{ fontSize: 9, color: "var(--ink-3)" }}>Changed · {ag.files.length}</span>
              </div>
            </>}
            {ag.files.length ? <ChangedFiles files={ag.files} selPath={selPath} onSelect={setSelPath} mode={treeMode ? "tree" : "flat"} />
              : <div style={{ padding: 16, fontSize: 11, color: "var(--ink-4)" }}>no changes yet</div>}
          </div>
        </div>
        {isConflictSel
          ? <ConflictFilePane file={activeConflictFile} fileRes={resMap[selPath] || {}} ours={conflict.ours} theirs={conflict.theirs}
              onResolve={(i, r) => resolve(selPath, i, r)} onEdit={(i, c) => editResult(selPath, i, c)} onAcceptAll={(side, idxs) => acceptAll(selPath, side, idxs)} />
          : selPath
          ? <DiffBody ag={ag} path={selPath} partial={partial && hasHunks} onAct={onAct} onOpenCommit={onOpenCommit} />
          : <div style={{ display: "grid", placeItems: "center", color: "var(--ink-4)", fontSize: 12 }}>working tree clean</div>}
      </div>
    </div>
  );
}

function Workspace({ ag, project, liveLogs, onAct, onFlash, openFile, onCloseFile, initialPane, gitView, onCloseGitView, onOpenGit }) {
  const [pane, setPane] = useStateW(initialPane || "diff");
  const [treeMode, setTreeMode] = useStateW(false);
  // when a file is opened from the right panel, jump to the file pane
  useEffectW(() => { if (openFile) setPane("file"); }, [openFile]);
  useEffectW(() => { if (!openFile && pane === "file") setPane("diff"); }, [openFile]);
  // when a git view (commit/range/conflict/file-history) is opened, jump to it
  useEffectW(() => { if (gitView) setPane("git"); }, [gitView]);
  useEffectW(() => { if (!gitView && pane === "git") setPane("diff"); }, [gitView]);

  const onOpenCommit = (sha) => onOpenGit && onOpenGit(ag.id, { kind: "commit", sha });

  const PANES = [
    { key: "diff", icon: "diff", label: "Diff", badge: ag.files.length },
    { key: "terminal", icon: "terminal", label: "Terminal" },
  ];
  const gitLabel = gitView ? (gitView.kind === "commit" ? gitView.sha : gitView.kind === "range" ? "range (" + gitView.shas.length + ")" : gitView.kind === "filehistory" ? fileName(gitView.path) : "conflicts") : "";
  const gitIcon = gitView ? (gitView.kind === "conflict" ? "merge" : gitView.kind === "filehistory" ? "clock" : gitView.kind === "range" ? "diff" : "commit") : "git";

  return (
    <div style={{ display: "flex", flexDirection: "column", minHeight: 0, background: "var(--panel-2)" }}>
      {/* agent header */}
      <div style={{ padding: "12px 18px", borderBottom: "1px solid var(--hair)", background: "var(--panel)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
          <h1 className="disp" style={{ fontSize: 15, fontWeight: 600 }}>{ag.name}</h1>
          <StatusPill status={ag.status} filled />
          {ag.status === "running" && <div className="activity" style={{ width: 60 }} />}
          <div style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
            {ag.status === "running"
              ? <button className="btn ghost-hair" onClick={() => onAct(ag.id, "pause")}><Icon name="pause" size="sm" />Pause</button>
              : ag.status === "done" ? null
              : <button className="btn ghost-hair" onClick={() => onAct(ag.id, "resume")}><Icon name="play" size="sm" />Resume</button>}
            <button className="btn ghost-hair" onClick={() => onAct(ag.id, "commit")}><Icon name="commit" size="sm" />Commit</button>
            <button className={"btn " + (ag.status === "done" ? "primary" : "ghost-hair")} onClick={() => onAct(ag.id, "merge")}>
              <Icon name="merge" size="sm" />Merge to {project ? project.branch : "main"}
            </button>
          </div>
        </div>
        <div style={{ marginTop: 8, fontSize: 11, color: "var(--ink-2)" }}>{ag.task}</div>
        <div style={{ display: "flex", alignItems: "center", gap: 14, marginTop: 8, fontSize: 10.5, color: "var(--ink-3)", flexWrap: "wrap" }} className="tnum">
          {project && <span style={{ display: "flex", gap: 5, alignItems: "center", color: project.color }}><Icon name={project.icon} size="sm" style={{ width: 12, height: 12 }} />{project.name}</span>}
          <span style={{ display: "flex", gap: 5 }}><Icon name="branch" size="sm" style={{ width: 12, height: 12, color: "var(--accent-2)" }} />{ag.branch}</span>
          <span style={{ display: "flex", gap: 5 }}><Icon name="folder" size="sm" style={{ width: 12, height: 12 }} />{ag.worktree}</span>
          <span style={{ display: "flex", gap: 5, alignItems: "center" }}><ToolBadge tool={ag.tool} size={13} />{toolMeta(ag.tool).name} · {ag.model}{ag.effort ? " · " + ag.effort : ""}</span>
          <span style={{ display: "flex", gap: 5 }}><Icon name="commit" size="sm" style={{ width: 12, height: 12 }} />{ag.commits} commits</span>
          <span style={{ display: "flex", gap: 5 }}><Icon name="clock" size="sm" style={{ width: 12, height: 12 }} />{ag.elapsed ? fmtDur(ag.elapsed) : "—"}</span>
        </div>
      </div>

      {/* pane tabs */}
      <div style={{ display: "flex", alignItems: "stretch", gap: 2, padding: "0 12px", background: "var(--panel)", borderBottom: "1px solid var(--hair)" }}>
        {PANES.map((p) => (
          <button key={p.key} className="btn" onClick={() => setPane(p.key)}
            style={{ padding: "9px 12px", borderRadius: 0, position: "relative", color: pane === p.key ? "var(--ink)" : "var(--ink-3)" }}>
            {pane === p.key && <span style={{ position: "absolute", left: 10, right: 10, bottom: 0, height: 2, background: "linear-gradient(90deg, var(--accent), var(--accent-2))" }} />}
            <Icon name={p.icon} size="sm" style={{ color: pane === p.key ? "var(--accent)" : "inherit" }} />
            {p.label}
            {p.badge != null && <span className="chip tnum" style={{ fontSize: 9, padding: "0 5px", color: "var(--ink-2)", borderColor: "var(--hair)" }}>{p.badge}</span>}
          </button>
        ))}
        {/* dynamic git-inspection tab */}
        {gitView && (
          <div onClick={() => setPane("git")} style={{ display: "flex", alignItems: "center", gap: 7, padding: "9px 10px 9px 12px", cursor: "pointer", position: "relative", color: pane === "git" ? "var(--ink)" : "var(--ink-3)", borderLeft: "1px solid var(--hair)" }}>
            {pane === "git" && <span style={{ position: "absolute", left: 8, right: 8, bottom: 0, height: 2, background: "linear-gradient(90deg, var(--accent), var(--accent-2))" }} />}
            <Icon name={gitIcon} size="sm" style={{ color: pane === "git" ? "var(--accent)" : (gitView.kind === "conflict" ? "var(--st-blocked)" : "inherit") }} />
            <span style={{ fontSize: 12 }} className={gitView.kind === "commit" ? "tnum" : ""}>{gitLabel}</span>
            <button onClick={(e) => { e.stopPropagation(); onCloseGitView(); }}
              style={{ background: "transparent", border: "none", color: "var(--ink-4)", cursor: "pointer", display: "flex", padding: 1, borderRadius: 3 }}
              onMouseEnter={(e) => e.currentTarget.style.color = "var(--ink)"}
              onMouseLeave={(e) => e.currentTarget.style.color = "var(--ink-4)"}>
              <Icon name="x" size="sm" />
            </button>
          </div>
        )}
        {/* dynamic file tab */}
        {openFile && (
          <div onClick={() => setPane("file")} style={{ display: "flex", alignItems: "center", gap: 7, padding: "9px 10px 9px 12px", cursor: "pointer", position: "relative", color: pane === "file" ? "var(--ink)" : "var(--ink-3)", borderLeft: "1px solid var(--hair)" }}>
            {pane === "file" && <span style={{ position: "absolute", left: 8, right: 8, bottom: 0, height: 2, background: "linear-gradient(90deg, var(--accent), var(--accent-2))" }} />}
            <Icon name="file" size="sm" style={{ color: pane === "file" ? "var(--accent)" : "inherit" }} />
            <span style={{ fontSize: 12 }}>{fileName(openFile)}</span>
            <button onClick={(e) => { e.stopPropagation(); onCloseFile(); }}
              style={{ background: "transparent", border: "none", color: "var(--ink-4)", cursor: "pointer", display: "flex", padding: 1, borderRadius: 3 }}
              onMouseEnter={(e) => e.currentTarget.style.color = "var(--ink)"}
              onMouseLeave={(e) => e.currentTarget.style.color = "var(--ink-4)"}>
              <Icon name="x" size="sm" />
            </button>
          </div>
        )}
      </div>

      {/* pane body */}
      {pane === "diff" && <DiffView ag={ag} project={project} treeMode={treeMode} setTreeMode={setTreeMode} onAct={onAct} onOpenCommit={onOpenCommit} onFlash={onFlash} />}
      {pane === "terminal" && <Terminal ag={ag} lines={liveLogs} streaming={ag.status === "running"} />}
      {pane === "git" && gitView && <AgentGitView ag={ag} project={project} gitView={gitView} onFlash={onFlash || (() => {})} onOpenCommit={onOpenCommit} />}
      {pane === "file" && openFile && <FileViewer path={openFile} agent={ag} onOpenCommit={onOpenCommit} />}
    </div>
  );
}

Object.assign(window, { Workspace, FileViewer, Terminal, DiffView });
