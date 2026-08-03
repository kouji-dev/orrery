/* global React, Icon, StatusDot, ToolBadge, toolMeta, projectOf, treeAgentIds */
// ORCHESTRA top bar — brand, draggable/groupable agent tabs, run widget, theme

const { useState: useStateT } = React;

// ---- Orrery mark: epicycle rosette (deferent A + epicycle B at freq K) ----
const _RA = 22, _RB = 13, _RK = -4, _RTAU = Math.PI * 2;
function _rosettePath(N) {
  let d = "";
  for (let i = 0; i <= N; i++) {
    const t = i / N * _RTAU;
    const x = 50 + _RA * Math.cos(t) + _RB * Math.cos(_RK * t);
    const y = 50 + _RA * Math.sin(t) + _RB * Math.sin(_RK * t);
    d += (i ? "L" : "M") + x.toFixed(2) + " " + y.toFixed(2) + " ";
  }
  return d + "Z";
}
const ORRERY_PATH = _rosettePath(160);

// channel metadata: dev = pre-release, prod = shipped (shown as Beta while in beta)
function VersionBadge({ build, onClick }) {
  const isProd = build.channel === "prod";
  const label = isProd ? "BETA" : "DEV";
  const color = isProd ? "var(--accent-2)" : "#f5c451";
  return (
    <span onClick={onClick} title={"Orrery v" + build.version + " · " + (isProd ? "production (beta)" : "development build") + " · " + build.commit + " · " + build.built + (onClick ? " · what’s new" : "")}
      style={{ display: "inline-flex", alignItems: "center", gap: 5, height: 17, padding: "0 7px", borderRadius: 999,
        border: "1px solid color-mix(in oklch, " + color + ", transparent 58%)",
        background: "color-mix(in oklch, " + color + ", transparent 88%)", cursor: onClick ? "pointer" : "default",
        fontFamily: "var(--font-mono)", fontWeight: 500, letterSpacing: "0.02em", whiteSpace: "nowrap" }}>
      <span style={{ fontSize: 10, color: "var(--ink-2)" }}>v{build.version}</span>
      <span style={{ width: 1, height: 9, background: "color-mix(in oklch, " + color + ", transparent 50%)" }} />
      <span className="up" style={{ fontSize: 8.5, fontWeight: 700, letterSpacing: "0.12em", color }}>{label}</span>
    </span>
  );
}

function Logo({ size = 22 }) {
  return (
    <svg width={size} height={size} viewBox="0 0 100 100" fill="none" aria-label="Orrery">
      <defs>
        <linearGradient id="orose" x1="0" y1="0" x2="1" y2="1">
          <stop offset="0" stopColor="var(--accent-3)" />
          <stop offset="0.5" stopColor="var(--accent)" />
          <stop offset="1" stopColor="var(--accent-2)" />
        </linearGradient>
        <radialGradient id="ocore" cx="50%" cy="50%">
          <stop offset="0" stopColor="#fff" stopOpacity="0.95" />
          <stop offset="0.4" stopColor="var(--accent)" />
          <stop offset="1" stopColor="var(--accent)" stopOpacity="0.2" />
        </radialGradient>
      </defs>
      <path d={ORRERY_PATH} fill="none" stroke="url(#orose)" strokeWidth="4.6" strokeLinejoin="round" />
      <circle cx="50" cy="50" r="12" fill="url(#ocore)" />
    </svg>
  );
}

