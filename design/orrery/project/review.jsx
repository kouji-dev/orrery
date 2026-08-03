/* global React, Icon, fileName, fileDir, highlight */
// Orrery — Inline review → send to agent.
// The reviewer (human) drops GitHub-style line/range comments while reading a
// diff or file; comments accumulate per-agent across files; the whole batch
// (plus a global note) is sent to the live local agent as one structured msg.

const { useState: useStateRv, useEffect: useEffectRv, useRef: useRefRv } = React;

// ========================================================================
// store — comments are keyed by agentId so the count follows the user across
// files. Persisted to localStorage; subscribers re-render on change.
// ========================================================================
const REVIEW_LS = "orrery.review.v2";
const reviewState = (() => { try { return JSON.parse(localStorage.getItem(REVIEW_LS)) || {}; } catch (e) { return {}; } })();
const reviewSubs = new Set();
function reviewPersist() {
  try { localStorage.setItem(REVIEW_LS, JSON.stringify(reviewState)); } catch (e) { /* ignore */ }
  reviewSubs.forEach((f) => f());
}
function reviewSlot(agentId) {
  if (!reviewState[agentId]) reviewState[agentId] = { comments: [], seq: 0 };
  return reviewState[agentId];
}
function reviewGet(agentId) { return (reviewState[agentId] && reviewState[agentId].comments) || []; }
function reviewAdd(agentId, c) {
  const slot = reviewSlot(agentId);
  const id = "rc" + (++slot.seq);
  slot.comments.push(Object.assign({ id }, c));
  reviewPersist();
  return id;
}
function reviewRemove(agentId, id) {
  const slot = reviewSlot(agentId);
  slot.comments = slot.comments.filter((c) => c.id !== id);
  reviewPersist();
}
function reviewClear(agentId) { if (reviewState[agentId]) { reviewState[agentId].comments = []; reviewPersist(); } }

function useReviewComments(agentId) {
  const [, force] = useStateRv(0);
  useEffectRv(() => {
    const f = () => force((x) => x + 1);
    reviewSubs.add(f);
    return () => reviewSubs.delete(f);
  }, []);
  return reviewGet(agentId);
}

// reference label exactly like the structured message (file:line or file:from-to)
function refLines(c) { return c.fromLine === c.toLine ? "" + c.fromLine : c.fromLine + "-" + c.toLine; }
function isBlock(c) { return c.toIdx > c.fromIdx; }

// ========================================================================
// row builders — normalize a diff or a plain file into a flat row list.
// type:"hunk" rows are separators (not commentable); type:"code" rows are.
// ========================================================================
function diffToRows(diff) {
  const rows = [];
  diff.hunks.forEach((h, hi) => {
    rows.push({ type: "hunk", meta: h.meta });
    h.lines.forEach((ln) => {
      rows.push({ type: "code", k: ln.k, n: ln.n, s: ln.s, side: ln.k === "-" ? "old" : "new" });
    });
  });
  return rows;
}
function fileToRows(lines) {
  return lines.map((ln) => ({ type: "code", k: ln.add ? "+" : " ", n: ln.n, s: ln.s, side: "file" }));
}

