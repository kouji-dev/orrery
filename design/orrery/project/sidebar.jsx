/* global React, Icon, StatusDot, ToolBadge, fmtDur */
// ORCHESTRA left sidebar — projects → agents

const { useState: useStateS, useRef: useRefS } = React;

const STATUS_PRIORITY = { blocked: 0, running: 1, waiting: 2, queued: 3, done: 4, idle: 5 };

function AgentRow({ ag, active, onOpen, onContext }) {
  const totAdd = ag.files.reduce((s, f) => s + f.add, 0);
  const totDel = ag.files.reduce((s, f) => s + f.del, 0);
  const needs = ag.status === "blocked" || (ag.pending && ag.pending.some((p) => p.kind === "permission" || p.kind === "decision"));
  return (
    <div onClick={() => onOpen(ag.id)} onContextMenu={(e) => onContext(e, ag.id)}
      draggable
      onDragStart={(e) => { window.__omDrag = { kind: "agent", id: ag.id, agentId: ag.id }; e.dataTransfer.effectAllowed = "copy"; try { e.dataTransfer.setData("text/plain", ag.name); } catch (_) {} }}
      onDragEnd={() => { window.__omDrag = null; }}
      title="drag onto a tab, or into a split, to add its terminal"
      style={{
        display: "flex", flexDirection: "column", gap: 3,
        padding: "6px 10px 7px", cursor: "pointer", position: "relative",
        borderRadius: "var(--r-md)", margin: "1px 8px 1px 14px",
        background: active ? "var(--panel-3)" : "transparent",
        border: "1px solid " + (active ? "var(--hair-2)" : "transparent"),
      }}
      onMouseEnter={(e) => { if (!active) e.currentTarget.style.background = "var(--panel-2)"; }}
      onMouseLeave={(e) => { if (!active) e.currentTarget.style.background = "transparent"; }}>
      {active && <span style={{ position: "absolute", left: -8, top: 7, bottom: 7, width: 2.5, borderRadius: 3, background: "linear-gradient(var(--accent), var(--accent-2))" }} />}
      <div style={{ display: "flex", alignItems: "center", gap: 7 }}>
        <StatusDot status={ag.status} />
        <span style={{ fontSize: "var(--fs-tree)", color: "var(--ink)", fontWeight: 500, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{ag.name}</span>
        {needs && <span style={{ width: 5, height: 5, borderRadius: "50%", background: "var(--st-blocked)", flex: "none" }} />}
        <ToolBadge tool={ag.tool} size={14} />
        <span style={{ marginLeft: "auto", fontSize: 9.5, color: "var(--ink-4)" }} className="tnum">{ag.elapsed ? fmtDur(ag.elapsed) : "—"}</span>
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 6, paddingLeft: 15 }}>
        <Icon name="branch" size="sm" style={{ color: "var(--ink-4)", width: 11, height: 11, flex: "none" }} />
        <span style={{ fontSize: 10.5, color: "var(--ink-3)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{ag.branch.replace("agent/", "")}</span>
        {ag.files.length > 0 && (
          <span className="tnum" style={{ marginLeft: "auto", fontSize: 10, display: "flex", gap: 5, flex: "none" }}>
            <span style={{ color: "var(--code-add-ink)" }}>+{totAdd}</span>
            <span style={{ color: "var(--code-del-ink)" }}>−{totDel}</span>
          </span>
        )}
      </div>
      {ag.status === "running" && <div className="activity" style={{ marginLeft: 15, marginTop: 2 }} />}
    </div>
  );
}

function ProjectGroup({ project, agents, activeAgent, onOpen, onContext, onSpawn, onProjectContext, collapsed, toggle, onScope }) {
  const sorted = [...agents].sort((a, b) => STATUS_PRIORITY[a.status] - STATUS_PRIORITY[b.status]);
  const running = agents.filter((a) => a.status === "running").length;
  const needs = agents.filter((a) => a.status === "blocked").length;
  return (
    <div style={{ marginBottom: 2 }}>
      <div onClick={() => toggle(project.id)} onContextMenu={(e) => onProjectContext(e, project.id)}
        className="proj-row"
        style={{ display: "flex", alignItems: "center", gap: 8, padding: "7px 10px", cursor: "pointer", position: "relative", margin: "0 6px", borderRadius: "var(--r-md)" }}>
        <Icon name={collapsed ? "chevron" : "chevronD"} size="sm" style={{ width: 11, height: 11, color: "var(--ink-4)", flex: "none" }} />
        <span style={{ width: 19, height: 19, flex: "none", borderRadius: 5, display: "grid", placeItems: "center",
          background: "color-mix(in oklch, " + project.color + ", transparent 82%)",
          border: "1px solid color-mix(in oklch, " + project.color + ", transparent 62%)" }}>
          <Icon name={project.icon} size="sm" style={{ width: 12, height: 12, color: project.color }} />
        </span>
        <span style={{ fontSize: 12.5, fontWeight: 600, color: "var(--ink)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{project.name}</span>
        {needs > 0 && <span className="tnum" style={{ fontSize: 9, fontWeight: 700, color: "var(--st-blocked)" }}>{needs}!</span>}
        <span className="tnum" style={{ marginLeft: "auto", fontSize: 9.5, color: "var(--ink-4)", flex: "none" }}>{agents.length}</span>
        <button className="proj-spawn" title="Spawn agent in this project"
          onClick={(e) => { e.stopPropagation(); onSpawn(project.id); }}
          style={{ background: "transparent", border: "none", color: "var(--ink-3)", cursor: "pointer", display: "flex", padding: 2, borderRadius: 4, flex: "none" }}
          onMouseEnter={(e) => { e.currentTarget.style.color = "var(--accent)"; e.currentTarget.style.background = "var(--panel-3)"; }}
          onMouseLeave={(e) => { e.currentTarget.style.color = "var(--ink-3)"; e.currentTarget.style.background = "transparent"; }}>
          <Icon name="bolt" size="sm" />
        </button>
      </div>
      {!collapsed && (
        <div style={{ position: "relative" }}>
          <span style={{ position: "absolute", left: 21, top: 0, bottom: 4, width: 1, background: "var(--hair)" }} />
          {sorted.length ? sorted.map((ag) => (
            <AgentRow key={ag.id} ag={ag} active={ag.id === activeAgent} onOpen={onOpen} onContext={onContext} />
          )) : (
            <div style={{ padding: "4px 10px 6px 30px", fontSize: 10.5, color: "var(--ink-4)" }}>no agents — spawn one</div>
          )}
        </div>
      )}
    </div>
  );
}

function NavRow({ icon, label, active, onClick, badge }) {
  return (
    <div onClick={onClick} style={{ display: "flex", alignItems: "center", gap: 9, padding: "7px 9px", borderRadius: "var(--r-md)", cursor: "pointer", position: "relative",
      background: active ? "var(--panel-3)" : "transparent", border: "1px solid " + (active ? "var(--hair-2)" : "transparent") }}
      onMouseEnter={(e) => { if (!active) e.currentTarget.style.background = "var(--panel-2)"; }}
      onMouseLeave={(e) => { if (!active) e.currentTarget.style.background = "transparent"; }}>
      {active && <span style={{ position: "absolute", left: -8, top: 7, bottom: 7, width: 2.5, borderRadius: 3, background: "linear-gradient(var(--accent), var(--accent-2))" }} />}
      <Icon name={icon} size="sm" style={{ color: active ? "var(--accent)" : "var(--ink-3)" }} />
      <span style={{ fontSize: 12.5, fontWeight: 600, color: active ? "var(--ink)" : "var(--ink-2)" }}>{label}</span>
      {badge > 0 && <span className="chip tnum" style={{ marginLeft: "auto", fontSize: 9, padding: "0 6px" }}>{badge}</span>}
    </div>
  );
}

function Sidebar({ projects, agents, activeAgent, onOpen, onContext, onProjectContext, onSpawn, onAddProject, query, setQuery, onToggleCompact, loading, onReload, view, onNav, openTicketCount }) {
  const [collapsed, setCollapsed] = useStateS({});
  const toggle = (id) => setCollapsed((c) => ({ ...c, [id]: !c[id] }));

  const q = query.toLowerCase();
  const matches = (a) => !q || a.name.toLowerCase().includes(q) || a.task.toLowerCase().includes(q);

  const totalRunning = agents.filter((a) => a.status === "running").length;

  return (
    <aside style={{ display: "flex", flexDirection: "column", minHeight: 0, background: "var(--panel)", borderRight: "1px solid var(--hair)" }}>
      {onNav && (
        <div style={{ padding: 8, borderBottom: "1px solid var(--hair)", display: "flex", flexDirection: "column", gap: 2 }}>
          <NavRow icon="layers" label="Orchestrator" active={view === "orchestrator"} onClick={() => onNav("orchestrator")} />
          <NavRow icon="columns" label="Backlog" active={view === "backlog"} onClick={() => onNav("backlog")} badge={openTicketCount} />
        </div>
      )}
      <div style={{ padding: "10px 12px 8px", borderBottom: "1px solid var(--hair)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 7, marginBottom: 8 }}>
          <Icon name="layers" size="sm" style={{ color: "var(--accent)" }} />
          <span className="up" style={{ fontSize: 9.5, color: "var(--ink-3)" }}>Projects</span>
          {loading
            ? <span className="skel" style={{ width: 16, height: 13, borderRadius: 999 }} />
            : <span className="chip tnum" style={{ fontSize: 9, padding: "0 6px" }}>{projects.length}</span>}
          <span className="chip tnum" style={{ marginLeft: "auto", fontSize: 9, padding: "1px 6px" }}>
            <span className="dot running" style={{ background: "var(--st-running)", width: 6, height: 6 }} />{totalRunning}/5
          </span>
          {onReload && (
            <button className="pane-btn" onClick={() => !loading && onReload()} title="Refresh projects" disabled={loading}>
              <Icon name="refresh" size="sm" style={{ width: 13, height: 13, animation: loading ? "spin 0.8s linear infinite" : "none" }} />
            </button>
          )}
          <button className="pane-btn" onClick={onToggleCompact} title="Collapse sidebar">
            <Icon name="panelLeft" size="sm" style={{ width: 14, height: 14 }} />
          </button>
        </div>
        <div style={{ display: "flex", alignItems: "center", gap: 7, padding: "5px 8px", background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-sm)" }}>
          <Icon name="search" size="sm" style={{ color: "var(--ink-4)" }} />
          <input value={query} onChange={(e) => setQuery(e.target.value)} placeholder="filter agents…"
            style={{ flex: 1, minWidth: 0, background: "transparent", border: "none", outline: "none", color: "var(--ink)", fontFamily: "var(--font-mono)", fontSize: 11.5 }} />
          {query && <Icon name="x" size="sm" style={{ color: "var(--ink-4)", cursor: "pointer" }} onClick={() => setQuery("")} />}
        </div>
      </div>

      <div className="scroll-y" style={{ flex: 1, padding: "6px 0" }}>
        {loading ? <SidebarSkeleton groups={3} /> : projects.map((p) => {
          const pa = agents.filter((a) => a.projectId === p.id && matches(a));
          if (q && !pa.length) return null;
          return (
            <ProjectGroup key={p.id} project={p} agents={pa} activeAgent={activeAgent}
              onOpen={onOpen} onContext={onContext} onProjectContext={onProjectContext}
              onSpawn={onSpawn} collapsed={!!collapsed[p.id]} toggle={toggle} />
          );
        })}
      </div>

      <div style={{ padding: 10, borderTop: "1px solid var(--hair)", display: "flex", gap: 8 }}>
        <button className="btn ghost-hair" onClick={onAddProject} style={{ flex: 1, justifyContent: "center" }}>
          <Icon name="folder" size="sm" />Add project
        </button>
        <button className="btn primary" onClick={() => onSpawn(null)} title="Spawn agent" style={{ padding: "5px 11px" }}>
          <Icon name="bolt" size="sm" />Spawn
        </button>
      </div>
    </aside>
  );
}

// ---------- compact rail ----------
function RailPopover({ project, agents, top, onOpen, onContext, onSpawn, onKeep, onLeave }) {
  const vh = window.innerHeight;
  const t = Math.min(top, vh - Math.min(60 + agents.length * 32, 320) - 40);
  return (
    <div className="rise" onMouseOver={onKeep} onMouseLeave={onLeave}
      style={{ position: "fixed", left: 52, top: Math.max(48, t), zIndex: 60, width: 236,
        background: "var(--elev)", border: "1px solid var(--hair-2)", borderRadius: "var(--r-md)", boxShadow: "var(--shadow)", overflow: "hidden" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "9px 11px", borderBottom: "1px solid var(--hair)" }}>
        <span style={{ width: 17, height: 17, flex: "none", borderRadius: 5, display: "grid", placeItems: "center",
          background: "color-mix(in oklch, " + project.color + ", transparent 82%)", border: "1px solid color-mix(in oklch, " + project.color + ", transparent 62%)" }}>
          <Icon name={project.icon} size="sm" style={{ width: 11, height: 11, color: project.color }} />
        </span>
        <span style={{ fontSize: 12, fontWeight: 600 }}>{project.name}</span>
        <span className="tnum" style={{ marginLeft: "auto", fontSize: 9.5, color: "var(--ink-4)" }}>{agents.length}</span>
        <button className="pane-btn" onClick={() => onSpawn(project.id)} title="Spawn agent"><Icon name="bolt" size="sm" style={{ width: 13, height: 13 }} /></button>
      </div>
      <div style={{ padding: 5, maxHeight: 280, overflowY: "auto" }}>
        {agents.length ? agents.map((ag) => (
          <div key={ag.id} onClick={() => onOpen(ag.id)} onContextMenu={(e) => onContext(e, ag.id)}
            draggable
            onDragStart={(e) => { window.__omDrag = { kind: "agent", id: ag.id, agentId: ag.id }; e.dataTransfer.effectAllowed = "copy"; try { e.dataTransfer.setData("text/plain", ag.name); } catch (_) {} }}
            onDragEnd={() => { window.__omDrag = null; }}
            style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 8px", borderRadius: 6, cursor: "pointer" }}
            onMouseEnter={(e) => e.currentTarget.style.background = "var(--panel-2)"}
            onMouseLeave={(e) => e.currentTarget.style.background = "transparent"}>
            <StatusDot status={ag.status} />
            <span style={{ flex: 1, fontSize: 11.5, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{ag.name}</span>
            {(ag.status === "blocked" || (ag.pending && ag.pending.length)) && <span style={{ width: 5, height: 5, borderRadius: "50%", background: "var(--st-blocked)" }} />}
            <ToolBadge tool={ag.tool} size={13} />
          </div>
        )) : <div style={{ padding: "8px 10px", fontSize: 10.5, color: "var(--ink-4)" }}>no agents — spawn one</div>}
      </div>
    </div>
  );
}

function CompactRail({ projects, agents, activeAgent, onOpen, onContext, onProjectContext, onSpawn, onAddProject, onExpand, loading, view, onNav, openTicketCount }) {
  const [hover, setHover] = useStateS(null); // { id, top }
  const timer = useRefS(null);
  const enter = (id, el) => { clearTimeout(timer.current); setHover({ id, top: el.getBoundingClientRect().top }); };
  const leave = () => { timer.current = setTimeout(() => setHover(null), 130); };
  const keep = () => clearTimeout(timer.current);
  const activeProj = activeAgent ? (agents.find((a) => a.id === activeAgent) || {}).projectId : null;
  const hoverProj = hover ? projects.find((p) => p.id === hover.id) : null;
  return (
    <aside style={{ display: "flex", flexDirection: "column", alignItems: "center", minHeight: 0, width: 54, background: "var(--panel)", borderRight: "1px solid var(--hair)", padding: "8px 0", gap: 4, position: "relative" }}>
      <button className="rail-btn" onClick={onExpand} title="Expand sidebar" style={{ marginBottom: 2 }}>
        <Icon name="panelLeft" size="sm" style={{ color: "var(--accent)" }} />
      </button>
      {onNav && (
        <React.Fragment>
          <button className="rail-btn" onClick={() => onNav("orchestrator")} title="Orchestrator"
            style={{ background: view === "orchestrator" ? "color-mix(in oklch, var(--accent), transparent 86%)" : "transparent", border: "1px solid " + (view === "orchestrator" ? "color-mix(in oklch, var(--accent), transparent 55%)" : "transparent") }}>
            <Icon name="grid" size="sm" style={{ color: view === "orchestrator" ? "var(--accent)" : "var(--ink-3)" }} />
          </button>
          <div className="rail-item">
            <button className="rail-btn" onClick={() => onNav("backlog")} title="Backlog"
              style={{ background: view === "backlog" ? "color-mix(in oklch, var(--accent), transparent 86%)" : "transparent", border: "1px solid " + (view === "backlog" ? "color-mix(in oklch, var(--accent), transparent 55%)" : "transparent") }}>
              <Icon name="columns" size="sm" style={{ color: view === "backlog" ? "var(--accent)" : "var(--ink-3)" }} />
              {openTicketCount > 0 && <span style={{ position: "absolute", top: 2, right: 2, minWidth: 13, height: 13, padding: "0 3px", borderRadius: 7, background: "var(--accent)", color: "#06070b", fontSize: 8, fontWeight: 700, display: "grid", placeItems: "center", border: "2px solid var(--panel)" }} className="tnum">{openTicketCount}</span>}
            </button>
          </div>
        </React.Fragment>
      )}
      <div style={{ width: 24, height: 1, background: "var(--hair)", margin: "2px 0 4px" }} />
      <div className="scroll-y" style={{ flex: 1, width: "100%", display: "flex", flexDirection: "column", alignItems: "center", gap: 5 }}>
        {loading
          ? Array.from({ length: 4 }).map((_, i) => <span key={i} className="skel" style={{ width: 30, height: 30, borderRadius: 8 }} />)
          : projects.map((p) => {
          const pa = agents.filter((a) => a.projectId === p.id);
          const running = pa.filter((a) => a.status === "running").length;
          const needs = pa.filter((a) => a.status === "blocked" || (a.pending && a.pending.length)).length;
          const isActive = activeProj === p.id;
          return (
            <div key={p.id} className="rail-item" onMouseOver={(e) => enter(p.id, e.currentTarget)} onMouseLeave={leave}>
              <button className="rail-btn" onClick={() => pa[0] && onOpen(pa[0].id)} onContextMenu={(e) => onProjectContext(e, p.id)} onFocus={(e) => enter(p.id, e.currentTarget.parentNode)}
                style={{ borderColor: isActive ? "color-mix(in oklch, " + p.color + ", transparent 50%)" : "transparent",
                  background: isActive ? "color-mix(in oklch, " + p.color + ", transparent 86%)" : ((hover && hover.id === p.id) ? "var(--panel-2)" : "transparent") }}>
                <span style={{ width: 22, height: 22, borderRadius: 6, display: "grid", placeItems: "center",
                  background: "color-mix(in oklch, " + p.color + ", transparent 84%)", border: "1px solid color-mix(in oklch, " + p.color + ", transparent 60%)" }}>
                  <Icon name={p.icon} size="sm" style={{ width: 13, height: 13, color: p.color }} />
                </span>
                {running > 0 && <span className="dot running" style={{ position: "absolute", top: 3, right: 3, width: 7, height: 7, background: "var(--st-running)" }} />}
                {needs > 0 && <span style={{ position: "absolute", top: 2, right: 2, minWidth: 13, height: 13, padding: "0 3px", borderRadius: 7, background: "var(--st-blocked)", color: "#fff", fontSize: 8, fontWeight: 700, display: "grid", placeItems: "center", border: "2px solid var(--panel)" }} className="tnum">{needs}</span>}
              </button>
            </div>
          );
        })}
      </div>
      <div style={{ width: 24, height: 1, background: "var(--hair)", margin: "4px 0" }} />
      <button className="rail-btn" onClick={onAddProject} title="Add project"><Icon name="folder" size="sm" style={{ color: "var(--ink-3)" }} /></button>
      <button className="rail-btn" onClick={() => onSpawn(null)} title="Spawn agent"
        style={{ background: "color-mix(in oklch, var(--accent), transparent 84%)", border: "1px solid color-mix(in oklch, var(--accent), transparent 60%)" }}>
        <Icon name="bolt" size="sm" style={{ color: "var(--accent)" }} />
      </button>
      {hoverProj && <RailPopover project={hoverProj} agents={agents.filter((a) => a.projectId === hoverProj.id)} top={hover.top}
        onOpen={onOpen} onContext={onContext} onSpawn={onSpawn} onKeep={keep} onLeave={leave} />}
    </aside>
  );
}

Object.assign(window, { Sidebar, CompactRail });
