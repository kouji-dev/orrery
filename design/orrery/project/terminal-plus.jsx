/* global React, Icon, logColor, logPrefix, fileName */
// Orrery — terminal stack: arbitrary splits inside one agent pane (B5.1),
// scrollback search (B5.3) and a prompt composer that accepts dropped files (B1.5).

const { useState: useStateT2, useEffect: useEffectT2, useRef: useRefT2, useMemo: useMemoT2 } = React;

let tlSeq = 1;
const leaf = () => ({ t: "leaf", id: "t" + (++tlSeq) });

function splitNode(node, targetId, dir) {
  if (node.t === "leaf") return node.id === targetId ? { t: "split", dir, a: node, b: leaf() } : node;
  return { ...node, a: splitNode(node.a, targetId, dir), b: splitNode(node.b, targetId, dir) };
}
function dropNode(node, targetId) {
  if (node.t === "leaf") return node.id === targetId ? null : node;
  const a = dropNode(node.a, targetId), b = dropNode(node.b, targetId);
  if (!a) return b; if (!b) return a;
  return { ...node, a, b };
}
const leafIds = (node) => node.t === "leaf" ? [node.id] : [...leafIds(node.a), ...leafIds(node.b)];

// ---- one terminal: scrollback + search + optional composer
function TerminalLeaf({ ag, lines, streaming, focused, onFocus, onSplit, onClose, canClose, index, composer, onSend, onFlash }) {
  const [search, setSearch] = useStateT2(null);   // null | {q, idx}
  const body = useRefT2(null);
  const rows = useRefT2({});
  const [draft, setDraft] = useStateT2("");
  const [dragOver, setDragOver] = useStateT2(false);
  const inputRef = useRefT2(null);

  useEffectT2(() => { if (body.current && !search) body.current.scrollTop = body.current.scrollHeight; }, [lines.length]);
  useEffectT2(() => {
    const open = () => { if (focused) setSearch((s) => s || { q: "", idx: 0 }); };
    window.addEventListener("orrery:scrollback-search", open);
    return () => window.removeEventListener("orrery:scrollback-search", open);
  }, [focused]);

  const hits = useMemoT2(() => {
    if (!search || !search.q) return [];
    const q = search.q.toLowerCase();
    return lines.map((l, i) => (l.s || "").toLowerCase().includes(q) ? i : -1).filter((i) => i >= 0);
  }, [search && search.q, lines.length]);
  const cur = hits.length ? hits[Math.min(search.idx, hits.length - 1) % hits.length] : -1;
  useEffectT2(() => {
    if (cur < 0) return;
    const el = rows.current[cur];
    if (el && body.current) body.current.scrollTop = Math.max(0, el.offsetTop - body.current.clientHeight / 2);
  }, [cur]);

  const hl = (s) => {
    if (!search || !search.q) return s;
    const q = search.q, out = [], low = (s || "").toLowerCase(), lq = q.toLowerCase();
    let at = 0, i;
    while ((i = low.indexOf(lq, at)) >= 0) {
      if (i > at) out.push(<span key={at}>{s.slice(at, i)}</span>);
      out.push(<mark key={"m" + i} style={{ background: "color-mix(in oklch, var(--accent), transparent 55%)", color: "var(--ink)", borderRadius: 2 }}>{s.slice(i, i + q.length)}</mark>);
      at = i + q.length;
    }
    out.push(<span key="t">{s.slice(at)}</span>);
    return out;
  };

  const send = () => {
    const v = draft.trim();
    if (!v) return;
    onSend(ag.id, v);
    setDraft("");
  };

  return (
    <div onMouseDown={onFocus} style={{ flex: 1, minWidth: 0, minHeight: 0, display: "flex", flexDirection: "column",
      outline: focused ? "1px solid color-mix(in oklch, var(--accent), transparent 62%)" : "none", outlineOffset: -1, background: "var(--bg)" }}>
      {/* leaf toolbar */}
      <div style={{ display: "flex", alignItems: "center", gap: 7, padding: "3px 8px 3px 10px", background: "var(--panel)", borderBottom: "1px solid var(--hair)", flex: "none" }}>
        <Icon name="terminal" size="sm" style={{ width: 12, height: 12, color: focused ? "var(--accent)" : "var(--ink-4)" }} />
        <span style={{ fontSize: 10, color: "var(--ink-3)" }}>{index === 0 ? "pty · " + ag.worktree : "shell " + (index + 1) + " · " + ag.worktree}</span>
        {streaming && index === 0 && <span className="dot running" style={{ background: "var(--st-running)" }} />}
        <div style={{ marginLeft: "auto", display: "flex", gap: 2 }}>
          <button className="pane-btn" title="Search scrollback" onClick={() => setSearch((s) => s ? null : { q: "", idx: 0 })}><Icon name="search" size="sm" style={{ width: 12, height: 12 }} /></button>
          <button className="pane-btn" title="Split right" onClick={() => onSplit("v")}><Icon name="splitCol" size="sm" style={{ width: 12, height: 12 }} /></button>
          <button className="pane-btn" title="Split down" onClick={() => onSplit("h")}><Icon name="splitRow" size="sm" style={{ width: 12, height: 12 }} /></button>
          {canClose && <button className="pane-btn" title="Close this shell" onClick={onClose}><Icon name="x" size="sm" style={{ width: 12, height: 12 }} /></button>}
        </div>
      </div>

      {search && (
        <div style={{ display: "flex", alignItems: "center", gap: 7, padding: "5px 9px", background: "var(--panel-2)", borderBottom: "1px solid var(--hair)", flex: "none" }}>
          <Icon name="search" size="sm" style={{ color: "var(--accent)" }} />
          <input autoFocus value={search.q} onChange={(e) => setSearch({ q: e.target.value, idx: 0 })}
            onKeyDown={(e) => { if (e.key === "Enter") setSearch((s) => ({ ...s, idx: s.idx + (e.shiftKey ? -1 : 1) })); if (e.key === "Escape") setSearch(null); }}
            placeholder="find in scrollback…"
            style={{ flex: 1, background: "transparent", border: "none", outline: "none", color: "var(--ink)", fontFamily: "var(--font-mono)", fontSize: 11.5 }} />
          <span className="tnum" style={{ fontSize: 10, color: hits.length ? "var(--ink-3)" : "var(--ink-4)" }}>
            {hits.length ? (Math.min(search.idx, hits.length - 1) % hits.length) + 1 + " / " + hits.length : search.q ? "no matches" : ""}
          </span>
          <button className="pane-btn" onClick={() => setSearch((s) => ({ ...s, idx: s.idx - 1 }))}><Icon name="chevron" size="sm" style={{ width: 12, height: 12, transform: "rotate(-90deg)" }} /></button>
          <button className="pane-btn" onClick={() => setSearch((s) => ({ ...s, idx: s.idx + 1 }))}><Icon name="chevron" size="sm" style={{ width: 12, height: 12, transform: "rotate(90deg)" }} /></button>
          <button className="pane-btn" onClick={() => setSearch(null)}><Icon name="x" size="sm" style={{ width: 12, height: 12 }} /></button>
        </div>
      )}

      <div ref={body} className="scroll-y" style={{ flex: 1, minHeight: 0, padding: "10px 14px", fontSize: 12, lineHeight: 1.7 }}>
        {index === 0 && <div style={{ color: "var(--ink-4)", marginBottom: 8, fontSize: 10.5 }}>── session: {ag.worktree} · {ag.branch} ──</div>}
        {index > 0 && <div style={{ color: "var(--ink-4)", marginBottom: 8, fontSize: 10.5 }}>── new shell in {ag.worktree} ──</div>}
        {(index === 0 ? lines : lines.slice(-3)).map((l, i) => (
          <div key={i} ref={(el) => { rows.current[i] = el; }}
            style={{ display: "flex", gap: 9, color: logColor(l.t), whiteSpace: "pre-wrap", wordBreak: "break-word",
              background: cur === i ? "color-mix(in oklch, var(--accent), transparent 80%)" : "transparent" }}>
            <span style={{ color: l.t === "cmd" ? "var(--accent)" : "var(--ink-4)", flex: "none", userSelect: "none", width: 10, textAlign: "center" }}>{logPrefix(l.t)}</span>
            <span style={{ flex: 1 }}>{hl(l.s)}</span>
          </div>
        ))}
        {streaming && index === 0 && (
          <div style={{ display: "flex", gap: 9, color: "var(--accent)" }}>
            <span style={{ width: 10, textAlign: "center" }}>$</span><span className="caret" />
          </div>
        )}
      </div>

      {composer && (
        <div onDragOver={(e) => { e.preventDefault(); setDragOver(true); }} onDragLeave={() => setDragOver(false)}
          onDrop={(e) => {
            e.preventDefault(); setDragOver(false);
            const d = window.__omDrag;
            const paths = [];
            if (d && d.kind === "file" && d.path) paths.push(d.path);
            const txt = e.dataTransfer.getData("text/plain");
            if (!paths.length && txt) paths.push(txt);
            if (e.dataTransfer.files && e.dataTransfer.files.length) [...e.dataTransfer.files].forEach((f) => paths.push(f.name + " (attached)"));
            if (!paths.length) return;
            setDraft((v) => (v ? v.trim() + " " : "") + paths.map((p) => "@" + p).join(" ") + " ");
            if (inputRef.current) inputRef.current.focus();
            onFlash && onFlash("referenced " + paths.length + " file" + (paths.length === 1 ? "" : "s") + " in the prompt");
          }}
          style={{ display: "flex", alignItems: "center", gap: 8, padding: "7px 10px", borderTop: "1px solid " + (dragOver ? "var(--accent)" : "var(--hair)"),
            background: dragOver ? "color-mix(in oklch, var(--accent), transparent 90%)" : "var(--panel)", flex: "none" }}>
          <Icon name="chat" size="sm" style={{ color: dragOver ? "var(--accent)" : "var(--ink-4)" }} />
          <input ref={inputRef} value={draft} onChange={(e) => setDraft(e.target.value)}
            onKeyDown={(e) => { if (e.key === "Enter" && !e.shiftKey) { e.preventDefault(); send(); } }}
            placeholder={dragOver ? "drop to reference the file" : "message " + ag.name + " · drag files in to reference them"}
            style={{ flex: 1, minWidth: 0, background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-sm)", padding: "6px 9px",
              color: "var(--ink)", fontFamily: "var(--font-mono)", fontSize: 11.5, outline: "none" }} />
          <button className="btn primary" disabled={!draft.trim()} onClick={send} style={{ padding: "5px 10px", fontSize: 11 }}>
            <Icon name="enter" size="sm" />Send
          </button>
        </div>
      )}
    </div>
  );
}

