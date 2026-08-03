/* global React, Icon, StatusDot, StatusPill, STATUS_META, Ring, fmtDur, fileName, PROJECTS, ORG, LOGS, logColor, logPrefix */
// ORCHESTRA center — Orchestrator overview (hero) with 4 visualization metaphors

const { useState: useStateO } = React;

function StatBlock({ n, label, color, pulse }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 2, paddingRight: 20 }}>
      <div style={{ display: "flex", alignItems: "baseline", gap: 6 }}>
        <span className="disp tnum" style={{ fontSize: 24, fontWeight: 600, color: color || "var(--ink)", lineHeight: 1 }}>{n}</span>
        {pulse && <span className="dot running" style={{ background: color }} />}
      </div>
      <span className="up" style={{ fontSize: 9, color: "var(--ink-3)" }}>{label}</span>
    </div>
  );
}

function MiniTerm({ id }) {
  const lines = (LOGS[id] || []).slice(-3);
  return (
    <div style={{
      background: "var(--bg)", border: "1px solid var(--hair)", borderRadius: "var(--r-sm)",
      padding: "6px 8px", fontSize: 10, lineHeight: 1.6, overflow: "hidden",
    }}>
      {lines.length ? lines.map((l, i) => (
        <div key={i} style={{ whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", color: logColor(l.t) }}>
          <span style={{ color: "var(--ink-4)", marginRight: 5 }}>{logPrefix(l.t)}</span>{l.s}
        </div>
      )) : <span style={{ color: "var(--ink-4)" }}>no output yet</span>}
    </div>
  );
}

function AgentCard({ ag, proj, onOpen, onAct, asleep, onSleep }) {
  const m = STATUS_META[ag.status];
  const totAdd = ag.files.reduce((s, f) => s + f.add, 0);
  const totDel = ag.files.reduce((s, f) => s + f.del, 0);
  return (
    <div className="surface rise" onClick={() => onOpen(ag.id)}
      style={{
        padding: 14, display: "flex", flexDirection: "column", gap: 11, cursor: "pointer",
        transition: "border-color 0.15s, transform 0.15s",
      }}
      onMouseEnter={(e) => { e.currentTarget.style.borderColor = "var(--hair-2)"; e.currentTarget.style.transform = "translateY(-2px)"; }}
      onMouseLeave={(e) => { e.currentTarget.style.borderColor = "var(--hair)"; e.currentTarget.style.transform = "none"; }}>
      <div style={{ display: "flex", alignItems: "flex-start", gap: 10 }}>
        <div style={{ position: "relative", flex: "none" }}>
          <Ring value={ag.progress} size={36} stroke={3} color={m.color} />
          <span style={{ position: "absolute", inset: 0, display: "grid", placeItems: "center", fontSize: 9, color: "var(--ink-2)" }} className="tnum">
            {Math.round(ag.progress * 100)}
          </span>
        </div>
        <div style={{ flex: 1, minWidth: 0 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 7, minWidth: 0 }}>
            <span className="disp" style={{ fontSize: 14, fontWeight: 600, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis", flex: "0 1 auto" }}>{ag.name}</span>
            <span style={{ flex: "none" }}><StatusPill status={ag.status} filled /></span>
          </div>
          <div style={{ fontSize: 11, color: "var(--ink-3)", marginTop: 2, display: "flex", gap: 6, alignItems: "center", minWidth: 0 }}>
            {proj && <span style={{ display: "flex", alignItems: "center", gap: 4, color: proj.color, minWidth: 0, overflow: "hidden" }}>
              <Icon name={proj.icon} size="sm" style={{ width: 11, height: 11, flex: "none" }} /><span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{proj.name}</span>
            </span>}
            <span style={{ color: "var(--ink-4)", flex: "none" }}>·</span>
            <Icon name="branch" size="sm" style={{ width: 11, height: 11, flex: "none" }} /><span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{ag.branch.replace("agent/", "")}</span>
          </div>
        </div>
      </div>

      <p style={{ fontSize: 12, color: "var(--ink-2)", lineHeight: 1.5, textWrap: "pretty", minHeight: 34 }}>
        {ag.task}
      </p>

      {ag.status === "blocked" && (
        <div style={{ display: "flex", gap: 7, padding: "7px 9px", borderRadius: "var(--r-sm)",
          background: "color-mix(in oklch, var(--st-blocked), transparent 90%)",
          border: "1px solid color-mix(in oklch, var(--st-blocked), transparent 70%)" }}>
          <Icon name="flag" size="sm" style={{ color: "var(--st-blocked)", flex: "none", marginTop: 1 }} />
          <span style={{ fontSize: 11, color: "var(--code-del-ink)", lineHeight: 1.45 }}>{ag.blockReason}</span>
        </div>
      )}

      <MiniTerm id={ag.id} />

      <div style={{ display: "flex", alignItems: "center", gap: 10, fontSize: 10.5, color: "var(--ink-3)" }} className="tnum">
        <span style={{ display: "flex", gap: 4 }}><Icon name="file" size="sm" style={{ width: 11, height: 11 }} />{ag.files.length}</span>
        <span style={{ color: "var(--code-add-ink)" }}>+{totAdd}</span>
        <span style={{ color: "var(--code-del-ink)" }}>−{totDel}</span>
        <span style={{ display: "flex", gap: 4 }}><Icon name="commit" size="sm" style={{ width: 11, height: 11 }} />{ag.commits}</span>
        {window.AgentCostChip && <span onClick={(e) => e.stopPropagation()}><window.AgentCostChip agent={ag} /></span>}
        <span style={{ marginLeft: "auto", display: "flex", gap: 4, color: "var(--ink-4)" }}>
          <Icon name="clock" size="sm" style={{ width: 11, height: 11 }} />{ag.elapsed ? fmtDur(ag.elapsed) : "—"}
        </span>
      </div>

      <div style={{ display: "flex", gap: 6 }} onClick={(e) => e.stopPropagation()}>
        {ag.status === "done"
          ? <button className="btn primary" style={{ flex: 1, justifyContent: "center" }} onClick={() => onAct(ag.id, "merge")}>
              <Icon name="merge" size="sm" />Merge</button>
          : ag.status === "blocked"
          ? <button className="btn primary" style={{ flex: 1, justifyContent: "center" }} onClick={() => onOpen(ag.id)}>
              <Icon name="chat" size="sm" />Answer</button>
          : ag.status === "queued"
          ? <button className="btn ghost-hair" style={{ flex: 1, justifyContent: "center" }} onClick={() => onAct(ag.id, "start")}>
              <Icon name="play" size="sm" />Start now</button>
          : <button className="btn ghost-hair" style={{ flex: 1, justifyContent: "center" }} onClick={() => onAct(ag.id, ag.status === "running" ? "pause" : "resume")}>
              <Icon name={ag.status === "running" ? "pause" : "play"} size="sm" />{ag.status === "running" ? "Pause" : "Resume"}</button>}
        <button className="btn ghost-hair" onClick={() => onSleep && onSleep(ag.id)} style={{ padding: "5px 9px", color: asleep ? "var(--accent)" : "var(--ink-3)" }}
          title={asleep ? "Wake — recreate the terminal and replay its scrollback ring" : "Sleep — disposes the xterm + WebGL context, frees ~6 MB (the Rust ring keeps the output)"}>
          <Icon name="moon" size="sm" />{asleep ? "Wake" : "Sleep"}
        </button>
        <button className="btn ghost-hair" onClick={() => onOpen(ag.id)} style={{ padding: "5px 9px" }}>
          <Icon name="terminal" size="sm" />Open
        </button>
      </div>
    </div>
  );
}

function GridView({ agents, projects, onOpen, onAct, asleep, onSleep }) {
  return (
    <div style={{
      display: "grid", gap: 14, padding: 18,
      gridTemplateColumns: "repeat(auto-fill, minmax(320px, 1fr))", alignContent: "start",
    }}>
      {agents.map((ag) => <AgentCard key={ag.id} ag={ag} proj={projects.find((p) => p.id === ag.projectId)} onOpen={onOpen} onAct={onAct} asleep={!!(asleep && asleep[ag.id])} onSleep={onSleep} />)}
    </div>
  );
}

function KanbanView({ agents, onOpen }) {
  const cols = [
    { key: "queued", label: "Queued" },
    { key: "running", label: "Running" },
    { key: "blocked", label: "Needs you", alt: "waiting" },
    { key: "done", label: "Done" },
  ];
  const colItems = (c) => agents.filter((a) => a.status === c.key || a.status === c.alt);
  return (
    <div style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: 12, padding: 18, alignItems: "start", minHeight: 0 }}>
      {cols.map((c) => {
        const items = colItems(c);
        const color = STATUS_META[c.key].color;
        return (
          <div key={c.key} style={{ display: "flex", flexDirection: "column", gap: 9 }}>
            <div style={{ display: "flex", alignItems: "center", gap: 7, padding: "0 2px 4px", borderBottom: "2px solid " + color }}>
              <span className="up" style={{ fontSize: 10, color: "var(--ink-2)" }}>{c.label}</span>
              <span className="chip tnum" style={{ marginLeft: "auto", fontSize: 9.5, padding: "0px 6px" }}>{items.length}</span>
            </div>
            {items.map((ag) => {
              const totAdd = ag.files.reduce((s, f) => s + f.add, 0);
              return (
                <div key={ag.id} className="surface rise" onClick={() => onOpen(ag.id)}
                  style={{ padding: 11, cursor: "pointer", display: "flex", flexDirection: "column", gap: 7 }}
                  onMouseEnter={(e) => e.currentTarget.style.borderColor = "var(--hair-2)"}
                  onMouseLeave={(e) => e.currentTarget.style.borderColor = "var(--hair)"}>
                  <div style={{ display: "flex", alignItems: "center", gap: 7 }}>
                    <StatusDot status={ag.status} />
                    <span className="disp" style={{ fontSize: 12.5, fontWeight: 600 }}>{ag.name}</span>
                  </div>
                  <span style={{ fontSize: 11, color: "var(--ink-2)", lineHeight: 1.45, textWrap: "pretty" }}>{ag.task}</span>
                  {ag.status === "running" && <div className="activity" />}
                  <div style={{ display: "flex", alignItems: "center", gap: 8, fontSize: 10, color: "var(--ink-3)" }} className="tnum">
                    <Icon name="branch" size="sm" style={{ width: 10, height: 10 }} />
                    <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{ag.branch.replace("agent/", "")}</span>
                    <span style={{ marginLeft: "auto", color: "var(--code-add-ink)" }}>+{totAdd}</span>
                  </div>
                </div>
              );
            })}
            {!items.length && <div style={{ padding: 14, textAlign: "center", color: "var(--ink-4)", fontSize: 10.5, border: "1px dashed var(--hair)", borderRadius: "var(--r-md)" }}>empty</div>}
          </div>
        );
      })}
    </div>
  );
}

function GraphView({ agents, projects, onOpen }) {
  // left→right hierarchy: orchestrator → projects → agents
  const rowH = 50, W = 780;
  const xRoot = 54, xProj = 250, xAgent = 470;
  let y = 26;
  const laid = projects.map((p) => {
    const ags = agents.filter((a) => a.projectId === p.id);
    const count = Math.max(ags.length, 1);
    const top = y;
    const agentNodes = ags.map((ag, i) => ({ ag, y: top + i * rowH + rowH / 2 }));
    const projY = top + (count * rowH) / 2;
    y += count * rowH + 18;
    return { p, projY, agentNodes };
  });
  const H = y + 8;
  const rootY = H / 2;

  return (
    <div style={{ padding: 18, display: "grid", placeItems: "center", minHeight: 0, overflow: "auto" }}>
      <svg viewBox={`0 0 ${W} ${H}`} style={{ width: "100%", maxWidth: 920, height: "auto" }}>
        <defs>
          <radialGradient id="core" cx="50%" cy="50%">
            <stop offset="0" stopColor="var(--accent)" stopOpacity="0.4" />
            <stop offset="1" stopColor="var(--accent)" stopOpacity="0" />
          </radialGradient>
        </defs>
        {/* root → project edges */}
        {laid.map(({ p, projY }, i) => (
          <path key={"e" + i} d={`M${xRoot + 22},${rootY} C${(xRoot + xProj) / 2},${rootY} ${(xRoot + xProj) / 2},${projY} ${xProj - 18},${projY}`}
            fill="none" stroke={p.color} strokeWidth="1.6" strokeOpacity="0.5" />
        ))}
        {/* project → agent edges */}
        {laid.map(({ p, projY, agentNodes }) => agentNodes.map((n, j) => {
          const c = STATUS_META[n.ag.status].color;
          const run = n.ag.status === "running";
          return (
            <path key={p.id + j} d={`M${xProj + 16},${projY} C${(xProj + xAgent) / 2},${projY} ${(xProj + xAgent) / 2},${n.y} ${xAgent - 10},${n.y}`}
              fill="none" stroke={c} strokeWidth="1.4" strokeOpacity={n.ag.status === "queued" ? 0.22 : 0.5}
              strokeDasharray={run ? "4 4" : "none"}>
              {run && <animate attributeName="stroke-dashoffset" from="16" to="0" dur="0.7s" repeatCount="indefinite" />}
            </path>
          );
        }))}
        {/* root node */}
        <circle cx={xRoot} cy={rootY} r="40" fill="url(#core)" />
        <circle cx={xRoot} cy={rootY} r="20" fill="var(--panel-3)" stroke="var(--accent)" strokeWidth="1.6" />
        <text x={xRoot} y={rootY + 1} textAnchor="middle" fontFamily="var(--font-disp)" fontSize="9" fontWeight="700" fill="var(--ink)">ORCH</text>
        <text x={xRoot} y={rootY + 36} textAnchor="middle" fontFamily="var(--font-mono)" fontSize="9" fill="var(--ink-3)">{ORG}</text>
        {/* project nodes */}
        {laid.map(({ p, projY }) => (
          <g key={p.id}>
            <rect x={xProj - 16} y={projY - 16} width="32" height="32" rx="9" fill="var(--panel-3)" stroke={p.color} strokeWidth="1.6" />
            <text x={xProj} y={projY + 5} textAnchor="middle" fontFamily="var(--font-disp)" fontSize="13" fontWeight="700" fill={p.color}>{p.name[0].toUpperCase()}</text>
            <text x={xProj + 26} y={projY - 2} fontFamily="var(--font-disp)" fontSize="12" fontWeight="600" fill="var(--ink)">{p.name}</text>
            <text x={xProj + 26} y={projY + 11} fontFamily="var(--font-mono)" fontSize="9" fill="var(--ink-3)">{p.branch} · {p.head}</text>
          </g>
        ))}
        {/* agent nodes */}
        {laid.map(({ agentNodes }) => agentNodes.map((n) => {
          const c = STATUS_META[n.ag.status].color;
          return (
            <g key={n.ag.id} style={{ cursor: "pointer" }} onClick={() => onOpen(n.ag.id)}>
              <circle cx={xAgent} cy={n.y} r="8" fill="var(--panel-3)" stroke={c} strokeWidth="2" />
              <circle cx={xAgent} cy={n.y} r="3.2" fill={c}>
                {n.ag.status === "running" && <animate attributeName="r" values="3.2;5;3.2" dur="1.4s" repeatCount="indefinite" />}
              </circle>
              <text x={xAgent + 16} y={n.y - 1} fontFamily="var(--font-disp)" fontSize="12" fontWeight="600" fill="var(--ink)">{n.ag.name}</text>
              <text x={xAgent + 16} y={n.y + 12} fontFamily="var(--font-mono)" fontSize="9" fill="var(--ink-3)">{STATUS_META[n.ag.status].label} · {n.ag.commits}c · {n.ag.branch.replace("agent/", "")}</text>
            </g>
          );
        }))}
        {/* empty projects */}
        {laid.filter(({ agentNodes }) => !agentNodes.length).map(({ p, projY }) => (
          <text key={"empty" + p.id} x={xAgent} y={projY + 4} fontFamily="var(--font-mono)" fontSize="10" fill="var(--ink-4)">no agents</text>
        ))}
      </svg>
    </div>
  );
}

function TimelineView({ agents, onOpen }) {
  const maxEl = Math.max(...agents.map((a) => a.elapsed), 1);
  return (
    <div style={{ padding: 18, display: "flex", flexDirection: "column", gap: 2 }}>
      <div style={{ display: "grid", gridTemplateColumns: "150px 1fr 70px", gap: 12, padding: "0 4px 8px", fontSize: 9.5 }} className="up">
        <span style={{ color: "var(--ink-3)" }}>Agent</span>
        <span style={{ color: "var(--ink-3)" }}>Elapsed · progress</span>
        <span style={{ color: "var(--ink-3)", textAlign: "right" }}>Commits</span>
      </div>
      {agents.map((ag) => {
        const c = STATUS_META[ag.status].color;
        const w = (ag.elapsed / maxEl) * 100;
        return (
          <div key={ag.id} onClick={() => onOpen(ag.id)}
            style={{ display: "grid", gridTemplateColumns: "150px 1fr 70px", gap: 12, alignItems: "center",
              padding: "9px 4px", cursor: "pointer", borderRadius: "var(--r-sm)", borderBottom: "1px solid var(--hair)" }}
            onMouseEnter={(e) => e.currentTarget.style.background = "var(--panel-2)"}
            onMouseLeave={(e) => e.currentTarget.style.background = "transparent"}>
            <div style={{ display: "flex", alignItems: "center", gap: 7, minWidth: 0 }}>
              <StatusDot status={ag.status} />
              <span className="disp" style={{ fontSize: 12.5, fontWeight: 600, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{ag.name}</span>
            </div>
            <div style={{ position: "relative", height: 22, display: "flex", alignItems: "center" }}>
              <div style={{ position: "absolute", left: 0, height: 9, width: Math.max(w, 3) + "%",
                borderRadius: 5, background: "color-mix(in oklch, " + c + ", transparent 55%)",
                border: "1px solid " + c, overflow: "hidden" }}>
                <div style={{ height: "100%", width: (ag.progress * 100) + "%", background: c, opacity: 0.5 }} />
                {ag.status === "running" && <div className="activity" style={{ position: "absolute", inset: 0, background: "transparent" }} />}
              </div>
              <span className="tnum" style={{ position: "absolute", left: "calc(" + Math.max(w, 3) + "% + 8px)", fontSize: 10, color: "var(--ink-3)", whiteSpace: "nowrap" }}>
                {ag.elapsed ? fmtDur(ag.elapsed) : "—"} · {Math.round(ag.progress * 100)}%
              </span>
            </div>
            <span className="tnum" style={{ textAlign: "right", fontSize: 11, color: "var(--ink-2)" }}>{ag.commits}</span>
          </div>
        );
      })}
    </div>
  );
}

function Overview({ agents, projects, viz, setViz, onOpen, onAct, onSpawn, loading, asleep, onSleep }) {
  const count = (s) => agents.filter((a) => a.status === s).length;
  const VIZ = [
    { key: "grid", icon: "grid", label: "Grid" },
    { key: "kanban", icon: "columns", label: "Board" },
    { key: "graph", icon: "graph", label: "Graph" },
    { key: "timeline", icon: "timeline", label: "Timeline" },
  ];
  return (
    <div style={{ display: "flex", flexDirection: "column", minHeight: 0, background: "var(--panel-2)" }}>
      {/* stat header */}
      <div style={{ display: "flex", alignItems: "center", gap: 0, padding: "14px 18px",
        borderBottom: "1px solid var(--hair)", background: "var(--panel)" }}>
        <div style={{ marginRight: 24 }}>
          <h1 className="disp" style={{ fontSize: 16, fontWeight: 600, letterSpacing: "-0.02em" }}>Orchestrator</h1>
          {loading
            ? <span className="skel" style={{ width: 220, height: 11, borderRadius: 3, marginTop: 4 }} />
            : <span style={{ fontSize: 10.5, color: "var(--ink-3)" }}>{agents.length} agents across {projects.length} projects · {ORG}</span>}
        </div>
        {loading
          ? ["Running", "Need you", "Waiting", "Done"].map((label) => (
              <div key={label} style={{ display: "flex", flexDirection: "column", gap: 5, paddingRight: 20 }}>
                <span className="skel" style={{ width: 26, height: 22, borderRadius: 5 }} />
                <span className="skel" style={{ width: 40, height: 8, borderRadius: 3 }} />
              </div>
            ))
          : <React.Fragment>
              <StatBlock n={count("running")} label="Running" color="var(--st-running)" pulse />
              <StatBlock n={count("blocked")} label="Need you" color="var(--st-blocked)" />
              <StatBlock n={count("waiting") + count("queued")} label="Waiting" color="var(--st-waiting)" />
              <StatBlock n={count("done")} label="Done" color="var(--st-done)" />
            </React.Fragment>}
        <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 8 }}>
          {/* viz switcher */}
          <div style={{ display: "flex", gap: 2, padding: 3, background: "var(--panel-2)", borderRadius: "var(--r-md)", border: "1px solid var(--hair)" }}>
            {VIZ.map((v) => (
              <button key={v.key} className="btn" onClick={() => setViz(v.key)}
                style={{ padding: "4px 9px", borderRadius: "var(--r-sm)",
                  background: viz === v.key ? "var(--panel-3)" : "transparent",
                  color: viz === v.key ? "var(--ink)" : "var(--ink-3)",
                  boxShadow: viz === v.key ? "0 0 0 1px var(--hair-2)" : "none" }}>
                <Icon name={v.icon} size="sm" style={{ color: viz === v.key ? "var(--accent)" : "inherit" }} />
                {v.label}
              </button>
            ))}
          </div>
          <button className="btn primary" onClick={onSpawn}><Icon name="bolt" size="sm" />Spawn</button>
        </div>
      </div>

      {/* body */}
      <div className="scroll-y" style={{ flex: 1 }}>
        {loading
          ? <OverviewSkeleton count={6} />
          : <React.Fragment>
              {viz === "grid" && <GridView agents={agents} projects={projects} onOpen={onOpen} onAct={onAct} asleep={asleep} onSleep={onSleep} />}
              {viz === "kanban" && <KanbanView agents={agents} onOpen={onOpen} />}
              {viz === "graph" && <GraphView agents={agents} projects={projects} onOpen={onOpen} />}
              {viz === "timeline" && <TimelineView agents={agents} onOpen={onOpen} />}
            </React.Fragment>}
      </div>
    </div>
  );
}

Object.assign(window, { Overview });
