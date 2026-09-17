/* global React, Icon, StatusDot, ToolBadge, STATUS_META, projectOf, ORG */
// ── Backlog — integrated Orchestrator view (board + cards) ──────────────────
const { useState: useStateBL } = React;

const TICKET_COL = {
  todo:       { key: "todo",       label: "To do",       color: "var(--ink-3)" },
  inprogress: { key: "inprogress", label: "In progress", color: "var(--accent)" },
  done:       { key: "done",       label: "Done",        color: "var(--st-done)" },
};
const TICKET_ORDER = ["todo", "inprogress", "done"];

const _plainCache = {};
function plainText(html) {
  if (html == null) return "";
  if (_plainCache[html]) return _plainCache[html];
  const d = document.createElement("div"); d.innerHTML = html;
  const t = (d.textContent || "").replace(/\s+/g, " ").trim();
  _plainCache[html] = t; return t;
}

// ── ticket project chip ──────────────────────────────────────────────────────
function TicketChip({ projectId, muted }) {
  const p = projectOf(projectId);
  if (!p) return (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 5, fontSize: 11, color: "var(--ink-4)" }}>
      <Icon name="folder" size="sm" style={{ width: 11, height: 11, flex: "none" }} />no project
    </span>
  );
  return (
    <span style={{ display: "inline-flex", alignItems: "center", gap: 5, fontSize: 11, minWidth: 0, overflow: "hidden", color: muted ? "var(--ink-3)" : p.color }}>
      <Icon name={p.icon} size="sm" style={{ width: 12, height: 12, flex: "none" }} />
      <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{p.name}</span>
    </span>
  );
}

// ── attached live-agent strip (resolves the real agent's status) ─────────────
function AgentStrip({ agent, onOpen }) {
  const m = STATUS_META[agent.status] || STATUS_META.idle;
  const [hover, setHover] = useStateBL(false);
  return (
    <div onClick={(e) => { e.stopPropagation(); onOpen(agent.id); }}
      onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}
      title="Open agent workspace"
      style={{ display: "flex", flexDirection: "column", gap: 7, padding: "8px 9px", cursor: "pointer", borderRadius: "var(--r-md)",
        background: "var(--panel-2)", border: "1px solid " + (hover ? "color-mix(in oklch, " + m.color + ", transparent 55%)" : "var(--hair)"),
        boxShadow: hover ? "0 0 0 1px color-mix(in oklch, " + m.color + ", transparent 72%)" : "none", transition: "border-color .15s, box-shadow .15s" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 7, minWidth: 0 }}>
        <ToolBadge tool={agent.tool} size={17} />
        <span className="disp" style={{ fontSize: 12, fontWeight: 600, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", flex: "0 1 auto" }}>{agent.name}</span>
        <span style={{ display: "inline-flex", alignItems: "center", gap: 5, flex: "none", fontSize: 9.5, padding: "2px 7px 2px 6px", borderRadius: 999,
          letterSpacing: ".08em", textTransform: "uppercase", color: m.color,
          border: "1px solid color-mix(in oklch, " + m.color + ", transparent 62%)", background: "color-mix(in oklch, " + m.color + ", transparent 88%)" }}>
          <StatusDot status={agent.status} />{m.label}
        </span>
        <Icon name="enter" size="sm" style={{ marginLeft: "auto", flex: "none", color: hover ? m.color : "var(--ink-4)", transition: "color .15s" }} />
      </div>
      <div style={{ height: 3, borderRadius: 3, background: "var(--hair)", overflow: "hidden", position: "relative" }}>
        <div style={{ height: "100%", width: Math.max((agent.progress || 0) * 100, 4) + "%", borderRadius: 3,
          background: m.color, opacity: agent.status === "blocked" ? 0.5 : 1, transition: "width .6s ease" }} />
        {agent.status === "running" && <div className="activity" style={{ position: "absolute", inset: 0, background: "transparent" }} />}
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 6, fontSize: 10, color: "var(--ink-3)" }} className="tnum">
        {agent.status === "blocked" && agent.blockReason
          ? <span style={{ display: "flex", alignItems: "center", gap: 5, color: "var(--code-del-ink)", minWidth: 0 }}>
              <Icon name="flag" size="sm" style={{ width: 11, height: 11, flex: "none" }} />
              <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{agent.blockReason}</span></span>
          : <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{agent.branch.replace("agent/", "")}</span>}
        <span style={{ marginLeft: "auto", display: "flex", gap: 4, flex: "none", color: "var(--ink-4)" }}>
          <Icon name="commit" size="sm" style={{ width: 11, height: 11 }} />{agent.commits}</span>
      </div>
    </div>
  );
}

