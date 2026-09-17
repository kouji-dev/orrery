/* global React, Icon, fileName, fileDir, highlight, RStateBadge */
// Orrery — Repository ▸ Conflicts  (Feature 4)
// 3-way merge/rebase session: conflicted-file list + per-file base/ours/theirs
// with per-conflict accept ours/theirs/both, conflict-to-conflict nav, a
// resolved/remaining counter, and Commit-merge / Continue / Abort actions.

const { useState: useStateC, useRef: useRefC, useMemo: useMemoC } = React;
const GC = () => window.GIT;

function resultLines(seg, r) {
  if (r === "ours") return seg.ours;
  if (r === "theirs") return seg.theirs;
  if (r === "both") return [...seg.ours, ...seg.theirs];
  return null;
}

// one side column (ours / theirs)
function SideCol({ label, sub, color, lines, chosen, onPick, lang }) {
  return (
    <div style={{ flex: 1, minWidth: 0, border: "1px solid " + (chosen ? color : "var(--hair)"), borderRadius: "var(--r-sm)", overflow: "hidden",
      background: chosen ? "color-mix(in oklch, " + color + ", transparent 92%)" : "var(--panel-2)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 7, padding: "5px 9px", borderBottom: "1px solid var(--hair)", background: "color-mix(in oklch, " + color + ", transparent 88%)" }}>
        <span style={{ width: 7, height: 7, borderRadius: 2, background: color, flex: "none" }} />
        <span style={{ fontSize: 10.5, fontWeight: 600, color: "var(--ink)" }}>{label}</span>
        <span className="tnum" style={{ fontSize: 9, color: "var(--ink-4)" }}>{sub}</span>
        <button className="btn" onClick={onPick} style={{ marginLeft: "auto", padding: "2px 8px", fontSize: 10,
          color: chosen ? color : "var(--ink-3)", border: "1px solid " + (chosen ? color : "var(--hair)"),
          background: chosen ? "color-mix(in oklch, " + color + ", transparent 86%)" : "transparent" }}>
          {chosen ? <><Icon name="check" size="sm" style={{ width: 11, height: 11 }} />accepted</> : "accept"}
        </button>
      </div>
      <pre style={{ margin: 0, padding: "5px 0", fontFamily: "var(--font-mono)", fontSize: 11.5, lineHeight: 1.65 }}>
        {lines.map((s, i) => (
          <div key={i} style={{ display: "flex" }}>
            <span style={{ width: 14, flex: "none", textAlign: "center", color: color, userSelect: "none", opacity: 0.7 }}>{chosen ? "✓" : "·"}</span>
            <code style={{ flex: 1, whiteSpace: "pre-wrap", wordBreak: "break-word", paddingRight: 10, color: "var(--ink-2)" }}>{highlight(s, lang) }</code>
          </div>
        ))}
      </pre>
    </div>
  );
}

function ConflictBlock({ seg, idx, num, res, lang, ours, theirs, onResolve, onEdit, blockRef }) {
  const [showBase, setShowBase] = useStateC(false);
  const [editing, setEditing] = useStateC(false);
  const resolved = res && res.res;
  const rLines = res && res.custom != null ? res.custom.split("\n") : resultLines(seg, resolved);

  return (
    <div ref={blockRef} style={{ margin: "10px 14px", border: "1px solid " + (resolved ? "color-mix(in oklch, var(--st-done), transparent 60%)" : "color-mix(in oklch, var(--st-blocked), transparent 55%)"),
      borderRadius: "var(--r-md)", overflow: "hidden", background: "var(--panel)" }}>
      {/* conflict header */}
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "7px 11px",
        background: resolved ? "color-mix(in oklch, var(--st-done), transparent 90%)" : "color-mix(in oklch, var(--st-blocked), transparent 91%)" }}>
        <span style={{ width: 8, height: 8, borderRadius: "50%", flex: "none", background: resolved ? "var(--st-done)" : "var(--st-blocked)" }} />
        <span style={{ fontSize: 11, fontWeight: 600, color: "var(--ink)" }}>Conflict {num}</span>
        <span style={{ fontSize: 10, color: resolved ? "var(--st-done)" : "var(--st-blocked)" }}>{resolved ? "resolved · " + ({ ours: "ours", theirs: "theirs", both: "both sides" }[resolved] || "edited") : "unresolved"}</span>
        <div style={{ marginLeft: "auto", display: "flex", gap: 6 }}>
          <button className="btn ghost-hair" onClick={() => onResolve(idx, "both")} style={{ padding: "3px 8px", fontSize: 10 }}>Both</button>
          <button className="btn ghost-hair" onClick={() => setShowBase((v) => !v)} style={{ padding: "3px 8px", fontSize: 10, color: showBase ? "var(--ink)" : "var(--ink-3)" }}>
            <Icon name={showBase ? "chevronD" : "chevron"} size="sm" style={{ width: 11, height: 11 }} />Base
          </button>
        </div>
      </div>

      {/* base (common ancestor) */}
      {showBase && (
        <div style={{ padding: "6px 0", borderBottom: "1px solid var(--hair)", background: "var(--bg)" }}>
          <div className="up" style={{ fontSize: 8.5, color: "var(--ink-4)", padding: "0 14px 4px" }}>base · common ancestor</div>
          <pre style={{ margin: 0, fontFamily: "var(--font-mono)", fontSize: 11.5, lineHeight: 1.6, color: "var(--ink-3)", padding: "0 14px" }}>{seg.base.join("\n")}</pre>
        </div>
      )}

      {/* ours | theirs */}
      <div style={{ display: "flex", gap: 9, padding: 11 }}>
        <SideCol label="Ours" sub={ours} color="var(--accent)" lines={seg.ours} lang={lang}
          chosen={resolved === "ours"} onPick={() => onResolve(idx, "ours")} />
        <SideCol label="Theirs" sub={theirs} color="var(--accent-2)" lines={seg.theirs} lang={lang}
          chosen={resolved === "theirs"} onPick={() => onResolve(idx, "theirs")} />
      </div>

      {/* result (editable) */}
      <div style={{ borderTop: "1px solid var(--hair)", background: "var(--bg)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 7, padding: "5px 12px" }}>
          <Icon name="enter" size="sm" style={{ width: 12, height: 12, color: resolved ? "var(--st-done)" : "var(--ink-4)" }} />
          <span className="up" style={{ fontSize: 8.5, color: "var(--ink-4)" }}>Result</span>
          {resolved && <button className="btn" onClick={() => setEditing((v) => !v)} style={{ marginLeft: "auto", padding: "2px 7px", fontSize: 9.5, color: editing ? "var(--accent)" : "var(--ink-3)" }}>
            <Icon name="rename" size="sm" style={{ width: 11, height: 11 }} />{editing ? "done" : "edit"}
          </button>}
        </div>
        {resolved ? (
          editing ? (
            <textarea autoFocus defaultValue={rLines.join("\n")} onBlur={(e) => { onEdit(idx, e.target.value); setEditing(false); }}
              spellCheck={false}
              style={{ width: "100%", minHeight: 70, resize: "vertical", background: "var(--panel-2)", border: "1px solid var(--accent)", outline: "none",
                color: "var(--code-add-ink)", fontFamily: "var(--font-mono)", fontSize: 11.5, lineHeight: 1.65, padding: "6px 12px 6px 14px" }} />
          ) : (
            <pre style={{ margin: 0, padding: "2px 12px 8px 14px", fontFamily: "var(--font-mono)", fontSize: 11.5, lineHeight: 1.65, background: "var(--code-add-bg)" }}>
              {rLines.map((s, i) => (
                <div key={i} style={{ display: "flex" }}>
                  <span style={{ width: 14, flex: "none", textAlign: "center", color: "var(--code-add-ink)", userSelect: "none" }}>+</span>
                  <code style={{ flex: 1, whiteSpace: "pre-wrap", wordBreak: "break-word", color: "var(--code-add-ink)" }}>{highlight(s, lang)}</code>
                </div>
              ))}
            </pre>
          )
        ) : (
          <div style={{ padding: "10px 14px", fontSize: 10.5, color: "var(--ink-4)", borderTop: "1px dashed var(--hair-2)", display: "flex", alignItems: "center", gap: 6 }}>
            <Icon name="flag" size="sm" style={{ width: 12, height: 12, color: "var(--st-blocked)" }} />pick a side above, or accept both, to resolve
          </div>
        )}
      </div>
    </div>
  );
}

function ContextLines({ lines, lang }) {
  return (
    <pre style={{ margin: 0, padding: "2px 0", fontFamily: "var(--font-mono)", fontSize: 11.5, lineHeight: 1.65 }}>
      {lines.map((s, i) => (
        <div key={i} style={{ display: "flex" }}>
          <span style={{ width: 14, flex: "none" }} />
          <code style={{ flex: 1, whiteSpace: "pre-wrap", wordBreak: "break-word", paddingRight: 10, color: "var(--ink-3)", paddingLeft: 14 }}>{highlight(s, lang)}</code>
        </div>
      ))}
    </pre>
  );
}

function ConflictView({ session, onFlash }) {
  const G = GC();
  session = session || G.conflict;
  const [activePath, setActivePath] = useStateC(session.files[0].path);
  // resolutions: { path: { confIdx: {res, custom} } }
  const [resMap, setResMap] = useStateC({});
  const [cursor, setCursor] = useStateC(0); // index within current file's conflicts
  const scrollRef = useRefC(null);
  const blockRefs = useRefC({});

  const file = session.files.find((f) => f.path === activePath);
  const confIdxs = useMemoC(() => file.segments.map((s, i) => s.type === "conflict" ? i : -1).filter((i) => i >= 0), [activePath]);

  const fileRes = resMap[activePath] || {};
  const resolvedCount = (f) => {
    const rm = resMap[f.path] || {};
    return f.segments.filter((s, i) => s.type === "conflict" && rm[i] && rm[i].res).length;
  };
  const totalConf = session.files.reduce((s, f) => s + f.conflicts, 0);
  const totalResolved = session.files.reduce((s, f) => s + resolvedCount(f), 0);
  const allDone = totalResolved === totalConf;

  const resolve = (confIdx, res) => {
    setResMap((m) => ({ ...m, [activePath]: { ...(m[activePath] || {}), [confIdx]: { res, custom: null } } }));
  };
  const editResult = (confIdx, custom) => {
    setResMap((m) => ({ ...m, [activePath]: { ...(m[activePath] || {}), [confIdx]: { ...(m[activePath] || {})[confIdx], custom } } }));
  };

  const gotoConf = (dir) => {
    const next = Math.max(0, Math.min(confIdxs.length - 1, cursor + dir));
    setCursor(next);
    const el = blockRefs.current[confIdxs[next]];
    if (el && scrollRef.current) scrollRef.current.scrollTop = el.offsetTop - 70;
  };

  const acceptAll = (side) => {
    setResMap((m) => {
      const fm = { ...(m[activePath] || {}) };
      confIdxs.forEach((ci) => { fm[ci] = { res: side, custom: null }; });
      return { ...m, [activePath]: fm };
    });
    onFlash && onFlash("accepted " + side + " for all conflicts in " + fileName(activePath));
  };

  return (
    <div style={{ flex: 1, display: "grid", gridTemplateColumns: "292px 1fr", minHeight: 0, background: "var(--panel-2)" }}>
      {/* conflicted file list + session actions */}
      <div style={{ display: "flex", flexDirection: "column", minHeight: 0, borderRight: "1px solid var(--hair)", background: "var(--panel)" }}>
        <div style={{ padding: "10px 14px", borderBottom: "1px solid var(--hair)", flex: "none" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 7 }}>
            <Icon name="merge" size="sm" style={{ color: "var(--st-blocked)" }} />
            <span style={{ fontSize: 12, fontWeight: 600, color: "var(--ink)" }}>{session.title}</span>
          </div>
          <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 8, fontSize: 10, color: "var(--ink-4)" }} className="tnum">
            <span style={{ display: "flex", alignItems: "center", gap: 4 }}><span style={{ width: 7, height: 7, borderRadius: 2, background: "var(--accent)" }} />ours · {session.ours}</span>
            <span style={{ display: "flex", alignItems: "center", gap: 4 }}><span style={{ width: 7, height: 7, borderRadius: 2, background: "var(--accent-2)" }} />theirs · {session.theirs}</span>
          </div>
          {/* progress */}
          <div style={{ marginTop: 11 }}>
            <div style={{ display: "flex", justifyContent: "space-between", fontSize: 9.5, color: "var(--ink-3)", marginBottom: 5 }} className="tnum">
              <span>{totalResolved} resolved</span><span>{totalConf - totalResolved} remaining</span>
            </div>
            <div className="meter"><i style={{ width: (totalResolved / totalConf * 100) + "%", background: allDone ? "var(--st-done)" : "linear-gradient(90deg, var(--accent), var(--accent-2))" }} /></div>
          </div>
        </div>

        <div className="scroll-y" style={{ flex: 1, padding: "6px 0" }}>
          {session.files.map((f) => {
            const rc = resolvedCount(f), done = rc === f.conflicts, active = f.path === activePath;
            return (
              <div key={f.path} onClick={() => { setActivePath(f.path); setCursor(0); }}
                style={{ display: "flex", alignItems: "center", gap: 8, padding: "7px 12px", cursor: "pointer", margin: "1px 6px", borderRadius: "var(--r-sm)", background: active ? "var(--panel-3)" : "transparent" }}
                onMouseEnter={(e) => { if (!active) e.currentTarget.style.background = "var(--panel-2)"; }}
                onMouseLeave={(e) => { if (!active) e.currentTarget.style.background = "transparent"; }}>
                <span style={{ flex: "none", width: 15, height: 15, borderRadius: "50%", display: "grid", placeItems: "center",
                  background: done ? "var(--st-done)" : "color-mix(in oklch, var(--st-blocked), transparent 80%)", border: done ? "none" : "1px solid color-mix(in oklch, var(--st-blocked), transparent 50%)" }}>
                  {done ? <Icon name="check" size="sm" style={{ width: 10, height: 10, color: "#06070b" }} /> : <span style={{ width: 5, height: 5, borderRadius: "50%", background: "var(--st-blocked)" }} />}
                </span>
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ fontSize: 11.5, color: "var(--ink)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{fileName(f.path)}</div>
                  <div style={{ fontSize: 9.5, color: "var(--ink-4)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{fileDir(f.path)}</div>
                </div>
                <span className="tnum" style={{ fontSize: 9.5, color: done ? "var(--st-done)" : "var(--st-blocked)", flex: "none" }}>{rc}/{f.conflicts}</span>
              </div>
            );
          })}
        </div>

        {/* session actions */}
        <div style={{ padding: 12, borderTop: "1px solid var(--hair)", display: "grid", gap: 7, flex: "none" }}>
          <button className={"btn " + (allDone ? "primary" : "ghost-hair")} disabled={!allDone}
            onClick={() => onFlash && onFlash(allDone ? "committed merge — " + session.theirs + " → " + session.ours : "")}
            style={{ justifyContent: "center" }}>
            <Icon name="commit" size="sm" />Commit merge
          </button>
          <div style={{ display: "flex", gap: 7 }}>
            <button className="btn ghost-hair" onClick={() => onFlash && onFlash("continued — staged resolved files")} style={{ flex: 1, justifyContent: "center" }}>
              <Icon name="merge" size="sm" />Continue
            </button>
            <button className="btn ghost-hair" onClick={() => onFlash && onFlash("aborted merge — working tree restored")} style={{ flex: 1, justifyContent: "center", color: "var(--st-blocked)" }}>
              <Icon name="discard" size="sm" />Abort
            </button>
          </div>
        </div>
      </div>

      {/* per-file 3-way merge surface */}
      <div style={{ display: "flex", flexDirection: "column", minHeight: 0, background: "var(--bg)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 14px", background: "var(--panel)", borderBottom: "1px solid var(--hair)", fontSize: 11.5, flex: "none" }}>
          <Icon name="file" size="sm" style={{ color: "var(--ink-3)" }} />
          <span style={{ color: "var(--ink-4)" }}>{fileDir(activePath)}</span><span style={{ marginLeft: -6 }}>{fileName(activePath)}</span>
          {/* conflict navigation */}
          <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 6 }}>
            <button className="btn ghost-hair" onClick={() => acceptAll("ours")} style={{ padding: "3px 8px", fontSize: 10 }}>Accept all ours</button>
            <button className="btn ghost-hair" onClick={() => acceptAll("theirs")} style={{ padding: "3px 8px", fontSize: 10 }}>theirs</button>
            <div style={{ display: "flex", alignItems: "center", gap: 2, padding: 2, background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-sm)" }}>
              <button className="pane-btn" onClick={() => gotoConf(-1)} title="Previous conflict"><Icon name="chevron" size="sm" style={{ transform: "rotate(180deg)", width: 13, height: 13 }} /></button>
              <span className="tnum" style={{ fontSize: 10, color: "var(--ink-3)", padding: "0 4px" }}>{cursor + 1}/{confIdxs.length}</span>
              <button className="pane-btn" onClick={() => gotoConf(1)} title="Next conflict"><Icon name="chevron" size="sm" style={{ width: 13, height: 13 }} /></button>
            </div>
          </div>
        </div>

        <div ref={scrollRef} className="scroll-y" style={{ flex: 1, padding: "4px 0 16px", minHeight: 0 }}>
          {file.segments.map((seg, i) => {
            if (seg.type === "ctx") return <ContextLines key={i} lines={seg.lines} lang={file.lang} />;
            const num = confIdxs.indexOf(i) + 1;
            return <ConflictBlock key={i} seg={seg} idx={i} num={num} res={fileRes[i]} lang={file.lang}
              ours={session.ours} theirs={session.theirs} onResolve={resolve} onEdit={editResult}
              blockRef={(el) => { blockRefs.current[i] = el; }} />;
          })}
        </div>
      </div>
    </div>
  );
}

// per-file 3-way resolution surface (right side), resolution state lifted to caller.
// used both by the standalone ConflictView and folded into the agent Diff pane.
function ConflictFilePane({ file, fileRes, ours, theirs, onResolve, onEdit, onAcceptAll }) {
  const [cursor, setCursor] = useStateC(0);
  const scrollRef = useRefC(null);
  const blockRefs = useRefC({});
  const confIdxs = useMemoC(() => file.segments.map((s, i) => s.type === "conflict" ? i : -1).filter((i) => i >= 0), [file.path]);
  React.useEffect(() => { setCursor(0); }, [file.path]);
  const gotoConf = (dir) => {
    const next = Math.max(0, Math.min(confIdxs.length - 1, cursor + dir));
    setCursor(next);
    const el = blockRefs.current[confIdxs[next]];
    if (el && scrollRef.current) scrollRef.current.scrollTop = el.offsetTop - 60;
  };
  return (
    <div style={{ display: "flex", flexDirection: "column", minHeight: 0, background: "var(--bg)" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 14px", background: "var(--panel)", borderBottom: "1px solid var(--hair)", fontSize: 11.5, flex: "none" }}>
        <Icon name="merge" size="sm" style={{ color: "var(--st-blocked)" }} />
        <span style={{ color: "var(--ink-4)" }}>{fileDir(file.path)}</span><span style={{ marginLeft: -6 }}>{fileName(file.path)}</span>
        <span className="chip" style={{ fontSize: 9, padding: "1px 6px", color: "var(--st-blocked)", borderColor: "color-mix(in oklch, var(--st-blocked), transparent 55%)" }}>3-way merge</span>
        <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 6 }}>
          <button className="btn ghost-hair" onClick={() => onAcceptAll("ours", confIdxs)} style={{ padding: "3px 8px", fontSize: 10 }}>Accept all ours</button>
          <button className="btn ghost-hair" onClick={() => onAcceptAll("theirs", confIdxs)} style={{ padding: "3px 8px", fontSize: 10 }}>theirs</button>
          <div style={{ display: "flex", alignItems: "center", gap: 2, padding: 2, background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-sm)" }}>
            <button className="pane-btn" onClick={() => gotoConf(-1)} title="Previous conflict"><Icon name="chevron" size="sm" style={{ transform: "rotate(180deg)", width: 13, height: 13 }} /></button>
            <span className="tnum" style={{ fontSize: 10, color: "var(--ink-3)", padding: "0 4px" }}>{cursor + 1}/{confIdxs.length}</span>
            <button className="pane-btn" onClick={() => gotoConf(1)} title="Next conflict"><Icon name="chevron" size="sm" style={{ width: 13, height: 13 }} /></button>
          </div>
        </div>
      </div>
      <div ref={scrollRef} className="scroll-y" style={{ flex: 1, padding: "4px 0 16px", minHeight: 0 }}>
        {file.segments.map((seg, i) => {
          if (seg.type === "ctx") return <ContextLines key={i} lines={seg.lines} lang={file.lang} />;
          const num = confIdxs.indexOf(i) + 1;
          return <ConflictBlock key={i} seg={seg} idx={i} num={num} res={fileRes[i]} lang={file.lang}
            ours={ours} theirs={theirs} onResolve={onResolve} onEdit={onEdit}
            blockRef={(el) => { blockRefs.current[i] = el; }} />;
        })}
      </div>
    </div>
  );
}

Object.assign(window, { ConflictView, ConflictFilePane, ConflictBlock, ContextLines });
