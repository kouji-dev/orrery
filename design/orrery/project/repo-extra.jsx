/* global React, Icon, fileName, fileDir, AuthorAvatar, authorName, Sha, AddDel, RStateBadge, HunkRows, FileDiff, DiffFileHeader, synthDiff */
// Orrery — Repository ▸ File history (Feature 5) and Partial commit (Feature 6)

const { useState: useStateE, useMemo: useMemoE } = React;
const GE = () => window.GIT;

// =========================================================================
// Feature 5 — file history: commits that touched a file; pick any two to diff
// =========================================================================
function RevRow({ r, role, onClick }) {
  const a = GE().authors[r.author] || {};
  const tint = role === "base" ? "var(--accent)" : role === "compare" ? "var(--accent-2)" : null;
  return (
    <div onClick={onClick}
      style={{ position: "relative", display: "flex", gap: 9, padding: "9px 12px 9px 10px", cursor: "pointer", borderRadius: "var(--r-sm)", margin: "1px 6px",
        background: role ? "color-mix(in oklch, " + tint + ", transparent 90%)" : "transparent",
        boxShadow: role ? "inset 0 0 0 1px " + tint : "none" }}
      onMouseEnter={(e) => { if (!role) e.currentTarget.style.background = "var(--panel-2)"; }}
      onMouseLeave={(e) => { if (!role) e.currentTarget.style.background = "transparent"; }}>
      <span style={{ flex: "none", width: 18, display: "grid", placeItems: "center", paddingTop: 1 }}>
        {role
          ? <span className="up" style={{ fontSize: 8, fontWeight: 700, color: tint }}>{role === "base" ? "A" : "B"}</span>
          : <span style={{ width: 9, height: 9, borderRadius: "50%", background: "var(--panel)", border: "2px solid " + (a.color || "var(--ink-4)") }} />}
      </span>
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: 12, color: "var(--ink)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{r.msg}</div>
        <div style={{ display: "flex", alignItems: "center", gap: 7, marginTop: 5, fontSize: 9.5, color: "var(--ink-4)" }} className="tnum">
          <AuthorAvatar id={r.author} size={15} />
          <span style={{ color: "var(--ink-3)" }}>{a.name}</span>
          <Sha sha={r.sha} dim />
          <AddDel add={r.add} del={r.del} />
          <span style={{ marginLeft: "auto" }}>{r.rel}</span>
        </div>
      </div>
    </div>
  );
}

