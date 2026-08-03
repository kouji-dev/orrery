/* global React, Icon, fileName, fileDir, langOf, getFileLines, highlight, symbolsOf, SYM_ICON, FileBlameGutter */
// Orrery — editor pane: writable CodeMirror-equivalent surface with dirty state,
// ⌘S / autosave (B1.1 · B1.2), gutter change markers + revert hunk (B4.3),
// structure view + breadcrumbs (B2.5), go-to-line (B2.3), image/PDF preview (B1.4).

const { useState: useStateE, useEffect: useEffectE, useRef: useRefE, useMemo: useMemoE } = React;

// one real vector asset so the preview surface is reachable from the tree
window.FILE_CONTENTS = window.FILE_CONTENTS || {};
if (!window.FILE_CONTENTS["public/prism.svg"]) {
  window.FILE_CONTENTS["public/prism.svg"] = `<svg xmlns="http://www.w3.org/2000/svg" viewBox="0 0 80 80" width="80" height="80">
  <defs>
    <linearGradient id="r" x1="0" y1="0" x2="1" y2="1">
      <stop offset="0" stop-color="#ff5d9e"/><stop offset=".5" stop-color="#a855f7"/><stop offset="1" stop-color="#22d3ee"/>
    </linearGradient>
  </defs>
  <path d="M40 8 L68 40 L40 72 L12 40 Z" fill="none" stroke="url(#r)" stroke-width="4"/>
  <circle cx="40" cy="40" r="9" fill="#a855f7" opacity=".85"/>
</svg>`;
}

const isImage = (p) => /\.(svg|png|jpe?g|gif|webp|avif)$/i.test(p);
const isPdf = (p) => /\.pdf$/i.test(p);
const LH = 19.5, FS = 12.2;

// ---------------------------------------------------------------- previews
function AssetPreview({ path, onFlash }) {
  const [zoom, setZoom] = useStateE(1);
  const src = (window.FILE_CONTENTS || {})[path];
  const svg = /\.svg$/i.test(path) && src ? src : null;
  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", minHeight: 0, background: "var(--bg)" }}>
      <div style={{ flex: 1, minHeight: 0, display: "grid", placeItems: "center", overflow: "auto",
        backgroundImage: "linear-gradient(45deg, var(--panel-2) 25%, transparent 25%, transparent 75%, var(--panel-2) 75%), linear-gradient(45deg, var(--panel-2) 25%, transparent 25%, transparent 75%, var(--panel-2) 75%)",
        backgroundSize: "18px 18px", backgroundPosition: "0 0, 9px 9px" }}>
        {svg
          ? <div style={{ transform: "scale(" + zoom * 2.4 + ")", filter: "drop-shadow(0 0 24px rgba(var(--accent-rgb),.35))" }} dangerouslySetInnerHTML={{ __html: svg }} />
          : <div style={{ display: "flex", flexDirection: "column", alignItems: "center", gap: 10, padding: 30 }}>
              <Icon name={isPdf(path) ? "file" : "layers"} size="lg" style={{ color: "var(--ink-4)" }} />
              <span style={{ fontSize: 11.5, color: "var(--ink-3)" }}>{fileName(path)}</span>
              <span style={{ fontSize: 10, color: "var(--ink-4)" }}>{isPdf(path) ? "PDF · rendered by the embedded viewer" : "binary image · decoded by the platform decoder"}</span>
              <button className="btn ghost-hair" onClick={() => onFlash("opened " + fileName(path) + " in the system viewer")}><Icon name="ext" size="sm" />Open externally</button>
            </div>}
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 9, padding: "6px 12px", borderTop: "1px solid var(--hair)", flex: "none", fontSize: 10, color: "var(--ink-4)" }}>
        <span className="tnum">{svg ? "vector · 80×80" : isPdf(path) ? "3 pages" : "raster"}</span>
        <span>·</span><span className="tnum">{src ? (src.length / 1000).toFixed(1) + " KB" : "—"}</span>
        <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 5 }}>
          <button className="pane-btn" onClick={() => setZoom((z) => Math.max(0.25, z - 0.25))}><Icon name="x" size="sm" style={{ width: 11, height: 11 }} /></button>
          <span className="tnum" style={{ width: 34, textAlign: "center" }}>{Math.round(zoom * 100)}%</span>
          <button className="pane-btn" onClick={() => setZoom((z) => Math.min(4, z + 0.25))}><Icon name="plus" size="sm" style={{ width: 11, height: 11 }} /></button>
          <button className="btn" style={{ padding: "2px 7px", fontSize: 10 }} onClick={() => setZoom(1)}>Fit</button>
        </div>
      </div>
    </div>
  );
}