function TerminalStack({ ag, lines, streaming, onFlash, onSend }) {
  const [tree, setTree] = useStateT2(() => leaf());
  const [focus, setFocus] = useStateT2(null);
  const ids = leafIds(tree);
  const focused = focus && ids.includes(focus) ? focus : ids[0];
  useEffectT2(() => {
    const onSplitEvt = (e) => setTree((t) => splitNode(t, focused, (e.detail && e.detail.dir) || "v"));
    window.addEventListener("orrery:split", onSplitEvt);
    return () => window.removeEventListener("orrery:split", onSplitEvt);
  }, [focused]);
  useEffectT2(() => { setTree(leaf()); }, [ag.id]);

  const render = (node) => {
    if (node.t === "leaf") {
      const i = ids.indexOf(node.id);
      return (
        <TerminalLeaf key={node.id} ag={ag} lines={lines} streaming={streaming} index={i}
          focused={focused === node.id} onFocus={() => setFocus(node.id)}
          onSplit={(dir) => { setTree((t) => splitNode(t, node.id, dir)); onFlash && onFlash("split terminal " + (dir === "v" ? "right" : "down")); }}
          onClose={() => setTree((t) => dropNode(t, node.id) || leaf())} canClose={ids.length > 1}
          composer={i === 0} onSend={onSend} onFlash={onFlash} />
      );
    }
    return (
      <div key={node.a.id + node.b.id} style={{ flex: 1, minWidth: 0, minHeight: 0, display: "flex", flexDirection: node.dir === "v" ? "row" : "column" }}>
        {render(node.a)}
        <div style={{ flex: "none", background: "var(--hair)", width: node.dir === "v" ? 1 : "auto", height: node.dir === "v" ? "auto" : 1 }} />
        {render(node.b)}
      </div>
    );
  };
  return <div style={{ flex: 1, minHeight: 0, display: "flex" }}>{render(tree)}</div>;
}

Object.assign(window, { TerminalStack, TerminalLeaf });