function FileHistoryView({ fh }) {
  const G = GE();
  fh = fh || G.fileHistory;
  // default: base = older (index 1), compare = newer (index 0)
  const [base, setBase] = useStateE(fh.revisions[1].sha);
  const [compare, setCompare] = useStateE(fh.revisions[0].sha);

  // clicking a revision: assign to whichever end is "further" — simple compare picker
  const pick = (sha) => {
    if (sha === base || sha === compare) return;
    // replace the end that keeps base older than compare in list order
    const idx = (s) => fh.revisions.findIndex((r) => r.sha === s);
    if (idx(sha) > idx(compare)) setBase(sha); else setCompare(sha);
  };
  const swap = () => { const b = base; setBase(compare); setCompare(b); };

  const isDefault = base === fh.revisions[1].sha && compare === fh.revisions[0].sha;
  const baseRev = fh.revisions.find((r) => r.sha === base);
  const compRev = fh.revisions.find((r) => r.sha === compare);
  const diff = isDefault ? fh.diff : synthDiff({ path: fh.path, state: "M", add: compRev.add, del: baseRev.del });

  return (
    <div style={{ flex: 1, display: "grid", gridTemplateColumns: "346px 1fr", minHeight: 0, background: "var(--panel-2)" }}>
      {/* revision timeline */}
      <div style={{ display: "flex", flexDirection: "column", minHeight: 0, borderRight: "1px solid var(--hair)", background: "var(--panel)" }}>
        <div style={{ padding: "10px 14px", borderBottom: "1px solid var(--hair)", flex: "none" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 7 }}>
            <Icon name="clock" size="sm" style={{ color: "var(--accent-2)" }} />
            <span style={{ color: "var(--ink-4)", fontSize: 11 }}>{fileDir(fh.path)}</span><span style={{ fontSize: 11.5, color: "var(--ink)", marginLeft: -4 }}>{fileName(fh.path)}</span>
          </div>
          <div style={{ fontSize: 10, color: "var(--ink-3)", marginTop: 7 }}>{fh.revisions.length} revisions · click two to compare</div>
        </div>
        <div className="scroll-y" style={{ flex: 1, position: "relative", padding: "6px 0" }}>
          <div style={{ position: "absolute", left: 23, top: 12, bottom: 12, width: 1, background: "var(--hair)" }} />
          {fh.revisions.map((r) => (
            <RevRow key={r.sha} r={r} role={r.sha === base ? "base" : r.sha === compare ? "compare" : null} onClick={() => pick(r.sha)} />
          ))}
        </div>
      </div>

      {/* diff across revisions */}
      <div style={{ display: "flex", flexDirection: "column", minHeight: 0, background: "var(--bg)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 14px", background: "var(--panel)", borderBottom: "1px solid var(--hair)", flex: "none" }}>
          <Icon name="diff" size="sm" style={{ color: "var(--accent)" }} />
          <span style={{ fontSize: 11, color: "var(--ink-2)" }}>{fileName(fh.path)} across revisions</span>
          <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 7 }} className="tnum">
            <span className="chip" style={{ fontSize: 9.5, padding: "1px 7px", color: "var(--accent)", borderColor: "color-mix(in oklch, var(--accent), transparent 55%)" }}>A {base}</span>
            <button className="pane-btn" onClick={swap} title="Swap"><Icon name="swap" size="sm" style={{ width: 14, height: 14 }} /></button>
            <span className="chip" style={{ fontSize: 9.5, padding: "1px 7px", color: "var(--accent-2)", borderColor: "color-mix(in oklch, var(--accent-2), transparent 55%)" }}>B {compare}</span>
          </div>
        </div>
        <div className="scroll-y" style={{ flex: 1, minHeight: 0 }}>
          <div style={{ padding: "7px 14px", fontSize: 10, color: "var(--ink-4)", borderBottom: "1px solid var(--hair)", display: "flex", gap: 10, flexWrap: "wrap" }} className="tnum">
            <span><b style={{ color: "var(--accent)" }}>A</b> {baseRev.rel} · {baseRev.msg}</span>
            <span><b style={{ color: "var(--accent-2)" }}>B</b> {compRev.rel} · {compRev.msg}</span>
          </div>
          <HunkRows hunks={diff.hunks} lang={diff.lang} />
        </div>
      </div>
    </div>
  );
}

// =========================================================================
// Feature 6 — partial commit: hunk- and line-level selection for partial
// commit / partial discard, with a running "N hunks selected" state.
// =========================================================================
const lineKey = (fp, hid, li) => fp + ":" + hid + ":" + li;