// ========================================================================
// ReviewCode — the diff/file renderer with gutter "+", drag-range select,
// inline composer, saved-comment markers + persistent inline cards.
// ========================================================================
function ReviewCode({ agentId, file, view, rows, lang, sticky }) {
  const all = useReviewComments(agentId);
  const comments = all.filter((c) => c.file === file && c.view === view);
  const [hover, setHover] = useStateRv(-1);
  const [drag, setDrag] = useStateRv(null);       // { anchor, current }
  const [composer, setComposer] = useStateRv(null); // { fromIdx, toIdx }
  const [draft, setDraft] = useStateRv("");
  const dragRef = useRefRv(null); dragRef.current = drag;
  const taRef = useRefRv(null);

  // finish a gutter drag → open one composer for the selected range
  useEffectRv(() => {
    if (!drag) return;
    const up = () => {
      const d = dragRef.current;
      if (d) { const a = Math.min(d.anchor, d.current), b = Math.max(d.anchor, d.current); openComposer(a, b); }
      setDrag(null);
    };
    window.addEventListener("mouseup", up);
    return () => window.removeEventListener("mouseup", up);
  }, [drag]);

  useEffectRv(() => { if (composer && taRef.current) taRef.current.focus(); }, [composer]);

  const openComposer = (a, b) => { setComposer({ fromIdx: a, toIdx: b }); setDraft(""); };
  const cancel = () => { setComposer(null); setDraft(""); };
  const save = () => {
    if (!composer || !draft.trim()) return;
    const block = rows.slice(composer.fromIdx, composer.toIdx + 1).filter((r) => r.type === "code");
    const fromLine = block[0] ? block[0].n : null;
    const toLine = block[block.length - 1] ? block[block.length - 1].n : fromLine;
    reviewAdd(agentId, {
      file, view, lang,
      fromIdx: composer.fromIdx, toIdx: composer.toIdx,
      fromLine, toLine,
      side: block[0] ? block[0].side : "new",
      snippet: (block[0] ? block[0].s : "").trim(),
      lines: block.map((r) => r.s),
      note: draft.trim(),
    });
    cancel();
  };

  const commentCovering = (i) => comments.find((c) => i >= c.fromIdx && i <= c.toIdx);
  const inSelection = (i) => drag && i >= Math.min(drag.anchor, drag.current) && i <= Math.max(drag.anchor, drag.current);
  const inComposer = (i) => composer && i >= composer.fromIdx && i <= composer.toIdx;

  const startDrag = (i, e) => { e.preventDefault(); e.stopPropagation(); if (composer) return; setDrag({ anchor: i, current: i }); };

  return (
    <div className="scroll-y" style={{ flex: 1, background: "var(--bg)", minHeight: 0, userSelect: drag ? "none" : "auto" }}>
      {sticky}
      <div style={{ position: "relative" }}>
        {rows.map((r, i) => {
          if (r.type === "hunk") {
            return (
              <div key={i} style={{ display: "flex", alignItems: "center", padding: "5px 14px 5px 38px", fontSize: 11,
                color: "var(--accent-2)", background: "color-mix(in oklch, var(--accent-2), transparent 93%)", borderTop: i ? "1px solid var(--hair)" : "none" }}>
                <span className="tnum" style={{ opacity: 0.95 }}>{r.meta}</span>
              </div>
            );
          }
          const k = r.k;
          const covering = commentCovering(i);
          const sel = inSelection(i) || inComposer(i);
          const codeBg = sel ? "color-mix(in oklch, var(--accent), transparent 84%)"
            : k === "+" ? "var(--code-add-bg)" : k === "-" ? "var(--code-del-bg)" : "transparent";
          const showPlus = hover === i && !composer && !drag && !covering;
          const isAnchor = covering && covering.fromIdx === i;
          return (
            <React.Fragment key={i}>
              <div onMouseEnter={() => { setHover(i); if (dragRef.current) setDrag((d) => d ? { ...d, current: i } : d); }}
                onMouseLeave={() => setHover((h) => (h === i ? -1 : h))}
                style={{ display: "flex", fontSize: 12, lineHeight: 1.7, background: codeBg,
                  boxShadow: covering ? "inset 2px 0 0 var(--accent)" : "none" }}>
                {/* comment gutter — hover "+", drag handle, saved marker */}
                <span style={{ width: 24, flex: "none", display: "flex", alignItems: "center", justifyContent: "center", userSelect: "none",
                  background: sel ? "color-mix(in oklch, var(--accent), transparent 70%)" : "transparent", cursor: showPlus ? "pointer" : "default" }}>
                  {covering
                    ? (isAnchor
                        ? <span title="comment saved" style={{ width: 15, height: 15, borderRadius: 4, display: "grid", placeItems: "center",
                            background: "var(--accent)", color: "#06070b" }}><Icon name="chat" size="sm" style={{ width: 10, height: 10 }} /></span>
                        : <span style={{ width: 5, height: 5, borderRadius: "50%", background: "color-mix(in oklch, var(--accent), transparent 30%)" }} />)
                    : showPlus
                      ? <button title="Comment on this line — or drag down to select a range" onMouseDown={(e) => startDrag(i, e)}
                          style={{ width: 16, height: 16, borderRadius: 4, border: "none", display: "grid", placeItems: "center", cursor: "grab",
                            background: "var(--accent)", color: "#06070b", padding: 0 }}>
                          <Icon name="plus" size="sm" style={{ width: 11, height: 11 }} />
                        </button>
                      : null}
                </span>
                <span className="tnum" style={{ width: 40, flex: "none", textAlign: "right", padding: "0 10px 0 0", color: "var(--ink-4)", userSelect: "none" }}>{r.n}</span>
                {view === "diff" && <span style={{ width: 16, flex: "none", textAlign: "center", userSelect: "none",
                  color: k === "+" ? "var(--code-add-ink)" : k === "-" ? "var(--code-del-ink)" : "var(--ink-4)" }}>{k === " " ? "" : k}</span>}
                <span style={{ flex: 1, whiteSpace: "pre-wrap", wordBreak: "break-word", paddingRight: 14, paddingLeft: view === "diff" ? 0 : 14,
                  color: k === "+" ? "var(--code-add-ink)" : k === "-" ? "var(--code-del-ink)" : "var(--ink-2)" }}>
                  {view === "file" && typeof highlight === "function" ? highlight(r.s, lang) : (r.s || " ")}
                </span>
              </div>

              {/* persistent saved-comment card (under its last line) */}
              {comments.filter((c) => c.toIdx === i).map((c) => (
                <SavedCommentCard key={c.id} c={c} onDelete={() => reviewRemove(agentId, c.id)} />
              ))}

              {/* inline composer (under the range's last line) */}
              {composer && composer.toIdx === i && (
                <InlineComposer range={composer} rows={rows} draft={draft} setDraft={setDraft} taRef={taRef} onCancel={cancel} onSave={save} />
              )}
            </React.Fragment>
          );
        })}
      </div>
    </div>
  );
}

