/* global React, Icon */
// Orrery — bottom tool window (IntelliJ-style dock). Panels register themselves
// into window.TOOL_PANELS; the shell owns the tab strip, resize and close.

const { useState: useStateTW, useRef: useRefTW, useEffect: useEffectTW } = React;

window.TOOL_PANELS = window.TOOL_PANELS || {};
// order of the tab strip
window.TOOL_ORDER = ["search", "graph", "branches", "history", "cost", "github"];

function ToolWindow({ tool, onSelect, onClose, ctx }) {
  const [h, setH] = useStateTW(() => Math.round(Math.min(420, Math.max(240, window.innerHeight * 0.36))));
  // a tab can hold agents from several projects, so the tool window carries its
  // own scope: pick the project (and optionally one worktree) the panels act on
  const projects = ctx.projects || [];
  const [scope, setScope] = useStateTW(() => ({ projectId: (ctx.scopeAgent && ctx.scopeAgent.projectId) || (projects[0] || {}).id, agentId: (ctx.scopeAgent || {}).id || null }));
  const project = projects.find((p) => p.id === scope.projectId) || projects[0] || null;
  const projAgents = (ctx.agents || []).filter((a) => project && a.projectId === project.id);
  const agent = projAgents.find((a) => a.id === scope.agentId) || projAgents[0] || null;
  const focus = ctx.scopeAgent;
  const outOfSync = !!(focus && (focus.projectId !== (project && project.id) || focus.id !== (agent && agent.id)) && projects.some((p) => p.id === focus.projectId));
  const scoped = Object.assign({}, ctx, { project, projectId: project && project.id, agents: projAgents, allAgents: ctx.agents, scopeAgent: agent });
  const drag = useRefTW(null);
  useEffectTW(() => {
    const move = (e) => { if (!drag.current) return; const dy = drag.current.y - e.clientY; setH(Math.max(160, Math.min(window.innerHeight - 200, drag.current.h + dy))); };
    const up = () => { drag.current = null; document.body.style.cursor = ""; };
    window.addEventListener("mousemove", move); window.addEventListener("mouseup", up);
    return () => { window.removeEventListener("mousemove", move); window.removeEventListener("mouseup", up); };
  }, []);
  const panels = window.TOOL_ORDER.map((k) => window.TOOL_PANELS[k]).filter(Boolean);
  const panel = window.TOOL_PANELS[tool.kind];
  const Body = panel && panel.Component;
  return (
    <div style={{ height: h, flex: "none", display: "flex", flexDirection: "column", minHeight: 0, background: "var(--panel)", borderTop: "1px solid var(--hair)" }}>
      <div onMouseDown={(e) => { drag.current = { y: e.clientY, h }; document.body.style.cursor = "row-resize"; }}
        style={{ height: 4, marginTop: -3, cursor: "row-resize", flex: "none" }} />
      <div style={{ display: "flex", alignItems: "stretch", gap: 2, padding: "0 8px", borderBottom: "1px solid var(--hair)", flex: "none", background: "var(--panel-2)" }}>
        {panels.map((p) => {
          const on = p.kind === tool.kind;
          return (
            <button key={p.kind} className="btn" onClick={() => onSelect({ kind: p.kind })}
              style={{ padding: "7px 10px", borderRadius: 0, position: "relative", fontSize: 11.5, color: on ? "var(--ink)" : "var(--ink-3)" }}>
              {on && <span style={{ position: "absolute", left: 8, right: 8, bottom: 0, height: 2, background: "linear-gradient(90deg, var(--accent), var(--accent-2))" }} />}
              <Icon name={p.icon} size="sm" style={{ color: on ? "var(--accent)" : "inherit" }} />{p.label}
            </button>
          );
        })}
        <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 6, paddingLeft: 10 }}>
          <span className="up" style={{ fontSize: 8.5, color: "var(--ink-4)", flex: "none" }}>scope</span>
          {project && (
            <span style={{ display: "flex", alignItems: "center", gap: 5, flex: "none" }}>
              <Icon name={project.icon} size="sm" style={{ width: 12, height: 12, color: project.color }} />
              <select className="osel" value={project.id} onChange={(e) => setScope({ projectId: e.target.value, agentId: null })}
                style={{ width: 152, padding: "3px 22px 3px 8px", fontSize: 10.5 }}>
                {projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
              </select>
            </span>
          )}
          <select className="osel" value={(agent && agent.id) || ""} onChange={(e) => setScope((s) => ({ ...s, agentId: e.target.value || null }))}
            title="Worktree the panel reads from" style={{ width: 150, padding: "3px 22px 3px 8px", fontSize: 10.5, flex: "none" }}>
            {!projAgents.length && <option value="">no worktrees</option>}
            {projAgents.map((a) => <option key={a.id} value={a.id}>{a.name}</option>)}
          </select>
          {outOfSync && (
            <button className="btn ghost-hair" title={"Follow the focused agent · " + focus.name} style={{ padding: "2px 7px", fontSize: 10, color: "var(--accent-2)", flex: "none" }}
              onClick={() => setScope({ projectId: focus.projectId, agentId: focus.id })}>
              <Icon name="link" size="sm" style={{ width: 11, height: 11 }} />{focus.name}
            </button>
          )}
          <button className="pane-btn" onClick={onClose} title="Hide tool window" style={{ alignSelf: "center" }}>
            <Icon name="x" size="sm" />
          </button>
        </div>
      </div>
      <div style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column", background: "var(--panel)" }}>
        {Body ? <Body key={(project && project.id) + ":" + ((agent && agent.id) || "none")} tool={tool} ctx={scoped} onClose={onClose} /> : <div style={{ padding: 18, fontSize: 11.5, color: "var(--ink-4)" }}>panel unavailable</div>}
      </div>
    </div>
  );
}

Object.assign(window, { ToolWindow });