function PartialCommitView({ onFlash }) {
  const G = GE();
  const tree = G.workingTree;
  const [activePath, setActivePath] = useStateE(tree[0].path);
  // selected changed lines (default: all selected)
  const [sel, setSel] = useStateE(() => {
    const s = {};
    tree.forEach((f) => f.hunks.forEach((h) => h.lines.forEach((ln, li) => { if (ln.k !== " ") s[lineKey(f.path, h.id, li)] = true; })));
    return s;
  });

  const file = tree.find((f) => f.path === activePath);
  const changedOf = (h) => h.lines.map((ln, li) => ({ ln, li })).filter((x) => x.ln.k !== " ");
  const hunkSel = (fp, h) => {
    const ch = changedOf(h);
    const on = ch.filter((x) => sel[lineKey(fp, h.id, x.li)]).length;
    return on === 0 ? "none" : on === ch.length ? "all" : "some";
  };

  const toggleLine = (fp, hid, li) => setSel((s) => ({ ...s, [lineKey(fp, hid, li)]: !s[lineKey(fp, hid, li)] }));
  const toggleHunk = (fp, h) => {
    const want = hunkSel(fp, h) !== "all";
    setSel((s) => { const n = { ...s }; changedOf(h).forEach((x) => { n[lineKey(fp, h.id, x.li)] = want; }); return n; });
  };
  const setAll = (want) => setSel(() => {
    const s = {};
    tree.forEach((f) => f.hunks.forEach((h) => h.lines.forEach((ln, li) => { if (ln.k !== " ") s[lineKey(f.path, h.id, li)] = want; })));
    return s;
  });

  // running counters
  const stats = useMemoE(() => {
    let lines = 0, hunks = 0, filesTouched = new Set();
    tree.forEach((f) => f.hunks.forEach((h) => {
      const ch = changedOf(h);
      const on = ch.filter((x) => sel[lineKey(f.path, h.id, x.li)]).length;
      if (on > 0) { hunks++; lines += on; filesTouched.add(f.path); }
    }));
    return { lines, hunks, files: filesTouched.size };
  }, [sel]);

  // file-level selection summary for the list
  const fileSummary = (f) => {
    let on = 0, total = 0;
    f.hunks.forEach((h) => changedOf(h).forEach((x) => { total++; if (sel[lineKey(f.path, h.id, x.li)]) on++; }));
    return { on, total };
  };

  // gutter checkbox render-props for HunkRows
  const lead = {
    hunkHead: (h) => {
      const st = hunkSel(activePath, h);
      return (
        <button onClick={(e) => { e.stopPropagation(); toggleHunk(activePath, h); }} title="Select hunk"
          style={{ flex: "none", width: 15, height: 15, marginRight: 9, borderRadius: 4, display: "grid", placeItems: "center", cursor: "pointer",
            border: "1px solid " + (st === "none" ? "var(--hair-2)" : "var(--accent)"), background: st === "all" ? "var(--accent)" : st === "some" ? "color-mix(in oklch, var(--accent), transparent 55%)" : "transparent" }}>
          {st === "all" && <Icon name="check" size="sm" style={{ width: 10, height: 10, color: "#06070b" }} />}
          {st === "some" && <span style={{ width: 7, height: 2, borderRadius: 2, background: "#06070b" }} />}
        </button>
      );
    },
    lineHead: (h, ln, li) => {
      if (ln.k === " ") return <span style={{ width: 24, flex: "none" }} />;
      const on = sel[lineKey(activePath, h.id, li)];
      return (
        <button onClick={(e) => { e.stopPropagation(); toggleLine(activePath, h.id, li); }}
          style={{ flex: "none", width: 14, height: 14, margin: "3px 10px 0 8px", borderRadius: 3, display: "grid", placeItems: "center", cursor: "pointer",
            border: "1px solid " + (on ? "var(--accent)" : "var(--hair-2)"), background: on ? "var(--accent)" : "transparent" }}>
          {on && <Icon name="check" size="sm" style={{ width: 9, height: 9, color: "#06070b" }} />}
        </button>
      );
    },
  };
  // dim unselected changed lines
  const lineStyle = (h, ln, li) => {
    if (ln.k === " ") return "transparent";
    const on = sel[lineKey(activePath, h.id, li)];
    if (!on) return "transparent";
    return ln.k === "+" ? "var(--code-add-bg)" : "var(--code-del-bg)";
  };

  return (
    <div style={{ flex: 1, display: "grid", gridTemplateColumns: "300px 1fr", minHeight: 0, background: "var(--panel-2)" }}>
      {/* working-tree file list */}
      <div style={{ display: "flex", flexDirection: "column", minHeight: 0, borderRight: "1px solid var(--hair)", background: "var(--panel)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 12px 8px 14px", borderBottom: "1px solid var(--hair)", flex: "none" }}>
          <Icon name="diff" size="sm" style={{ color: "var(--accent-2)" }} />
          <span className="up" style={{ fontSize: 9, color: "var(--ink-3)" }}>Working tree · {tree.length}</span>
          <div style={{ marginLeft: "auto", display: "flex", gap: 4 }}>
            <button className="btn ghost-hair" onClick={() => setAll(true)} style={{ padding: "2px 7px", fontSize: 9.5 }}>All</button>
            <button className="btn ghost-hair" onClick={() => setAll(false)} style={{ padding: "2px 7px", fontSize: 9.5 }}>None</button>
          </div>
        </div>
        <div className="scroll-y" style={{ flex: 1, padding: "6px 0" }}>
          {tree.map((f) => {
            const { on, total } = fileSummary(f), active = f.path === activePath;
            const st = on === 0 ? "none" : on === total ? "all" : "some";
            return (
              <div key={f.path} onClick={() => setActivePath(f.path)}
                style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 12px", cursor: "pointer", margin: "1px 6px", borderRadius: "var(--r-sm)", background: active ? "var(--panel-3)" : "transparent" }}
                onMouseEnter={(e) => { if (!active) e.currentTarget.style.background = "var(--panel-2)"; }}
                onMouseLeave={(e) => { if (!active) e.currentTarget.style.background = "transparent"; }}>
                <span style={{ flex: "none", width: 15, height: 15, borderRadius: 4, display: "grid", placeItems: "center",
                  border: "1px solid " + (st === "none" ? "var(--hair-2)" : "var(--accent)"), background: st === "all" ? "var(--accent)" : st === "some" ? "color-mix(in oklch, var(--accent), transparent 55%)" : "transparent" }}>
                  {st === "all" && <Icon name="check" size="sm" style={{ width: 10, height: 10, color: "#06070b" }} />}
                  {st === "some" && <span style={{ width: 7, height: 2, borderRadius: 2, background: "#06070b" }} />}
                </span>
                <RStateBadge state={f.state} />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: 11.5, color: "var(--ink)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{fileName(f.path)}</div>
                  <div style={{ fontSize: 9.5, color: "var(--ink-4)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{fileDir(f.path)}</div>
                </div>
                <AddDel add={f.add} del={f.del} />
              </div>
            );
          })}
        </div>
      </div>

      {/* hunks with hunk/line selection */}
      <div style={{ display: "flex", flexDirection: "column", minHeight: 0, background: "var(--bg)" }}>
        <DiffFileHeader path={activePath} lang={file.lang}
          left={<span className="chip" style={{ fontSize: 9, padding: "1px 6px", color: "var(--accent-2)" }}>modified</span>} />
        <div className="scroll-y" style={{ flex: 1, minHeight: 0 }}>
          <HunkRows hunks={file.hunks} lang={file.lang} lead={lead} lineStyle={lineStyle} onLineClick={(h, ln, li) => ln.k !== " " && toggleLine(activePath, h.id, li)} />
        </div>

        {/* running selection state + actions */}
        <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "10px 14px", borderTop: "1px solid var(--hair)", background: "var(--panel)", flex: "none" }}>
          <span style={{ fontSize: 11, color: "var(--ink)" }}>
            <b className="tnum" style={{ color: "var(--accent)" }}>{stats.hunks}</b> {stats.hunks === 1 ? "hunk" : "hunks"} selected
          </span>
          <span className="tnum" style={{ fontSize: 10, color: "var(--ink-4)" }}>{stats.lines} lines · {stats.files} files</span>
          <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
            <button className="btn ghost-hair" disabled={!stats.hunks} onClick={() => onFlash && onFlash("discarded " + stats.hunks + " hunks")} style={{ color: "var(--st-blocked)" }}>
              <Icon name="discard" size="sm" />Discard selected
            </button>
            <button className="btn ghost-hair" disabled={!stats.hunks} onClick={() => onFlash && onFlash("staged " + stats.hunks + " hunks")}>
              <Icon name="stage" size="sm" />Stage
            </button>
            <button className="btn primary" disabled={!stats.hunks} onClick={() => onFlash && onFlash("committed " + stats.hunks + " hunks · " + stats.lines + " lines")}>
              <Icon name="commit" size="sm" />Commit {stats.hunks} {stats.hunks === 1 ? "hunk" : "hunks"}
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

Object.assign(window, { FileHistoryView, PartialCommitView });