function rangeLabel(range, rows) {
  const block = rows.slice(range.fromIdx, range.toIdx + 1).filter((r) => r.type === "code");
  if (!block.length) return "";
  const a = block[0].n, b = block[block.length - 1].n;
  return a === b ? "line " + a : "lines " + a + "–" + b;
}

function InlineComposer({ range, rows, draft, setDraft, taRef, onCancel, onSave }) {
  return (
    <div style={{ display: "flex", background: "color-mix(in oklch, var(--accent), transparent 95%)", borderTop: "1px solid color-mix(in oklch, var(--accent), transparent 70%)", borderBottom: "1px solid var(--hair)" }}>
      <span style={{ width: 24, flex: "none", background: "color-mix(in oklch, var(--accent), transparent 70%)" }} />
      <div style={{ flex: 1, padding: "10px 14px 10px 12px" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 7, marginBottom: 7 }}>
          <Icon name="chat" size="sm" style={{ color: "var(--accent)" }} />
          <span style={{ fontSize: 10.5, color: "var(--ink-2)" }}>Comment on <b style={{ color: "var(--ink)" }}>{rangeLabel(range, rows)}</b></span>
          <span className="up" style={{ marginLeft: "auto", fontSize: 8.5, color: "var(--ink-4)" }}>review · queued for agent</span>
        </div>
        <textarea ref={taRef} value={draft} onChange={(e) => setDraft(e.target.value)} rows={3}
          placeholder="Leave a note for the agent — what to change and why…"
          onKeyDown={(e) => { if ((e.metaKey || e.ctrlKey) && e.key === "Enter") onSave(); if (e.key === "Escape") onCancel(); }}
          style={{ width: "100%", resize: "vertical", minHeight: 56, background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-sm)",
            padding: "8px 10px", color: "var(--ink)", fontFamily: "var(--font-mono)", fontSize: 12, lineHeight: 1.55, outline: "none" }}
          onFocus={(e) => (e.target.style.borderColor = "color-mix(in oklch, var(--accent), transparent 45%)")}
          onBlur={(e) => (e.target.style.borderColor = "var(--hair)")} />
        <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 8 }}>
          <span style={{ fontSize: 9.5, color: "var(--ink-4)" }}><span className="kbd">⌘</span> <span className="kbd">↵</span> to save</span>
          <div style={{ marginLeft: "auto", display: "flex", gap: 7 }}>
            <button className="btn ghost-hair" onClick={onCancel} style={{ padding: "4px 12px", fontSize: 11 }}>Cancel</button>
            <button className="btn primary" onClick={onSave} disabled={!draft.trim()} style={{ padding: "4px 12px", fontSize: 11 }}>Save</button>
          </div>
        </div>
      </div>
    </div>
  );
}

