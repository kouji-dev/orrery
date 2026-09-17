/* global React, Icon, StatusDot, ToolBadge, toolMeta, STATUS_META, projectOf, RichEditor, RichView, TICKET_COL, TICKET_ORDER, PROJECTS */
// ── TicketPage — dedicated inline page: details + rich notes + comments ──────
const { useState: useStateTP, useEffect: useEffectTP, useRef: useRefTP } = React;

function initials(name) {
  const parts = name.trim().split(/\s+/);
  return ((parts[0] || "")[0] || "?").toUpperCase() + (parts.length > 1 ? (parts[parts.length - 1][0] || "").toUpperCase() : "");
}
const AV_COLORS = ["#a855f7", "#22d3ee", "#34e0a1", "#ff6b35", "#ff4d8d", "#3b82f6", "#f5c451"];
function avColor(name) { let h = 0; for (let i = 0; i < name.length; i++) h = (h * 31 + name.charCodeAt(i)) >>> 0; return AV_COLORS[h % AV_COLORS.length]; }

function Avatar({ name, agentTool, size = 28 }) {
  if (agentTool) return <ToolBadge tool={agentTool} size={size} />;
  const c = avColor(name);
  return (
    <span style={{ width: size, height: size, flex: "none", borderRadius: "50%", display: "grid", placeItems: "center",
      fontFamily: "var(--font-disp)", fontSize: size * 0.36, fontWeight: 600, color: c,
      background: "color-mix(in oklch, " + c + ", transparent 84%)", border: "1px solid color-mix(in oklch, " + c + ", transparent 62%)" }}>{initials(name)}</span>
  );
}

// ── one posted comment ───────────────────────────────────────────────────────
function Comment({ c }) {
  const isAgent = c.role === "agent";
  return (
    <div style={{ display: "flex", gap: 11 }}>
      <Avatar name={c.author} agentTool={isAgent ? c.tool : null} size={28} />
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, marginBottom: 5 }}>
          <span className="disp" style={{ fontSize: 12.5, fontWeight: 600, color: "var(--ink)" }}>{c.author}</span>
          {isAgent && <span className="chip" style={{ fontSize: 8.5, padding: "1px 6px", color: "var(--accent-2)", borderColor: "color-mix(in oklch, var(--accent-2), transparent 60%)" }}>AGENT</span>}
          <span style={{ fontSize: 10.5, color: "var(--ink-4)" }} className="tnum">{c.when}</span>
        </div>
        <div style={{ background: isAgent ? "color-mix(in oklch, var(--accent-2), transparent 94%)" : "var(--panel)", border: "1px solid var(--hair)",
          borderRadius: "var(--r-md)", padding: "9px 12px" }}>
          <RichView html={c.body} />
        </div>
      </div>
    </div>
  );
}

// ── side detail row ──────────────────────────────────────────────────────────
function DRow({ label, children }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
      <span className="field-label" style={{ margin: 0 }}>{label}</span>
      {children}
    </div>
  );
}