// label content for a tab (single agent or a group)
function TabLabel({ tab, agents, tickets, active }) {
  if (tab.kind === "orchestrator") {
    return (<>
      <Icon name="layers" size="sm" style={{ color: active ? "var(--accent)" : "inherit" }} />
      <span style={{ fontSize: 12 }}>Orchestrator</span>
    </>);
  }
  if (tab.kind === "backlog") {
    const open = (tickets || []).filter((t) => t.status !== "done").length;
    return (<>
      <Icon name="columns" size="sm" style={{ color: active ? "var(--accent)" : "inherit" }} />
      <span style={{ fontSize: 12 }}>Backlog</span>
      {open > 0 && <span className="chip tnum" style={{ fontSize: 9, padding: "0 5px" }}>{open}</span>}
    </>);
  }
  if (tab.kind === "ticket") {
    const t = tab.ticketId === "new" ? null : (tickets || []).find((x) => x.id === tab.ticketId);
    return (<>
      <Icon name="layers" size="sm" style={{ color: active ? "var(--accent)" : "var(--ink-3)" }} />
      <span style={{ fontSize: 12, maxWidth: 154, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", display: "inline-block" }}>{t ? t.title : "New ticket"}</span>
    </>);
  }
  const ids = treeAgentIds(tab.root);
  const tabAgents = ids.map((id) => agents.find((a) => a.id === id)).filter(Boolean);
  if (tabAgents.length <= 1) {
    const ag = tabAgents[0];
    const proj = ag ? projectOf(ag.projectId) : null;
    return (<>
      <StatusDot status={ag ? ag.status : "idle"} />
      {proj && <span style={{ width: 6, height: 6, borderRadius: 2, background: proj.color, flex: "none" }} title={proj.name} />}
      <span style={{ fontSize: 12 }}>{ag ? ag.name : "agent"}</span>
    </>);
  }
  // group
  return (<>
    <Icon name="columns" size="sm" style={{ color: active ? "var(--accent)" : "var(--ink-3)" }} />
    <span style={{ display: "flex", gap: 2 }}>
      {tabAgents.slice(0, 3).map((a) => <StatusDot key={a.id} status={a.status} />)}
    </span>
    <span style={{ fontSize: 12 }}>{tabAgents[0].name} <span style={{ color: "var(--ink-4)" }}>+{tabAgents.length - 1}</span></span>
  </>);
}

function TopBar({ nav, onSearchEverywhere, tabs, activeTab, onTab, onCloseTab, onTabContext, agents, projects, tickets, onTheme, theme, onRunAll, running, onMergeTabs, onReorderTab, onAddAgent, win, onHide, onMinimize, onMaximize, onClose, build, compact, rightPanel, onSettings, onShowWhatsNew }) {
  const [dragId, setDragId] = useStateT(null);
  const [drop, setDrop] = useStateT(null); // { id, zone: 'merge'|'before'|'after' }

  const onDragOver = (e, tab) => {
    const d = window.__omDrag;
    if (!d || tab.kind !== "agent") return;
    if (d.kind === "agent") {
      e.preventDefault();
      setDrop({ id: tab.id, zone: "merge" });
      return;
    }
    if (d.kind === "tab") {
      if (d.id === tab.id) return;
      const dragged = tabs.find((t) => t.id === d.id);
      if (!dragged || dragged.kind === "orchestrator") return;
      e.preventDefault();
      const r = e.currentTarget.getBoundingClientRect();
      const x = e.clientX - r.left;
      const zone = x < r.width * 0.28 ? "before" : x > r.width * 0.72 ? "after" : "merge";
      setDrop({ id: tab.id, zone });
    }
  };
  const onDrop = (e, tab) => {
    e.preventDefault();
    const d = window.__omDrag;
    if (!d) { setDrop(null); return; }
    if (d.kind === "agent") {
      onAddAgent(d.id, tab.id);
    } else if (d.kind === "tab" && drop && drop.id === tab.id) {
      if (drop.zone === "merge") onMergeTabs(d.id, tab.id);
      else onReorderTab(d.id, tab.id, drop.zone === "before");
    }
    setDragId(null); setDrop(null); window.__omDrag = null;
  };

  return (
    <header style={{ display: "flex", alignItems: "stretch", background: "var(--panel)", borderBottom: "1px solid var(--hair)", height: 44, position: "relative", zIndex: 5 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 11, flex: "none", overflow: "hidden",
        width: compact ? "var(--rail-w)" : "var(--sidebar-w)",
        padding: compact ? 0 : "0 14px", justifyContent: compact ? "center" : "flex-start",
        borderRight: "1px solid var(--hair)" }}>
        <Logo size={24} />
        {!compact && (
          <div style={{ display: "flex", flexDirection: "column", lineHeight: 1.12, minWidth: 0 }}>
            <span className="disp" style={{ fontSize: 15, fontWeight: 600, letterSpacing: "0.005em", display: "flex", alignItems: "center", gap: 8 }}>
              <span><span style={{ color: "var(--accent)" }}>O</span>rrery</span>
              <VersionBadge build={build} onClick={onShowWhatsNew} />
            </span>
            <span style={{ fontSize: 9.5, color: "var(--ink-3)", letterSpacing: "0.04em", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{projects.length} projects · {agents.length} agents</span>
          </div>
        )}
      </div>

      {/* tabs */}
      <div style={{ display: "flex", alignItems: "stretch", flex: 1, minWidth: 0, overflowX: "auto" }}>
        {tabs.map((tab) => {
          const active = activeTab === tab.id;
          const isGroup = tab.kind === "agent" && treeAgentIds(tab.root).length > 1;
          const d = drop && drop.id === tab.id ? drop.zone : null;
          const draggable = tab.kind === "agent";
          return (
            <div key={tab.id} onClick={() => onTab(tab.id)}
              draggable={draggable}
              onDragStart={(e) => { setDragId(tab.id); window.__omDrag = { kind: "tab", id: tab.id, agentId: tab.kind === "agent" ? treeAgentIds(tab.root)[0] : null }; e.dataTransfer.effectAllowed = "move"; }}
              onDragEnd={() => { setDragId(null); setDrop(null); window.__omDrag = null; }}
              onDragOver={(e) => onDragOver(e, tab)}
              onDragLeave={() => setDrop((cur) => cur && cur.id === tab.id ? null : cur)}
              onDrop={(e) => onDrop(e, tab)}
              onContextMenu={(e) => { if (tab.kind === "agent") onTabContext(e, tab); }}
              style={{ display: "flex", alignItems: "center", gap: 8, padding: "0 13px", cursor: "pointer", whiteSpace: "nowrap", position: "relative",
                borderRight: "1px solid var(--hair)", background: active ? "var(--panel-2)" : "transparent", color: active ? "var(--ink)" : "var(--ink-3)",
                opacity: dragId === tab.id ? 0.45 : 1,
                boxShadow: d === "merge" ? "inset 0 0 0 2px color-mix(in oklch, var(--accent), transparent 35%)" : "none" }}>
              {active && <span style={{ position: "absolute", left: 0, right: 0, top: 0, height: 2, background: "linear-gradient(90deg, var(--accent), var(--accent-2))" }} />}
              {/* reorder insertion bars */}
              {d === "before" && <span style={{ position: "absolute", left: -1, top: 4, bottom: 4, width: 3, borderRadius: 2, background: "var(--accent)" }} />}
              {d === "after" && <span style={{ position: "absolute", right: -1, top: 4, bottom: 4, width: 3, borderRadius: 2, background: "var(--accent)" }} />}
              {d === "merge" && <span style={{ position: "absolute", inset: 0, background: "color-mix(in oklch, var(--accent), transparent 90%)", pointerEvents: "none" }} />}

              <TabLabel tab={tab} agents={agents} tickets={tickets} active={active} />
              {(tab.kind === "agent" || tab.kind === "ticket") && (
                <button onClick={(e) => { e.stopPropagation(); onCloseTab(tab.id); }}
                  style={{ background: "transparent", border: "none", color: "var(--ink-4)", cursor: "pointer", display: "flex", padding: 1, borderRadius: 3, marginLeft: 2 }}
                  onMouseEnter={(e) => e.currentTarget.style.color = "var(--ink)"}
                  onMouseLeave={(e) => e.currentTarget.style.color = "var(--ink-4)"}>
                  <Icon name="x" size="sm" />
                </button>
              )}
            </div>
          );
        })}
        {/* hint */}
        {tabs.filter((t) => t.kind === "agent").length >= 2 && (
          <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "0 12px", color: "var(--ink-4)", fontSize: 10, whiteSpace: "nowrap" }}>
            <Icon name="columns" size="sm" style={{ width: 12, height: 12 }} />drag a tab onto another to tile them
          </div>
        )}
        {/* nav stack + search everywhere */}
        <div style={{ marginLeft: "auto", display: "flex", alignItems: "center", gap: 7, padding: "0 12px", flex: "none" }}>
          {nav && <NavArrows {...nav} />}
          {onSearchEverywhere && (
            <button className="btn ghost-hair" onClick={onSearchEverywhere} title={"Search Everywhere · " + kbdLabel("Shift Shift")}
              style={{ height: 26, padding: "0 7px", gap: 5, fontSize: 10.5, color: "var(--ink-3)" }}>
              <Icon name="search" size="sm" /><span className="kbd">{kbdLabel("Shift Shift")}</span>
            </button>
          )}
        </div>
      </div>

      {/* right cluster — column-aligned to the right panel */}
      <div style={{ display: "flex", alignItems: "stretch", flex: "none",
        width: rightPanel ? "var(--right-w)" : "auto",
        borderLeft: "1px solid var(--hair)" }}>
        <div style={{ display: "flex", alignItems: "center", gap: 7, padding: "0 12px", flex: 1, minWidth: 0, justifyContent: "flex-end" }}>
          <div className="action-pill" role="group" aria-label="Workspace controls">
            <button className={"pill-seg run" + (running ? " running" : "")} onClick={onRunAll}
              title={running ? "Pause all agents" : "Run all agents"} aria-label={running ? "Pause all" : "Run all"}>
              <Icon name={running ? "pause" : "play"} size="sm" />
            </button>
            <span className="pill-div" />
            <button className="pill-seg" onClick={onTheme} title="Toggle theme" aria-label="Toggle theme">
              <Icon name={theme === "dark" ? "sun" : "moon"} size="sm" />
            </button>
            <span className="pill-div" />
            <button className="pill-seg" onClick={onSettings} title="Settings" aria-label="Settings">
              <Icon name="settings" size="sm" />
            </button>
          </div>
        </div>
        <div className="vdiv" />
        <WindowControls win={win} onHide={onHide} onMinimize={onMinimize} onMaximize={onMaximize} onClose={onClose} />
      </div>
    </header>
  );
}

function WinBtn({ title, danger, onClick, children }) {
  return (
    <button className={"winbtn" + (danger ? " danger" : "")} title={title} onClick={onClick} aria-label={title}>
      {children}
    </button>
  );
}

function WindowControls({ win, onHide, onMinimize, onMaximize, onClose }) {
  const s = { width: 13, height: 13, viewBox: "0 0 24 24", fill: "none", stroke: "currentColor",
    strokeWidth: 1.8, strokeLinecap: "round", strokeLinejoin: "round" };
  return (
    <div style={{ display: "flex", alignItems: "stretch", height: "100%", flex: "none" }}>
      <WinBtn title="Hide to tray" onClick={onHide}>
        <svg {...s}><path d="M3 3l18 18" /><path d="M10.6 10.7a2 2 0 002.7 2.8" /><path d="M9.4 5.2A9 9 0 0121 12a16 16 0 01-2.1 2.8M6.3 6.2A16 16 0 003 12a9 9 0 0011 6.4" /></svg>
      </WinBtn>
      <WinBtn title="Minimize" onClick={onMinimize}>
        <svg {...s}><path d="M6 12h12" /></svg>
      </WinBtn>
      <WinBtn title={win === "normal" ? "Maximize" : "Restore"} onClick={onMaximize}>
        {win === "normal"
          ? <svg {...s}><rect x="6" y="6" width="12" height="12" rx="1.5" /></svg>
          : <svg {...s}><rect x="8.5" y="8.5" width="9.5" height="9.5" rx="1.5" /><path d="M6 15.5V6h9.5" /></svg>}
      </WinBtn>
      <WinBtn title="Close" danger onClick={onClose}>
        <svg {...s}><path d="M6 6l12 12M18 6L6 18" /></svg>
      </WinBtn>
    </div>
  );
}

Object.assign(window, { TopBar, Logo, WindowControls });