function SavedCommentCard({ c, onDelete }) {
  return (
    <div style={{ display: "flex", background: "var(--panel)", borderTop: "1px solid var(--hair)", borderBottom: "1px solid var(--hair)" }}>
      <span style={{ width: 24, flex: "none", background: "color-mix(in oklch, var(--accent), transparent 86%)", boxShadow: "inset 2px 0 0 var(--accent)" }} />
      <div style={{ flex: 1, padding: "9px 14px 9px 12px", minWidth: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 7 }}>
          <span style={{ width: 17, height: 17, flex: "none", borderRadius: "50%", display: "grid", placeItems: "center", fontSize: 8.5, fontWeight: 700,
            color: "var(--accent)", background: "color-mix(in oklch, var(--accent), transparent 84%)", border: "1px solid color-mix(in oklch, var(--accent), transparent 60%)" }}>YOU</span>
          <span style={{ fontSize: 11, color: "var(--ink-2)" }}>You</span>
          <span className="tnum" style={{ fontSize: 9.5, color: "var(--ink-4)" }}>· {isBlock(c) ? "lines " + refLines(c) : "line " + c.fromLine}</span>
          <span className="chip" style={{ fontSize: 8.5, padding: "0 5px", color: "var(--accent)", borderColor: "color-mix(in oklch, var(--accent), transparent 60%)" }}>pending</span>
          <button onClick={onDelete} title="Delete comment"
            style={{ marginLeft: "auto", background: "transparent", border: "none", color: "var(--ink-4)", cursor: "pointer", display: "flex", padding: 2, borderRadius: 3 }}
            onMouseEnter={(e) => (e.currentTarget.style.color = "var(--st-blocked)")}
            onMouseLeave={(e) => (e.currentTarget.style.color = "var(--ink-4)")}>
            <Icon name="trash" size="sm" />
          </button>
        </div>
        <div style={{ fontSize: 12, color: "var(--ink)", lineHeight: 1.5, marginTop: 5, whiteSpace: "pre-wrap", wordBreak: "break-word" }}>{c.note}</div>
      </div>
    </div>
  );
}

// ========================================================================
// SendReviewButton — top-toolbar action; hidden at N=0, shows the count.
// ========================================================================
function SendReviewButton({ agentId, onOpen }) {
  const comments = useReviewComments(agentId);
  const n = comments.length;
  if (!n) return null;
  return (
    <button className="btn" onClick={onOpen} title="Review all pending comments and send them to this agent"
      style={{ padding: "5px 11px", color: "var(--ink)", border: "1px solid color-mix(in oklch, var(--accent), transparent 55%)",
        background: "color-mix(in oklch, var(--accent), transparent 86%)" }}>
      <Icon name="chat" size="sm" style={{ color: "var(--accent)" }} />Send review
      <span className="tnum" style={{ fontSize: 10, fontWeight: 700, color: "#06070b", background: "var(--accent)", borderRadius: 999, padding: "1px 6px", marginLeft: 2 }}>{n}</span>
    </button>
  );
}