// -------------------------------------------------------------- structure view
function StructureView({ path, cursorLine, onGo }) {
  const syms = useMemoE(() => symbolsOf(path), [path]);
  return (
    <div style={{ display: "flex", flexDirection: "column", minHeight: 0, borderRight: "1px solid var(--hair)", background: "var(--panel)", width: 214, flex: "none" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 7, padding: "7px 11px", borderBottom: "1px solid var(--hair)", flex: "none" }}>
        <Icon name="layers" size="sm" style={{ color: "var(--accent)" }} />
        <span className="up" style={{ fontSize: 8.5, color: "var(--ink-3)" }}>Structure</span>
        <span className="tnum" style={{ marginLeft: "auto", fontSize: 9, color: "var(--ink-4)" }}>{syms.length}</span>
      </div>
      <div className="scroll-y" style={{ flex: 1, padding: "3px 0" }}>
        {!syms.length && <div style={{ padding: "10px 11px", fontSize: 10.5, color: "var(--ink-4)" }}>no symbols in this file</div>}
        {syms.map((s) => {
          const on = cursorLine >= s.line && cursorLine < (syms[syms.indexOf(s) + 1] ? syms[syms.indexOf(s) + 1].line : 1e9);
          return (
            <div key={s.line + s.name} onClick={() => onGo(s.line)}
              style={{ display: "flex", alignItems: "center", gap: 6, padding: "3px 10px", paddingLeft: 10 + Math.min(3, s.indent / 2) * 9, cursor: "pointer",
                background: on ? "var(--panel-3)" : "transparent", borderLeft: "2px solid " + (on ? "var(--accent)" : "transparent") }}>
              <Icon name={SYM_ICON[s.kind] || "spark"} size="sm" style={{ width: 11, height: 11, color: on ? "var(--accent)" : "var(--ink-4)" }} />
              <span style={{ fontSize: 11, color: on ? "var(--ink)" : "var(--ink-2)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{s.name}</span>
              <span className="tnum" style={{ marginLeft: "auto", fontSize: 8.5, color: "var(--ink-4)" }}>{s.line}</span>
            </div>
          );
        })}
      </div>
    </div>
  );
}

// ---- change hunks: current buffer vs the last saved text (+ git-added lines)
function diffMarks(orig, cur, gitAdded) {
  const marks = {};
  const n = Math.max(orig.length, cur.length);
  for (let i = 0; i < n; i++) {
    if (cur[i] == null) continue;
    if (orig[i] == null) marks[i + 1] = "add";
    else if (orig[i] !== cur[i]) marks[i + 1] = "mod";
  }
  if (cur.length < orig.length) marks[Math.max(1, cur.length)] = "del";
  (gitAdded || []).forEach((ln) => { if (!marks[ln]) marks[ln] = "git"; });
  const hunks = [];
  let run = null;
  Object.keys(marks).map(Number).sort((a, b) => a - b).forEach((ln) => {
    if (run && ln === run.to + 1 && marks[ln] === run.kind) { run.to = ln; return; }
    run = { from: ln, to: ln, kind: marks[ln] };
    hunks.push(run);
  });
  return { marks, hunks };
}

function EditorPane({ path, agent, onFlash, structureOpen, autosave, jump, onOpenCommit }) {
  const FC = window.FILE_CONTENTS || {};
  const initial = useMemoE(() => (FC[path] != null ? FC[path] : getFileLines(path, agent).map((l) => l.s).join("\n")), [path]);
  const [text, setText] = useStateE(initial);
  const [saved, setSaved] = useStateE(initial);
  const [cursor, setCursor] = useStateE({ line: 1, col: 1 });
  const [blame, setBlame] = useStateE(false);
  const [hunkOpen, setHunkOpen] = useStateE(null);
  const [flashLine, setFlashLine] = useStateE(null);
  const ta = useRefE(null);
  const scroller = useRefE(null);
  const saveT = useRefE(null);
  useEffectE(() => { setText(initial); setSaved(initial); setCursor({ line: 1, col: 1 }); }, [path, initial]);

  const lines = text.split("\n");
  const savedLines = saved.split("\n");
  const gitAdded = useMemoE(() => {
    const d = (window.DIFFS || {})[agent && agent.id];
    if (!d || d.file !== path) return [];
    return d.hunks.flatMap((h) => h.lines.filter((l) => l.k === "+").map((l) => l.n));
  }, [path, agent && agent.id]);
  const { marks, hunks } = useMemoE(() => diffMarks(savedLines, lines, gitAdded), [text, saved, gitAdded]);
  const dirty = text !== saved;
  const lang = langOf(path);

  const save = () => {
    if (!dirty) return;
    FC[path] = text;
    setSaved(text);
    onFlash("saved " + fileName(path) + " · " + lines.length + " lines written through the worktree");
  };
  // ⌘S + autosave (debounced 2s after typing stops)
  useEffectE(() => {
    const onSave = () => save();
    window.addEventListener("orrery:save", onSave);
    const onBlame = () => setBlame((v) => !v);
    window.addEventListener("orrery:blame", onBlame);
    const onRevert = () => { if (hunks.length) revertHunk(hunks[0]); };
    window.addEventListener("orrery:revert-hunk", onRevert);
    return () => { window.removeEventListener("orrery:save", onSave); window.removeEventListener("orrery:blame", onBlame); window.removeEventListener("orrery:revert-hunk", onRevert); };
  }, [text, saved, hunks]);
  useEffectE(() => {
    if (!autosave || !dirty) return;
    clearTimeout(saveT.current);
    saveT.current = setTimeout(save, 2000);
    return () => clearTimeout(saveT.current);
  }, [text, autosave]);

  // go-to-line / symbol jump
  const goLine = (line, col) => {
    const el = ta.current;
    if (!el) return;
    const pos = lines.slice(0, line - 1).join("\n").length + (line > 1 ? 1 : 0) + Math.max(0, (col || 1) - 1);
    el.focus();
    el.setSelectionRange(pos, pos);
    if (scroller.current) scroller.current.scrollTop = Math.max(0, (line - 6) * LH);
    setCursor({ line, col: col || 1 });
    setFlashLine(line);
    setTimeout(() => setFlashLine(null), 1200);
  };
  useEffectE(() => { if (jump && jump.path === path && jump.line) goLine(jump.line, jump.col); }, [jump && jump.at]);

  const syncCursor = (e) => {
    const upto = e.target.value.slice(0, e.target.selectionStart);
    const ls = upto.split("\n");
    setCursor({ line: ls.length, col: ls[ls.length - 1].length + 1 });
  };
  const revertHunk = (h) => {
    const next = lines.slice();
    for (let ln = h.from; ln <= h.to; ln++) {
      if (savedLines[ln - 1] != null) next[ln - 1] = savedLines[ln - 1];
    }
    setText(next.join("\n"));
    setHunkOpen(null);
    onFlash("reverted hunk · lines " + h.from + (h.to > h.from ? "–" + h.to : ""));
  };

  if (isImage(path) || isPdf(path)) {
    return (
      <div style={{ flex: 1, display: "flex", flexDirection: "column", minHeight: 0 }}>
        <Breadcrumbs path={path} agent={agent} symbol={null} right={<span className="chip" style={{ fontSize: 9 }}>preview</span>} />
        <AssetPreview path={path} onFlash={onFlash} />
      </div>
    );
  }

  const syms = symbolsOf(path);
  const curSym = syms.filter((s) => s.line <= cursor.line).slice(-1)[0];
  const markColor = { add: "var(--code-add-ink)", mod: "var(--accent-2)", del: "var(--code-del-ink)", git: "color-mix(in oklch, var(--code-add-ink), transparent 55%)" };

  return (
    <div style={{ flex: 1, display: "flex", flexDirection: "column", minHeight: 0, background: "var(--bg)" }}>
      <Breadcrumbs path={path} agent={agent} symbol={curSym} right={
        <React.Fragment>
          {dirty && <span className="chip" style={{ fontSize: 9, color: "#f5c451", borderColor: "color-mix(in oklch, #f5c451, transparent 60%)" }}>unsaved</span>}
          <button className="btn ghost-hair" onClick={() => setBlame((v) => !v)} style={{ padding: "3px 8px", fontSize: 10.5, color: blame ? "var(--ink)" : "var(--ink-3)",
            background: blame ? "color-mix(in oklch, var(--accent), transparent 86%)" : "transparent", borderColor: blame ? "color-mix(in oklch, var(--accent), transparent 60%)" : "var(--hair)" }}>
            <Icon name="git" size="sm" style={{ color: blame ? "var(--accent)" : "inherit" }} />Annotate
          </button>
          <button className={"btn " + (dirty ? "primary" : "ghost-hair")} disabled={!dirty} onClick={save} style={{ padding: "3px 9px", fontSize: 10.5 }}>
            <Icon name="check" size="sm" />Save
          </button>
        </React.Fragment>
      } />
      {blame
        ? <FileBlameGutter agent={agent} path={path} onOpenCommit={onOpenCommit} />
        : (
          <div style={{ flex: 1, display: "flex", minHeight: 0 }}>
            {structureOpen && <StructureView path={path} cursorLine={cursor.line} onGo={(l) => goLine(l, 1)} />}
            <div ref={scroller} className="scroll-y" style={{ flex: 1, minWidth: 0, position: "relative" }}>
              <div style={{ display: "flex", minHeight: "100%" }}>
                {/* gutter: line numbers + change markers */}
                <div style={{ flex: "none", width: 62, background: "var(--panel)", borderRight: "1px solid var(--hair)", position: "relative", userSelect: "none" }}>
                  {lines.map((_, i) => {
                    const ln = i + 1, mk = marks[ln];
                    const h = hunks.find((x) => ln >= x.from && ln <= x.to);
                    return (
                      <div key={ln} style={{ height: LH, display: "flex", alignItems: "center", gap: 4, justifyContent: "flex-end", paddingRight: 6, fontSize: 10.5,
                        color: cursor.line === ln ? "var(--ink-2)" : "var(--ink-4)", background: cursor.line === ln ? "var(--panel-2)" : "transparent" }} className="tnum">
                        {ln}
                        {mk
                          ? <button title={mk === "git" ? "changed by the agent" : "local change · click to revert this hunk"}
                              onClick={() => h && mk !== "git" && setHunkOpen(hunkOpen && hunkOpen.from === h.from ? null : h)}
                              style={{ width: 4, height: LH - 3, marginLeft: 2, border: "none", padding: 0, borderRadius: 2, cursor: mk === "git" ? "default" : "pointer", background: markColor[mk] }} />
                          : <span style={{ width: 4, marginLeft: 2 }} />}
                      </div>
                    );
                  })}
                </div>
                {/* code: highlighted layer under a transparent textarea */}
                <div style={{ flex: 1, minWidth: 0, position: "relative" }}>
                  <pre aria-hidden="true" style={{ margin: 0, padding: "0 14px", fontFamily: "var(--font-mono)", fontSize: FS, lineHeight: LH + "px", pointerEvents: "none", whiteSpace: "pre" }}>
                    {lines.map((s, i) => (
                      <div key={i} style={{ height: LH, background: flashLine === i + 1 ? "color-mix(in oklch, var(--accent), transparent 80%)" : marks[i + 1] === "git" ? "var(--code-add-bg)" : "transparent" }}>
                        <code>{highlight(s, lang)}</code>
                      </div>
                    ))}
                  </pre>
                  <textarea ref={ta} value={text} spellCheck={false} wrap="off"
                    onChange={(e) => { setText(e.target.value); syncCursor(e); }}
                    onKeyUp={syncCursor} onClick={syncCursor}
                    onKeyDown={(e) => { if ((e.metaKey || e.ctrlKey) && e.key.toLowerCase() === "s") { e.preventDefault(); save(); } }}
                    style={{ position: "absolute", inset: 0, width: "100%", height: "100%", resize: "none", border: "none", outline: "none",
                      background: "transparent", color: "transparent", caretColor: "var(--accent)", padding: "0 14px",
                      fontFamily: "var(--font-mono)", fontSize: FS, lineHeight: LH + "px", whiteSpace: "pre", overflow: "hidden" }} />
                  {hunkOpen && (
                    <div className="surface rise" style={{ position: "absolute", left: 8, top: (hunkOpen.from) * LH + 4, zIndex: 6, padding: 8, minWidth: 260, boxShadow: "var(--shadow)", background: "var(--elev)" }}>
                      <div style={{ display: "flex", alignItems: "center", gap: 7, marginBottom: 6 }}>
                        <Icon name="diff" size="sm" style={{ color: "var(--accent-2)" }} />
                        <span style={{ fontSize: 10.5, color: "var(--ink)" }}>lines {hunkOpen.from}{hunkOpen.to > hunkOpen.from ? "–" + hunkOpen.to : ""}</span>
                        <button className="pane-btn" style={{ marginLeft: "auto" }} onClick={() => setHunkOpen(null)}><Icon name="x" size="sm" /></button>
                      </div>
                      <pre style={{ margin: "0 0 7px", fontSize: 10.5, lineHeight: 1.55, maxHeight: 90, overflow: "auto" }}>
                        {Array.from({ length: hunkOpen.to - hunkOpen.from + 1 }, (_, k) => {
                          const ln = hunkOpen.from + k;
                          return (
                            <React.Fragment key={ln}>
                              {savedLines[ln - 1] != null && <div style={{ color: "var(--code-del-ink)", background: "var(--code-del-bg)" }}>− {savedLines[ln - 1]}</div>}
                              <div style={{ color: "var(--code-add-ink)", background: "var(--code-add-bg)" }}>+ {lines[ln - 1]}</div>
                            </React.Fragment>
                          );
                        })}
                      </pre>
                      <button className="btn ghost-hair" style={{ width: "100%", justifyContent: "center", fontSize: 10.5, color: "var(--st-blocked)" }} onClick={() => revertHunk(hunkOpen)}>
                        <Icon name="discard" size="sm" />Revert hunk
                      </button>
                    </div>
                  )}
                </div>
              </div>
            </div>
          </div>
        )}
      {/* editor status line */}
      <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "4px 12px", borderTop: "1px solid var(--hair)", background: "var(--panel)", flex: "none", fontSize: 9.5, color: "var(--ink-4)" }} className="tnum">
        <span>Ln {cursor.line}, Col {cursor.col}</span>
        <span>{lines.length} lines</span>
        <span>{lang}</span>
        <span>LF · UTF-8 · spaces: 2</span>
        {hunks.filter((h) => h.kind !== "git").length > 0 && <span style={{ color: "var(--accent-2)" }}>{hunks.filter((h) => h.kind !== "git").length} local hunks</span>}
        <span style={{ marginLeft: "auto", color: dirty ? "#f5c451" : "var(--st-done)" }}>
          {dirty ? (autosave ? "autosaving in 2s…" : "unsaved — ⌘S") : "saved"}
        </span>
      </div>
    </div>
  );
}