// ── ticket card ──────────────────────────────────────────────────────────────
function TicketCard({ ticket, agents, onOpen, onDispatch, onOpenAgent, onDragStart, onDragEnd, dragging }) {
  const [hover, setHover] = useStateBL(false);
  const st = ticket.status;
  const agent = ticket.agentId ? agents.find((a) => a.id === ticket.agentId) : null;

  const wrap = {
    background: "var(--panel)", border: "1px solid var(--hair)", borderRadius: "var(--r-lg)", padding: 13,
    display: "flex", flexDirection: "column", gap: 9, position: "relative", cursor: "pointer",
    transition: "border-color .15s, transform .15s, box-shadow .15s",
    ...(st === "inprogress" ? { borderLeft: "2px solid color-mix(in oklch, var(--accent), transparent 35%)" } : {}),
    ...(st === "done" ? { background: "var(--panel-2)" } : {}),
    ...(hover ? { borderColor: "var(--hair-2)", transform: "translateY(-2px)", boxShadow: "var(--shadow)" } : {}),
    ...(dragging ? { opacity: 0.4 } : {}),
  };

  return (
    <div draggable onDragStart={(e) => onDragStart(e, ticket)} onDragEnd={onDragEnd}
      onClick={() => onOpen(ticket.id)} onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)} style={wrap}>
      <div style={{ display: "flex", alignItems: "flex-start", gap: 8 }}>
        {st === "done" && (
          <span style={{ flex: "none", width: 16, height: 16, marginTop: 1, borderRadius: "50%", display: "grid", placeItems: "center",
            background: "color-mix(in oklch, var(--st-done), transparent 82%)", border: "1px solid color-mix(in oklch, var(--st-done), transparent 55%)" }}>
            <Icon name="check" size="sm" style={{ width: 11, height: 11, color: "var(--st-done)" }} />
          </span>
        )}
        <span className="disp" style={{ fontSize: 13.5, fontWeight: 600, lineHeight: 1.3, flex: 1, minWidth: 0,
          display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical", overflow: "hidden",
          color: st === "done" ? "var(--ink-2)" : "var(--ink)" }}>{ticket.title}</span>
        <span style={{ flex: "none", fontSize: 9.5, color: "var(--ink-4)", marginTop: 2 }} className="tnum">#{ticket.id.replace(/^t/, "")}</span>
      </div>

      {st !== "done" && <p style={{ fontSize: 12, color: "var(--ink-2)", lineHeight: 1.5,
        display: "-webkit-box", WebkitLineClamp: 2, WebkitBoxOrient: "vertical", overflow: "hidden" }}>{plainText(ticket.notes)}</p>}

      {st === "inprogress" && (agent
        ? <AgentStrip agent={agent} onOpen={onOpenAgent} />
        : <div onClick={(e) => { e.stopPropagation(); onDispatch(ticket); }}
            style={{ display: "flex", alignItems: "center", gap: 7, padding: "7px 9px", borderRadius: "var(--r-md)", cursor: "pointer",
              border: "1px dashed color-mix(in oklch, var(--accent), transparent 55%)", color: "var(--accent)", fontSize: 11 }}>
            <Icon name="bolt" size="sm" />No agent yet — dispatch one</div>)}

      {st === "todo" && (
        <div style={{ display: "flex", alignItems: "center", gap: 8 }}>
          <TicketChip projectId={ticket.projectId} />
          <button className={"btn " + (hover ? "primary" : "ghost-hair")} onClick={(e) => { e.stopPropagation(); onDispatch(ticket); }}
            style={{ marginLeft: "auto", flex: "none", padding: "5px 11px", fontSize: 11.5,
              ...(hover ? {} : { color: "var(--accent)", borderColor: "color-mix(in oklch, var(--accent), transparent 55%)" }) }}>
            <Icon name="bolt" size="sm" />Dispatch</button>
        </div>
      )}
      {st === "inprogress" && <div style={{ display: "flex", alignItems: "center" }}><TicketChip projectId={ticket.projectId} /></div>}
      {st === "done" && (
        <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 11, color: "var(--ink-3)" }} className="tnum">
          <TicketChip projectId={ticket.projectId} muted />
          {agent && <span style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 6, flex: "none" }}>
            <ToolBadge tool={agent.tool} size={14} />
            <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", maxWidth: 92 }}>{agent.name}</span>
            <span style={{ color: "var(--ink-4)" }}>·</span>
            <span style={{ display: "flex", alignItems: "center", gap: 3, color: "var(--st-done)" }}>
              <Icon name="commit" size="sm" style={{ width: 11, height: 11 }} />{agent.commits}</span>
          </span>}
        </div>
      )}
    </div>
  );
}