// ========================================================================
// SendReviewModal — pending comments grouped by file + global note + send.
// Its body mirrors the structured message the agent ultimately receives.
// ========================================================================
function SendReviewModal({ agentId, agentName, onClose, onSend }) {
  const comments = useReviewComments(agentId);
  const [global, setGlobal] = useStateRv("");
  // group by file, keep first-seen order
  const groups = [];
  const byFile = {};
  comments.forEach((c) => { if (!byFile[c.file]) { byFile[c.file] = { file: c.file, items: [] }; groups.push(byFile[c.file]); } byFile[c.file].items.push(c); });

  const send = () => {
    onSend(agentId, { comments: comments.map((c) => ({ file: c.file, fromLine: c.fromLine, toLine: c.toLine, snippet: c.snippet, note: c.note, block: isBlock(c) })), global: global.trim() });
    reviewClear(agentId);
    onClose();
  };

  return (
    <div onClick={onClose} style={{ position: "fixed", inset: 0, zIndex: 70, display: "grid", placeItems: "center", padding: 24, background: "rgba(0,0,0,0.5)", backdropFilter: "blur(3px)" }}>
      <div className="surface rise" onClick={(e) => e.stopPropagation()} style={{ width: 600, maxHeight: "88vh", display: "flex", flexDirection: "column", padding: 0, overflow: "hidden", boxShadow: "var(--shadow)" }}>
        {/* header */}
        <div style={{ padding: "14px 18px", borderBottom: "1px solid var(--hair)", display: "flex", alignItems: "center", gap: 9, flex: "none" }}>
          <Icon name="chat" style={{ color: "var(--accent)" }} />
          <span className="disp" style={{ fontSize: 14, fontWeight: 600 }}>Send review</span>
          <span className="tnum" style={{ fontSize: 11, color: "var(--ink-3)" }}>{comments.length} comment{comments.length !== 1 ? "s" : ""} · {groups.length} file{groups.length !== 1 ? "s" : ""}</span>
          <span className="chip" style={{ marginLeft: "auto", fontSize: 9.5 }}>→ {agentName || agentId}</span>
          <button onClick={onClose} style={{ background: "transparent", border: "none", color: "var(--ink-4)", cursor: "pointer", display: "flex", padding: 3, borderRadius: 4 }}
            onMouseEnter={(e) => (e.currentTarget.style.color = "var(--ink)")} onMouseLeave={(e) => (e.currentTarget.style.color = "var(--ink-4)")}>
            <Icon name="x" size="sm" />
          </button>
        </div>

        {/* grouped comment list */}
        <div className="scroll-y" style={{ flex: 1, minHeight: 0, padding: "6px 0" }}>
          {groups.length === 0
            ? <div style={{ display: "grid", placeItems: "center", color: "var(--ink-4)", fontSize: 12, padding: 40 }}>no comments yet — hover a line and click the + to add one</div>
            : groups.map((g) => (
              <div key={g.file} style={{ marginBottom: 4 }}>
                <div style={{ display: "flex", alignItems: "center", gap: 7, padding: "7px 18px", position: "sticky", top: 0, background: "var(--panel)", zIndex: 1 }}>
                  <Icon name="file" size="sm" style={{ color: "var(--ink-3)" }} />
                  <span style={{ fontSize: 11, color: "var(--ink-4)" }}>{fileDir(g.file)}</span>
                  <span style={{ fontSize: 11.5, color: "var(--ink)", marginLeft: -3 }}>{fileName(g.file)}</span>
                  <span className="tnum" style={{ marginLeft: "auto", fontSize: 9.5, color: "var(--ink-4)" }}>{g.items.length}</span>
                </div>
                {g.items.map((c) => (
                  <div key={c.id} style={{ display: "flex", gap: 10, padding: "8px 18px 10px", margin: "0 10px", borderRadius: "var(--r-sm)" }}
                    onMouseEnter={(e) => (e.currentTarget.style.background = "var(--panel-2)")}
                    onMouseLeave={(e) => (e.currentTarget.style.background = "transparent")}>
                    <span className="tnum" style={{ flex: "none", width: 54, paddingTop: 2, fontSize: 10.5, color: "var(--accent-2)", textAlign: "right" }}>:{refLines(c)}</span>
                    <div style={{ flex: 1, minWidth: 0 }}>
                      <div className="tnum" style={{ fontSize: 11, color: "var(--ink-3)", background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: 4,
                        padding: "4px 8px", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                        {isBlock(c) ? <span style={{ color: "var(--ink-4)" }}>{c.lines.length} lines · </span> : null}{c.snippet || "(blank line)"}
                      </div>
                      <div style={{ display: "flex", gap: 6, marginTop: 6 }}>
                        <span style={{ color: "var(--accent)", flex: "none" }}>→</span>
                        <span style={{ fontSize: 12, color: "var(--ink)", lineHeight: 1.5, whiteSpace: "pre-wrap", wordBreak: "break-word" }}>{c.note}</span>
                      </div>
                    </div>
                    <button onClick={() => reviewRemove(agentId, c.id)} title="Delete comment"
                      style={{ flex: "none", background: "transparent", border: "none", color: "var(--ink-4)", cursor: "pointer", display: "flex", padding: 3, borderRadius: 3, alignSelf: "flex-start" }}
                      onMouseEnter={(e) => (e.currentTarget.style.color = "var(--st-blocked)")} onMouseLeave={(e) => (e.currentTarget.style.color = "var(--ink-4)")}>
                      <Icon name="trash" size="sm" />
                    </button>
                  </div>
                ))}
              </div>
            ))}
        </div>

        {/* global note */}
        <div style={{ padding: "12px 18px", borderTop: "1px solid var(--hair)", flex: "none", background: "var(--panel-2)" }}>
          <div style={{ display: "flex", alignItems: "center", gap: 6, marginBottom: 7 }}>
            <span className="up" style={{ fontSize: 9, color: "var(--ink-3)" }}>Global note</span>
            <span style={{ fontSize: 9.5, color: "var(--ink-4)" }}>· applies to the whole review</span>
          </div>
          <textarea value={global} onChange={(e) => setGlobal(e.target.value)} rows={2}
            placeholder="Overall direction for this pass — e.g. tighten error handling and keep the public API stable…"
            style={{ width: "100%", resize: "none", background: "var(--panel)", border: "1px solid var(--hair)", borderRadius: "var(--r-sm)",
              padding: "9px 11px", color: "var(--ink)", fontFamily: "var(--font-mono)", fontSize: 12, lineHeight: 1.5, outline: "none" }}
            onFocus={(e) => (e.target.style.borderColor = "color-mix(in oklch, var(--accent), transparent 45%)")}
            onBlur={(e) => (e.target.style.borderColor = "var(--hair)")} />
        </div>

        {/* footer */}
        <div style={{ padding: "12px 18px", borderTop: "1px solid var(--hair)", display: "flex", alignItems: "center", gap: 8, flex: "none" }}>
          <span style={{ fontSize: 10, color: "var(--ink-4)" }}>delivered to the agent as one structured message</span>
          <div style={{ marginLeft: "auto", display: "flex", gap: 8 }}>
            <button className="btn ghost-hair" onClick={onClose}>Cancel</button>
            <button className="btn primary" onClick={send} disabled={!comments.length}><Icon name="enter" size="sm" />Send to agent</button>
          </div>
        </div>
      </div>
    </div>
  );
}

Object.assign(window, {
  ReviewCode, SendReviewButton, SendReviewModal,
  diffToRows, fileToRows, reviewGet,
});