function StatusSeg({ status, onMove }) {
  return (
    <div style={{ display: "flex", gap: 4, padding: 3, background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-md)" }}>
      {TICKET_ORDER.map((s) => {
        const m = TICKET_COL[s], on = status === s;
        return (
          <button key={s} className="btn" onClick={() => onMove(s)}
            style={{ flex: 1, justifyContent: "center", padding: "5px 4px", fontSize: 10.5, borderRadius: "var(--r-sm)", gap: 5,
              background: on ? "color-mix(in oklch, " + m.color + ", transparent 86%)" : "transparent",
              color: on ? m.color : "var(--ink-3)", boxShadow: on ? "0 0 0 1px color-mix(in oklch, " + m.color + ", transparent 60%)" : "none" }}>
            <span className="dot" style={{ background: m.color, animation: "none", width: 6, height: 6 }} />{m.label}
          </button>
        );
      })}
    </div>
  );
}

function TicketPage({ ticket, draft, agents, projects, onCreate, onUpdate, onDelete, onDispatch, onOpenAgent, onMove, onAddComment, onClose }) {
  const [editing, setEditing] = useStateTP(!!draft);
  const [title, setTitle] = useStateTP(ticket.title || "");
  const [notes, setNotes] = useStateTP(ticket.notes || "");
  const [projectId, setProjectId] = useStateTP(ticket.projectId || "");
  const [comment, setComment] = useStateTP("");
  const [cKey, setCKey] = useStateTP(0);
  const titleRef = useRefTP(null);

  // reset local state when navigating to a different ticket
  useEffectTP(() => {
    setEditing(!!draft); setTitle(ticket.title || ""); setNotes(ticket.notes || "");
    setProjectId(ticket.projectId || ""); setComment(""); setCKey((k) => k + 1);
    // eslint-disable-next-line react-hooks/exhaustive-deps
  }, [ticket.id]);
  useEffectTP(() => { if (editing && draft && titleRef.current) titleRef.current.focus(); }, [editing, draft]);

  const agent = ticket.agentId ? agents.find((a) => a.id === ticket.agentId) : null;
  const m = TICKET_COL[ticket.status];
  const proj = projectOf(ticket.projectId);
  const comments = ticket.comments || [];

  const save = () => {
    const tt = title.trim(); if (!tt) return;
    if (draft) onCreate({ title: tt, notes, projectId: projectId || null });
    else { onUpdate(ticket.id, { title: tt, notes, projectId: projectId || null }); setEditing(false); }
  };
  const cancel = () => {
    if (draft) { onClose && onClose(); return; }
    setTitle(ticket.title || ""); setNotes(ticket.notes || ""); setProjectId(ticket.projectId || ""); setEditing(false);
  };
  const post = () => { const b = comment.trim(); if (!b) return; onAddComment(ticket.id, { body: comment }); setComment(""); setCKey((k) => k + 1); };

  return (
    <div style={{ display: "flex", flexDirection: "column", minHeight: 0, background: "var(--panel-2)" }}>
      {/* header */}
      <div style={{ padding: "12px 22px", borderBottom: "1px solid var(--hair)", background: "var(--panel)", flex: "none" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 9, fontSize: 11, color: "var(--ink-3)", marginBottom: 9 }}>
          <Icon name="layers" size="sm" style={{ color: "var(--accent)" }} />
          <span>Backlog</span>
          <Icon name="chevron" size="sm" style={{ width: 11, height: 11, color: "var(--ink-4)" }} />
          <span className="tnum">{draft ? "New ticket" : "#" + ticket.id.replace(/^t/, "")}</span>
          <span style={{ marginLeft: "auto", display: "flex", gap: 6 }} className="up">
            <span style={{ display: "inline-flex", alignItems: "center", gap: 6, fontSize: 9.5, padding: "3px 9px", borderRadius: 999, letterSpacing: ".1em",
              color: m.color, border: "1px solid color-mix(in oklch, " + m.color + ", transparent 62%)", background: "color-mix(in oklch, " + m.color + ", transparent 88%)" }}>
              <span className="dot" style={{ background: m.color, animation: "none" }} />{m.label}</span>
          </span>
        </div>

        {editing
          ? <input ref={titleRef} value={title} onChange={(e) => setTitle(e.target.value)} placeholder="Ticket title — what needs to be done?"
              onKeyDown={(e) => { if (e.key === "Enter") save(); }}
              style={{ width: "100%", background: "transparent", border: "none", outline: "none", color: "var(--ink)",
                fontFamily: "var(--font-disp)", fontSize: 22, fontWeight: 600, letterSpacing: "-0.02em", padding: 0 }} />
          : <h1 className="disp" style={{ fontSize: 22, fontWeight: 600, letterSpacing: "-0.02em", lineHeight: 1.25, textWrap: "balance" }}>{ticket.title}</h1>}

        <div style={{ display: "flex", alignItems: "center", gap: 8, marginTop: 12 }}>
          {editing ? (
            <React.Fragment>
              <button className="btn primary" disabled={!title.trim()} onClick={save}><Icon name={draft ? "plus" : "check"} size="sm" />{draft ? "Create ticket" : "Save"}</button>
              <button className="btn ghost-hair" onClick={cancel}>Cancel</button>
            </React.Fragment>
          ) : (
            <React.Fragment>
              {ticket.status === "todo" && <button className="btn primary" onClick={() => onDispatch(ticket)}><Icon name="bolt" size="sm" />Dispatch agent</button>}
              {agent && <button className="btn ghost-hair" onClick={() => onOpenAgent(agent.id)}><Icon name="enter" size="sm" />Open agent</button>}
              <button className="btn ghost-hair" onClick={() => setEditing(true)}><Icon name="rename" size="sm" />Edit</button>
              <button className="btn ghost-hair" onClick={() => onDelete(ticket.id)} style={{ color: "var(--ink-3)" }}
                onMouseEnter={(e) => e.currentTarget.style.color = "var(--st-blocked)"} onMouseLeave={(e) => e.currentTarget.style.color = "var(--ink-3)"}>
                <Icon name="trash" size="sm" />Delete</button>
            </React.Fragment>
          )}
        </div>
      </div>

      {/* body */}
      <div className="scroll-y" style={{ flex: 1 }}>
        <div style={{ display: "grid", gridTemplateColumns: "minmax(0,1fr) 268px", gap: 28, padding: "22px 24px", maxWidth: 1040, margin: "0 auto", alignItems: "start" }}>
          {/* main */}
          <div style={{ display: "flex", flexDirection: "column", gap: 24, minWidth: 0 }}>
            <div>
              <div className="up" style={{ fontSize: 10, color: "var(--ink-3)", marginBottom: 9 }}>What should be done</div>
              {editing
                ? <RichEditor value={notes} onChange={setNotes} minHeight={150} placeholder="Describe the work — context, acceptance criteria, links…" />
                : (plainTextNonEmpty(ticket.notes)
                    ? <RichView html={ticket.notes} style={{ fontSize: 12.5 }} />
                    : <div style={{ fontSize: 12, color: "var(--ink-4)", fontStyle: "italic" }}>No description yet.</div>)}
            </div>

            {/* comments */}
            {!draft && (
              <div style={{ display: "flex", flexDirection: "column", gap: 14 }}>
                <div className="up" style={{ fontSize: 10, color: "var(--ink-3)", display: "flex", alignItems: "center", gap: 7 }}>
                  <Icon name="chat" size="sm" />Comments<span className="chip tnum" style={{ fontSize: 9, padding: "0 6px" }}>{comments.length}</span>
                </div>
                {comments.map((c) => <Comment key={c.id} c={c} />)}
                {!comments.length && <div style={{ fontSize: 11.5, color: "var(--ink-4)" }}>No comments yet — start the thread.</div>}

                {/* composer */}
                <div style={{ display: "flex", gap: 11, marginTop: 2 }}>
                  <Avatar name="You" size={28} />
                  <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 8 }}>
                    <RichEditor value="" onChange={setComment} minHeight={70} compact resetSignal={cKey} placeholder="Leave a comment…  ⌘B bold · ⌘K link" />
                    <div style={{ display: "flex", justifyContent: "flex-end" }}>
                      <button className="btn primary" disabled={!comment.trim()} onClick={post}><Icon name="chat" size="sm" />Comment</button>
                    </div>
                  </div>
                </div>
              </div>
            )}
          </div>

          {/* side details */}
          <div style={{ display: "flex", flexDirection: "column", gap: 18 }}>
            <DRow label="Status">
              <StatusSeg status={ticket.status} onMove={(s) => onMove(ticket.id, s)} />
            </DRow>
            <DRow label="Project">
              {editing
                ? <select className="osel" value={projectId} onChange={(e) => setProjectId(e.target.value)}>
                    <option value="">No project</option>
                    {projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
                  </select>
                : (proj
                    ? <span style={{ display: "inline-flex", alignItems: "center", gap: 7, fontSize: 12, color: proj.color }}>
                        <span style={{ width: 19, height: 19, borderRadius: 5, display: "grid", placeItems: "center", flex: "none",
                          background: "color-mix(in oklch, " + proj.color + ", transparent 82%)", border: "1px solid color-mix(in oklch, " + proj.color + ", transparent 62%)" }}>
                          <Icon name={proj.icon} size="sm" style={{ width: 12, height: 12, color: proj.color }} /></span>{proj.name}</span>
                    : <span style={{ fontSize: 12, color: "var(--ink-4)" }}>No project</span>)}
            </DRow>

            <DRow label="Agent">
              {agent ? (
                <div onClick={() => onOpenAgent(agent.id)} style={{ display: "flex", alignItems: "center", gap: 9, padding: "9px 10px", borderRadius: "var(--r-md)",
                  background: "var(--panel)", border: "1px solid var(--hair)", cursor: "pointer" }}
                  onMouseEnter={(e) => e.currentTarget.style.borderColor = "var(--hair-2)"} onMouseLeave={(e) => e.currentTarget.style.borderColor = "var(--hair)"}>
                  <ToolBadge tool={agent.tool} size={20} />
                  <div style={{ minWidth: 0, flex: 1 }}>
                    <div className="disp" style={{ fontSize: 12, fontWeight: 600, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{agent.name}</div>
                    <div style={{ display: "flex", alignItems: "center", gap: 5, fontSize: 10, color: (STATUS_META[agent.status] || {}).color, marginTop: 2 }}>
                      <StatusDot status={agent.status} />{(STATUS_META[agent.status] || {}).label}</div>
                  </div>
                  <Icon name="enter" size="sm" style={{ color: "var(--ink-4)", flex: "none" }} />
                </div>
              ) : ticket.status === "todo" ? (
                <button className="btn ghost-hair" onClick={() => onDispatch(ticket)} style={{ justifyContent: "center", color: "var(--accent)", borderColor: "color-mix(in oklch, var(--accent), transparent 55%)" }}>
                  <Icon name="bolt" size="sm" />Dispatch an agent</button>
              ) : <span style={{ fontSize: 12, color: "var(--ink-4)" }}>None attached</span>}
            </DRow>

            {agent && (
              <DRow label="Branch">
                <span style={{ display: "inline-flex", alignItems: "center", gap: 6, fontSize: 11.5, color: "var(--ink-2)" }}>
                  <Icon name="branch" size="sm" style={{ width: 12, height: 12, color: "var(--accent-2)" }} />{agent.branch}</span>
              </DRow>
            )}

            <DRow label="Created">
              <span style={{ fontSize: 11.5, color: "var(--ink-2)" }} className="tnum">{draft ? "just now" : (ticket.created || "—")}</span>
            </DRow>
          </div>
        </div>
      </div>
    </div>
  );
}

function plainTextNonEmpty(html) { if (!html) return false; const d = document.createElement("div"); d.innerHTML = html; return (d.textContent || "").trim().length > 0; }

Object.assign(window, { TicketPage });