function Breadcrumbs({ path, agent, symbol, right }) {
  const parts = path.split("/");
  const proj = agent && window.projectOf ? window.projectOf(agent.projectId) : null;
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "6px 12px", background: "var(--panel)", borderBottom: "1px solid var(--hair)", flex: "none", fontSize: 11, overflow: "hidden" }}>
      {proj && <span style={{ display: "flex", alignItems: "center", gap: 5, color: proj.color, flex: "none" }}><Icon name={proj.icon} size="sm" style={{ width: 12, height: 12 }} />{proj.name}</span>}
      {parts.map((p, i) => (
        <React.Fragment key={i}>
          <Icon name="chevron" size="sm" style={{ width: 10, height: 10, color: "var(--ink-4)", flex: "none" }} />
          <span style={{ color: i === parts.length - 1 ? "var(--ink)" : "var(--ink-3)", flex: "none", whiteSpace: "nowrap" }}>{p}</span>
        </React.Fragment>
      ))}
      {symbol && (
        <React.Fragment>
          <Icon name="chevron" size="sm" style={{ width: 10, height: 10, color: "var(--ink-4)", flex: "none" }} />
          <span style={{ display: "flex", alignItems: "center", gap: 4, color: "var(--accent-2)", flex: "none" }}>
            <Icon name={SYM_ICON[symbol.kind] || "spark"} size="sm" style={{ width: 11, height: 11 }} />{symbol.name}
          </span>
        </React.Fragment>
      )}
      <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 7, flex: "none" }}>{right}</div>
    </div>
  );
}

Object.assign(window, { EditorPane, StructureView, Breadcrumbs, AssetPreview, diffMarks });