// ── board column (drop target) ───────────────────────────────────────────────
function Column({ status, tickets, agents, onOpen, onDispatch, onOpenAgent, onMove, drag, setDrag, onNew }) {
  const m = TICKET_COL[status];
  const [over, setOver] = useStateBL(false);
  const isTarget = drag && drag.from !== status;

  return (
    <div onDragOver={(e) => { if (drag) { e.preventDefault(); setOver(true); } }}
      onDragLeave={() => setOver(false)}
      onDrop={(e) => { e.preventDefault(); if (drag) onMove(drag.id, status); setOver(false); }}
      style={{ display: "flex", flexDirection: "column", gap: 9, minWidth: 0, borderRadius: "var(--r-lg)",
        outline: over && isTarget ? "1.5px dashed color-mix(in oklch, " + m.color + ", transparent 35%)" : "1.5px dashed transparent",
        outlineOffset: 4, background: over && isTarget ? "color-mix(in oklch, " + m.color + ", transparent 94%)" : "transparent", transition: "background .15s" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "0 2px 6px", borderBottom: "2px solid " + m.color, flex: "none" }}>
        <span className="up" style={{ fontSize: 10, color: "var(--ink-2)" }}>{m.label}</span>
        <span className="chip tnum" style={{ marginLeft: "auto", fontSize: 9.5, padding: "0 6px" }}>{tickets.length}</span>
      </div>
      {tickets.map((tk) => (
        <TicketCard key={tk.id} ticket={tk} agents={agents} onOpen={onOpen} onDispatch={onDispatch} onOpenAgent={onOpenAgent}
          dragging={drag && drag.id === tk.id}
          onDragStart={(e, t) => { setDrag({ id: t.id, from: t.status }); try { e.dataTransfer.effectAllowed = "move"; e.dataTransfer.setData("text/plain", t.id); } catch (_) {} }}
          onDragEnd={() => { setDrag(null); setOver(false); }} />
      ))}
      {status === "todo" && (
        <button className="btn" onClick={onNew} style={{ justifyContent: "center", gap: 6, padding: 9, borderRadius: "var(--r-md)",
          border: "1px dashed var(--hair-2)", color: "var(--ink-3)", fontSize: 11.5, background: "transparent" }}>
          <Icon name="plus" size="sm" />New ticket</button>
      )}
      {!tickets.length && status !== "todo" && (
        <div style={{ padding: 14, textAlign: "center", color: "var(--ink-4)", fontSize: 10.5, border: "1px dashed var(--hair)", borderRadius: "var(--r-md)" }}>empty</div>
      )}
    </div>
  );
}

// ── the Backlog view (center pane) ───────────────────────────────────────────
function BacklogView({ tickets, agents, projects, onOpen, onNew, onDispatch, onOpenAgent, onMove }) {
  const [filter, setFilter] = useStateBL("all");
  const [filterOpen, setFilterOpen] = useStateBL(false);
  const [drag, setDrag] = useStateBL(null);

  const shown = filter === "all" ? tickets : tickets.filter((t) => t.projectId === filter);
  const open = tickets.filter((t) => t.status !== "done").length;
  const filterName = filter === "all" ? "All projects" : (projectOf(filter) || {}).name;
  const isEmpty = tickets.length === 0;

  return (
    <div style={{ display: "flex", flexDirection: "column", minHeight: 0, background: "var(--panel-2)" }}>
      {/* header */}
      <div style={{ display: "flex", alignItems: "center", gap: 14, padding: "14px 18px", flex: "none", borderBottom: "1px solid var(--hair)", background: "var(--panel)" }}>
        <div>
          <h1 className="disp" style={{ fontSize: 16, fontWeight: 600, letterSpacing: "-0.02em" }}>Backlog</h1>
          <span style={{ fontSize: 10.5, color: "var(--ink-3)" }}>{tickets.length} tickets · {open} open · {ORG}</span>
        </div>
        <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 8 }}>
          <div style={{ position: "relative" }}>
            <button className="btn ghost-hair" style={{ fontSize: 11.5, color: "var(--ink-2)" }} onClick={() => setFilterOpen((o) => !o)}>
              <Icon name="folder" size="sm" style={{ color: "var(--ink-3)" }} />{filterName}
              <Icon name="chevronD" size="sm" style={{ color: "var(--ink-4)" }} />
            </button>
            {filterOpen && (
              <div className="surface" style={{ position: "absolute", top: "100%", right: 0, marginTop: 6, zIndex: 20, padding: 5, minWidth: 190, boxShadow: "var(--shadow)" }}>
                {[{ id: "all", name: "All projects" }, ...projects].map((p) => (
                  <div key={p.id} onClick={() => { setFilter(p.id); setFilterOpen(false); }}
                    style={{ display: "flex", alignItems: "center", gap: 8, padding: "7px 9px", borderRadius: "var(--r-sm)", cursor: "pointer", fontSize: 12,
                      background: filter === p.id ? "var(--panel-3)" : "transparent", color: filter === p.id ? "var(--ink)" : "var(--ink-2)" }}
                    onMouseEnter={(e) => { if (filter !== p.id) e.currentTarget.style.background = "var(--panel-2)"; }}
                    onMouseLeave={(e) => { if (filter !== p.id) e.currentTarget.style.background = "transparent"; }}>
                    {p.id === "all" ? <Icon name="layers" size="sm" style={{ color: "var(--ink-3)" }} /> : <Icon name={p.icon} size="sm" style={{ color: p.color }} />}
                    {p.name}
                  </div>
                ))}
              </div>
            )}
          </div>
          <button className="btn primary" onClick={onNew}><Icon name="plus" size="sm" />New ticket</button>
        </div>
      </div>

      {/* body */}
      <div className="scroll-y" style={{ flex: 1 }}>
        {isEmpty ? (
          <div style={{ display: "flex", flexDirection: "column", alignItems: "center", justifyContent: "center", gap: 14, padding: "80px 20px", textAlign: "center" }}>
            <div style={{ width: 52, height: 52, borderRadius: 14, display: "grid", placeItems: "center",
              background: "color-mix(in oklch, var(--accent), transparent 88%)", border: "1px solid color-mix(in oklch, var(--accent), transparent 60%)" }}>
              <Icon name="layers" size="lg" style={{ color: "var(--accent)" }} />
            </div>
            <div>
              <div className="disp" style={{ fontSize: 16, fontWeight: 600 }}>No tickets yet</div>
              <div style={{ fontSize: 12, color: "var(--ink-3)", marginTop: 5, maxWidth: 340, lineHeight: 1.5 }}>
                Capture work to be done, then dispatch an agent straight from a ticket — the board tracks it to&nbsp;done.</div>
            </div>
            <button className="btn primary" onClick={onNew}><Icon name="plus" size="sm" />New ticket</button>
          </div>
        ) : (
          <div style={{ display: "grid", gridTemplateColumns: "repeat(3, 1fr)", gap: 14, padding: 18, alignContent: "start" }}>
            {TICKET_ORDER.map((s) => (
              <Column key={s} status={s} tickets={shown.filter((t) => t.status === s)} agents={agents}
                onOpen={onOpen} onDispatch={onDispatch} onOpenAgent={onOpenAgent} onMove={onMove}
                drag={drag} setDrag={setDrag} onNew={onNew} />
            ))}
          </div>
        )}
      </div>
    </div>
  );
}

Object.assign(window, { BacklogView, TicketCard, TICKET_COL, TICKET_ORDER, plainText, TicketChip });
