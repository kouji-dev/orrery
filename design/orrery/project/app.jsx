/* global React, ReactDOM, TopBar, Sidebar, CompactRail, Overview, Workspace, PaneManager, makeLeaf, RightPanel, Icon, ContextMenu, ToolBadge,
   AGENTS, PROJECTS, AGENT_TOOLS, ORG, WORKTREE_ROOT, BUILD, LOGS, STREAM, COMMITS, fmtDur, toolMeta,
   useTweaks, TweaksPanel, TweakSection, TweakColor, TweakRadio, TweakToggle, TweakSelect, DevConsole,
   SidebarSkeleton, OverviewSkeleton */
// ORCHESTRA — app shell, state, live streaming, tweaks

const { useState, useEffect, useRef } = React;

const PALETTES = {
  "Nebula": ["#a855f7", "#22d3ee"],
  "Plasma": ["#ff4d8d", "#a855f7"],
  "Reactor": ["#00e5c7", "#3b82f6"],
  "Ember": ["#ff6b35", "#ffd23f"],
};
const PROJECT_COLORS = ["#a855f7", "#22d3ee", "#34e0a1", "#ff6b35", "#ff4d8d", "#3b82f6", "#f5c451"];
const PROJECT_ICONS = ["box", "globe", "server", "database", "grid", "layers", "terminal", "spark"];
const NAMES = ["refund-flow", "rate-limiter", "audit-log", "webhook-dlq", "fee-engine", "ledger-sync", "fraud-rules", "currency-fx"];

function hexRgb(hex) {
  const h = hex.replace("#", "");
  const n = parseInt(h.length === 3 ? h.split("").map((c) => c + c).join("") : h, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255].join(", ");
}

const TWEAK_DEFAULTS = /*EDITMODE-BEGIN*/{
  "theme": "dark",
  "palette": ["#a855f7", "#22d3ee"],
  "density": "regular",
  "defaultViz": "grid",
  "rightPanel": true,
  "motion": true,
  "channel": "dev",
  "simulateUpdates": true
}/*EDITMODE-END*/;

// ---------------------------------------------------------------- Spawn modal
function FieldLabel({ children }) {
  return <label className="field-label">{children}</label>;
}

const slugName = (title) => (title || "").toLowerCase().replace(/[^a-z0-9]+/g, "-").replace(/^-+|-+$/g, "").split("-").filter(Boolean).slice(0, 3).join("-") || "agent";

function SpawnModal({ projects, tickets = [], defaultProject, defaultTicket, onClose, onSpawn }) {
  const [ticketId, setTicketId] = useState(defaultTicket || "");
  const ticket = tickets.find((t) => t.id === ticketId) || null;
  const [projectId, setProjectId] = useState(defaultProject || (ticket && ticket.projectId) || projects[0].id);
  const [prompt, setPrompt] = useState("");
  const [name, setName] = useState("");
  const [toolId, setToolId] = useState("claude");
  const tool = AGENT_TOOLS.find((t) => t.id === toolId);
  const [model, setModel] = useState(tool.models[0]);
  const [effort, setEffort] = useState(tool.effort ? "high" : null);
  const proj = projects.find((p) => p.id === projectId) || projects[0];
  const [branch, setBranch] = useState(proj.branch);
  const ref = useRef(null);
  const openTk = tickets.filter((t) => t.status !== "done");
  const linked = !!ticket;

  // selecting a ticket prefills + links Name and Initial prompt
  const applyTicket = (id) => {
    setTicketId(id);
    const tk = tickets.find((t) => t.id === id);
    if (tk) {
      if (tk.projectId) setProjectId(tk.projectId);
      setName(slugName(tk.title));
      const np = window.plainText ? window.plainText(tk.notes) : "";
      setPrompt(tk.title + (np ? "\n\n" + np : ""));
    }
  };
  useEffect(() => { if (defaultTicket) applyTicket(defaultTicket); else if (ref.current) ref.current.focus(); }, []);
  useEffect(() => { setBranch(proj.branch); }, [projectId]);
  useEffect(() => { setModel(tool.models[0]); setEffort(tool.effort ? "high" : null); }, [toolId]);

  const inputStyle = { width: "100%", background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-md)", padding: "10px 12px", color: "var(--ink)", fontFamily: "var(--font-mono)", fontSize: 12.5, lineHeight: 1.5, outline: "none" };
  const submit = (open) => onSpawn({ projectId, branch, toolId, model, effort, name: name.trim() || null, ticketId: ticketId || null, prompt: prompt.trim() || "Explore and improve the codebase", open });

  return (
    <div onClick={onClose} style={{ position: "fixed", inset: 0, zIndex: 60, display: "grid", placeItems: "center", padding: 24, background: "rgba(0,0,0,0.5)", backdropFilter: "blur(3px)" }}>
      <div className="surface rise" onClick={(e) => e.stopPropagation()} style={{ width: 540, maxHeight: "90vh", display: "flex", flexDirection: "column", padding: 0, overflow: "hidden", boxShadow: "var(--shadow)" }}>
        <div style={{ padding: "14px 18px", borderBottom: "1px solid var(--hair)", display: "flex", alignItems: "center", gap: 9, flex: "none" }}>
          <Icon name="agent" style={{ color: "var(--accent)" }} />
          <span className="disp" style={{ fontSize: 14, fontWeight: 600, whiteSpace: "nowrap" }}>Spawn agent</span>
          <span className="chip" style={{ marginLeft: "auto", fontSize: 9.5 }}>new git worktree + branch</span>
        </div>
        <div className="scroll-y" style={{ padding: 18, display: "flex", flexDirection: "column", gap: 16, flex: 1 }}>
          {/* project + branch */}
          <div style={{ display: "flex", gap: 14 }}>
            <div style={{ flex: 1 }}>
              <FieldLabel>Project</FieldLabel>
              <select className="osel" value={projectId} onChange={(e) => setProjectId(e.target.value)}>
                {projects.map((p) => <option key={p.id} value={p.id}>{p.name}</option>)}
              </select>
              <div style={{ fontSize: 9.5, color: "var(--ink-4)", marginTop: 6, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{proj.path}</div>
            </div>
            <div style={{ flex: 1 }}>
              <FieldLabel>Source branch</FieldLabel>
              <select className="osel" value={branch} onChange={(e) => setBranch(e.target.value)}>
                {proj.branches.map((b) => <option key={b} value={b}>{b}</option>)}
              </select>
              <div style={{ fontSize: 9.5, color: "var(--ink-4)", marginTop: 6 }}>base · {proj.head}</div>
            </div>
          </div>

          {/* NEW: ticket (optional) — prefills + links Name and Initial prompt */}
          <div>
            <FieldLabel>Ticket</FieldLabel>
            <select className="osel" value={ticketId} onChange={(e) => (e.target.value ? applyTicket(e.target.value) : setTicketId(""))}
              style={{ borderColor: linked ? "color-mix(in oklch, var(--accent), transparent 55%)" : "var(--hair)" }}>
              <option value="">None — start from scratch</option>
              <optgroup label="To do">
                {openTk.filter((t) => t.status === "todo").map((t) => <option key={t.id} value={t.id}>{t.title}</option>)}
              </optgroup>
              <optgroup label="In progress">
                {openTk.filter((t) => t.status === "inprogress").map((t) => <option key={t.id} value={t.id}>{t.title}</option>)}
              </optgroup>
            </select>
            {linked
              ? <div style={{ display: "flex", alignItems: "center", gap: 5, marginTop: 6, fontSize: 10, color: "var(--accent-2)" }}>
                  <Icon name="link" size="sm" style={{ width: 11, height: 11 }} />Name &amp; initial prompt are linked to this ticket</div>
              : <div style={{ fontSize: 9.5, color: "var(--ink-4)", marginTop: 6 }}>optional · attach a ticket to prefill from it</div>}
          </div>

          {/* name */}
          <div>
            <FieldLabel>Name</FieldLabel>
            <input value={name} onChange={(e) => setName(e.target.value)} placeholder="auto · derived from the task"
              style={{ ...inputStyle, borderLeft: linked ? "2px solid var(--accent)" : inputStyle.border }}
              onFocus={(e) => (e.target.style.borderColor = "var(--accent)")} onBlur={(e) => (e.target.style.borderColor = "var(--hair)")} />
          </div>

          {/* agent tool */}
          <div>
            <FieldLabel>Agent</FieldLabel>
            <div style={{ display: "grid", gridTemplateColumns: "repeat(4, 1fr)", gap: 7 }}>
              {AGENT_TOOLS.map((tl) => {
                const on = toolId === tl.id;
                return (
                  <button key={tl.id} className="btn" onClick={() => setToolId(tl.id)}
                    style={{ flexDirection: "column", gap: 6, padding: "10px 6px", borderRadius: "var(--r-md)",
                      border: "1px solid " + (on ? "color-mix(in oklch, " + tl.accent + ", transparent 45%)" : "var(--hair)"),
                      background: on ? "color-mix(in oklch, " + tl.accent + ", transparent 88%)" : "var(--panel-2)" }}>
                    <ToolBadge tool={tl.id} size={20} />
                    <span style={{ fontSize: 10.5, color: on ? "var(--ink)" : "var(--ink-3)" }}>{tl.name}</span>
                  </button>
                );
              })}
            </div>
          </div>

          {/* model + effort */}
          <div style={{ display: "flex", gap: 14 }}>
            <div style={{ flex: 1 }}>
              <FieldLabel>Model</FieldLabel>
              <select className="osel" value={model} onChange={(e) => setModel(e.target.value)}>
                {tool.models.map((m) => <option key={m} value={m}>{m}</option>)}
              </select>
            </div>
            {tool.effort && (
              <div style={{ flex: 1 }}>
                <FieldLabel>Reasoning effort</FieldLabel>
                <div style={{ display: "flex", gap: 6 }}>
                  {tool.effort.map((ef) => (
                    <button key={ef} className="btn ghost-hair" onClick={() => setEffort(ef)}
                      style={{ flex: 1, justifyContent: "center", fontSize: 11, textTransform: "capitalize",
                        borderColor: effort === ef ? "var(--accent)" : "var(--hair)",
                        color: effort === ef ? "var(--ink)" : "var(--ink-3)",
                        background: effort === ef ? "color-mix(in oklch, var(--accent), transparent 90%)" : "transparent" }}>
                      {ef}
                    </button>
                  ))}
                </div>
              </div>
            )}
          </div>

          {/* initial prompt */}
          <div>
            <FieldLabel>Initial prompt</FieldLabel>
            <textarea ref={ref} value={prompt} onChange={(e) => setPrompt(e.target.value)} rows={3}
              placeholder="Describe what this agent should do…"
              style={{ width: "100%", resize: "none", background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-md)", padding: "10px 12px", color: "var(--ink)", fontFamily: "var(--font-mono)", fontSize: 12.5, lineHeight: 1.5, outline: "none" }}
              onFocus={(e) => e.target.style.borderColor = "var(--accent)"}
              onBlur={(e) => e.target.style.borderColor = "var(--hair)"} />
          </div>
        </div>
        <div style={{ padding: "12px 18px", borderTop: "1px solid var(--hair)", display: "flex", alignItems: "center", gap: 8, flex: "none" }}>
          <span style={{ fontSize: 10, color: "var(--ink-4)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>→ {WORKTREE_ROOT}/{proj.id}-…</span>
          <div style={{ marginLeft: "auto", display: "flex", gap: 8, flex: "none" }}>
            <button className="btn ghost-hair" onClick={onClose}>Cancel</button>
            <button className="btn ghost-hair" title="Create the worktree without opening a terminal"
              onClick={() => submit(false)}>
              <Icon name="plus" size="sm" />Create
            </button>
            <button className="btn primary" title="Create and open its terminal"
              onClick={() => submit(true)}>
              <Icon name="bolt" size="sm" />Spawn
            </button>
          </div>
        </div>
      </div>
    </div>
  );
}

// ---------------------------------------------------------- Add-project modal
function AddProjectModal({ onClose, onAdd }) {
  const [dir, setDir] = useState("");
  const [icon, setIcon] = useState(PROJECT_ICONS[0]);
  const [color, setColor] = useState(PROJECT_COLORS[0]);
  const [gitInit, setGitInit] = useState(true);
  const ref = useRef(null);
  const picker = useRef(null);
  useEffect(() => { if (ref.current) ref.current.focus(); }, []);
  const name = dir ? dir.replace(/\/+$/, "").split("/").pop() : "";
  // simulate detection: a path that mentions an existing repo dir is treated as already-git
  const detectedGit = /(\/code\/|github|\.git)/i.test(dir) && dir.length > 4;
  useEffect(() => { setGitInit(!detectedGit); }, [detectedGit]);

  const onPick = (e) => {
    const files = e.target.files;
    if (files && files.length) {
      const rel = files[0].webkitRelativePath || files[0].name;
      const folder = rel.split("/")[0];
      setDir("~/" + folder);
    }
  };

  return (
    <div onClick={onClose} style={{ position: "fixed", inset: 0, zIndex: 60, display: "grid", placeItems: "center", padding: 24, background: "rgba(0,0,0,0.5)", backdropFilter: "blur(3px)" }}>
      <div className="surface rise" onClick={(e) => e.stopPropagation()} style={{ width: 480, padding: 0, overflow: "hidden", boxShadow: "var(--shadow)" }}>
        <div style={{ padding: "14px 18px", borderBottom: "1px solid var(--hair)", display: "flex", alignItems: "center", gap: 9 }}>
          <span style={{ flex: "none", width: 24, height: 24, borderRadius: 7, display: "grid", placeItems: "center", background: "color-mix(in oklch, " + color + ", transparent 82%)", border: "1px solid color-mix(in oklch, " + color + ", transparent 55%)" }}>
            <Icon name={icon} size="sm" style={{ color }} />
          </span>
          <span className="disp" style={{ fontSize: 14, fontWeight: 600, whiteSpace: "nowrap" }}>Add project</span>
          <span className="chip" style={{ marginLeft: "auto", fontSize: 9.5 }}>git repository</span>
        </div>
        <div style={{ padding: 18, display: "flex", flexDirection: "column", gap: 16 }}>
          <div>
            <FieldLabel>Working directory</FieldLabel>
            <div style={{ display: "flex", gap: 8 }}>
              <div style={{ flex: 1, display: "flex", alignItems: "center", gap: 8, background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-md)", padding: "0 10px" }}>
                <Icon name="folder" size="sm" style={{ color: "var(--ink-4)" }} />
                <input ref={ref} value={dir} onChange={(e) => setDir(e.target.value)} placeholder="~/code/my-repo"
                  style={{ flex: 1, minWidth: 0, background: "transparent", border: "none", outline: "none", padding: "10px 0", color: "var(--ink)", fontFamily: "var(--font-mono)", fontSize: 12.5 }} />
              </div>
              <button className="btn ghost-hair" onClick={() => picker.current && picker.current.click()}><Icon name="folderOpen" size="sm" />Browse…</button>
              <input ref={picker} type="file" webkitdirectory="" directory="" multiple style={{ display: "none" }} onChange={onPick} />
            </div>
            {name && <div style={{ fontSize: 10, color: "var(--ink-4)", marginTop: 6 }}>project name → <span style={{ color: "var(--ink-2)" }}>{name}</span></div>}
          </div>

          {/* git detection + init */}
          <div onClick={() => setGitInit((v) => !v)}
            style={{ display: "flex", alignItems: "center", gap: 10, padding: "10px 12px", borderRadius: "var(--r-md)", cursor: "pointer",
              background: detectedGit ? "color-mix(in oklch, var(--st-done), transparent 92%)" : "var(--panel-2)",
              border: "1px solid " + (detectedGit ? "color-mix(in oklch, var(--st-done), transparent 70%)" : "var(--hair)") }}>
            <span style={{ flex: "none", width: 16, height: 16, borderRadius: 4, display: "grid", placeItems: "center",
              border: "1px solid " + (gitInit ? "var(--accent)" : "var(--hair-2)"),
              background: gitInit ? "var(--accent)" : "transparent" }}>
              {gitInit && <Icon name="check" size="sm" style={{ width: 11, height: 11, color: "#06070b" }} />}
            </span>
            <div style={{ flex: 1 }}>
              <div style={{ fontSize: 11.5, color: "var(--ink)" }}>{detectedGit ? "Existing git repository detected" : "Run git init (no .git found)"}</div>
              <div style={{ fontSize: 9.5, color: "var(--ink-4)", marginTop: 2 }}>{detectedGit ? "you can still re-initialize" : "initializes a repo so agents can branch + commit"}</div>
            </div>
            <Icon name={detectedGit ? "git" : "bolt"} size="sm" style={{ color: detectedGit ? "var(--st-done)" : "var(--accent)" }} />
          </div>

          <div>
            <FieldLabel>Icon</FieldLabel>
            <div style={{ display: "flex", gap: 6, flexWrap: "wrap" }}>
              {PROJECT_ICONS.map((ic) => (
                <button key={ic} className="btn" onClick={() => setIcon(ic)}
                  style={{ padding: 8, borderRadius: "var(--r-md)", border: "1px solid " + (icon === ic ? "var(--accent)" : "var(--hair)"), background: icon === ic ? "color-mix(in oklch, var(--accent), transparent 90%)" : "var(--panel-2)" }}>
                  <Icon name={ic} size="sm" style={{ color: icon === ic ? color : "var(--ink-3)" }} />
                </button>
              ))}
            </div>
          </div>
          <div>
            <FieldLabel>Color</FieldLabel>
            <div style={{ display: "flex", gap: 8 }}>
              {PROJECT_COLORS.map((c) => (
                <button key={c} onClick={() => setColor(c)}
                  style={{ width: 26, height: 26, borderRadius: "50%", cursor: "pointer", background: c, border: "2px solid " + (color === c ? "var(--ink)" : "transparent"), boxShadow: color === c ? "0 0 0 2px var(--panel), 0 0 12px -2px " + c : "none" }} />
              ))}
            </div>
          </div>
        </div>
        <div style={{ padding: "12px 18px", borderTop: "1px solid var(--hair)", display: "flex", justifyContent: "flex-end", gap: 8 }}>
          <button className="btn ghost-hair" onClick={onClose}>Cancel</button>
          <button className="btn primary" disabled={!dir.trim()} onClick={() => dir.trim() && onAdd({ path: dir.trim(), name, icon, color, gitInit })}>
            <Icon name="plus" size="sm" />Add project
          </button>
        </div>
      </div>
    </div>
  );
}

function FootAction({ icon, title, onClick, gap = 5, children }) {
  const [hover, setHover] = useState(false);
  return (
    <span role="button" tabIndex={0} title={title} onClick={onClick}
      onMouseEnter={() => setHover(true)} onMouseLeave={() => setHover(false)}
      style={{ display: "flex", gap, alignItems: "center", flex: "none", whiteSpace: "nowrap", cursor: onClick ? "pointer" : "default",
        color: hover && onClick ? "var(--ink)" : "inherit", transition: "color .12s" }}>
      {icon && <Icon name={icon} size="sm" style={{ width: 11, height: 11 }} />}{children}
    </span>
  );
}

function StatusBar({ agents, projects, toast, build, onShowWhatsNew, onOpenLog, scopeAgent, asleep, onOpenPerf, onOpenCost }) {
  const running = agents.filter((a) => a.status === "running").length;
  const blocked = agents.filter((a) => a.status === "blocked").length;
  const isProd = build.channel === "prod";
  const chColor = isProd ? "var(--accent-2)" : "#f5c451";
  const mem = window.memorySplit ? window.memorySplit(agents, (id) => !!(asleep && asleep[id])) : null;
  const mb = window.procMb || ((v) => v + " MB");
  // the footer is a fixed 24px row — measure it and shed the lowest-priority
  // items instead of letting anything wrap and clip
  const footRef = useRef(null);
  const [fw, setFw] = useState(1400);
  useEffect(() => {
    const el = footRef.current; if (!el || !window.ResizeObserver) return;
    const ro = new ResizeObserver(() => setFw(el.clientWidth));
    ro.observe(el); setFw(el.clientWidth);
    return () => ro.disconnect();
  }, []);
  const showRoot = fw > 1240, showCounts = fw > 1120, showHealth = fw > 1040, fullMem = fw > 1330;
  const item = { display: "flex", alignItems: "center", gap: 5, flex: "none", whiteSpace: "nowrap" };
  return (
    <footer ref={footRef} style={{ display: "flex", alignItems: "center", gap: 14, padding: "0 14px", background: "var(--panel)", borderTop: "1px solid var(--hair)", fontSize: 10.5, color: "var(--ink-3)", overflow: "hidden", whiteSpace: "nowrap" }}>
      <span style={{ display: "flex", gap: 6, alignItems: "center", flex: "none", whiteSpace: "nowrap" }}>
        <span className="dot running" style={{ background: "var(--st-running)" }} />{running} running
      </span>
      {blocked > 0 && <span style={{ ...item, gap: 6, color: "var(--st-blocked)" }}>
        <span className="dot" style={{ background: "var(--st-blocked)" }} />{blocked} need attention
      </span>}
      {showCounts && <span className="tnum" style={item}>{projects.length} projects · {agents.length} agents</span>}
      {showRoot && <span style={item}><Icon name="folder" size="sm" style={{ width: 11, height: 11 }} />{WORKTREE_ROOT}</span>}
      <span style={{ marginLeft: "auto", display: "flex", gap: 8, alignItems: "center", flex: "none", whiteSpace: "nowrap" }} className="tnum">
        {toast && <span className="grad-ink" style={{ fontWeight: 600 }}>{toast}</span>}
        {toast && <span style={{ color: "var(--ink-4)" }}>·</span>}
        {scopeAgent && window.LiveTokenMeter && (
          <React.Fragment>
            <FootAction title="Token & cost dashboard" onClick={onOpenCost}><window.LiveTokenMeter agent={scopeAgent} /></FootAction>
            <span style={{ color: "var(--ink-4)" }}>·</span>
          </React.Fragment>
        )}
        {showHealth && <span style={item}>
          <Icon name="link" size="sm" style={{ width: 11, height: 11 }} />orchestrator: healthy
        </span>}
        {showHealth && <span style={{ color: "var(--ink-4)" }}>·</span>}
        <FootAction icon="file" title="Open log file" onClick={onOpenLog}>Logs</FootAction>
        <span style={{ color: "var(--ink-4)" }}>·</span>
        {mem && (
          <React.Fragment>
            <FootAction icon="cpu" title={"Orrery " + mb(mem.own) + " · agents " + mb(mem.agents) + " · CPU " + mem.cpu + "% — open the process tree"} onClick={onOpenPerf}>
              <span className="tnum" style={{ whiteSpace: "nowrap" }}>
                {fullMem && <React.Fragment>{mb(mem.own)}<span style={{ color: "var(--ink-4)" }}> · </span></React.Fragment>}
                <span style={{ color: mem.agents > 1400 ? "var(--st-blocked)" : "var(--ink-2)" }}>{mb(mem.agents)}</span>
                <span style={{ color: "var(--ink-4)" }}> · </span>
                <span style={{ color: mem.cpu > 70 ? "var(--st-blocked)" : mem.cpu > 40 ? "#f5c451" : "var(--ink-2)" }}>{mem.cpu}%</span>
              </span>
              {mem.slept > 0 && fullMem && <span style={{ color: "var(--accent)" }}>· {mem.slept} slept</span>}
            </FootAction>
            <span style={{ color: "var(--ink-4)" }}>·</span>
          </React.Fragment>
        )}
        <FootAction title="Release changelog · what’s new" onClick={onShowWhatsNew} gap={6}>
          <span>v{build.version}</span>
          <span className="up" style={{ fontSize: 8.5, fontWeight: 700, letterSpacing: "0.1em", color: chColor,
            border: "1px solid color-mix(in oklch, " + chColor + ", transparent 58%)", borderRadius: 4, padding: "0 4px" }}>
            {isProd ? "BETA" : "DEV"}
          </span>
        </FootAction>
      </span>
    </footer>
  );
}

function App() {
  const [t, setTweak] = useTweaks(TWEAK_DEFAULTS);
  const [projects, setProjects] = useState(() => JSON.parse(JSON.stringify(PROJECTS)));
  const [agents, setAgents] = useState(() => JSON.parse(JSON.stringify(AGENTS)));
  const [tickets, setTickets] = useState(() => JSON.parse(JSON.stringify(TICKETS)));
  const [tabs, setTabs] = useState([{ id: "orchestrator", kind: "orchestrator" }, { id: "backlog", kind: "backlog" }, { id: "tab1", kind: "agent", root: makeLeaf("a1", "terminal") }]);
  const [activeTab, setActiveTab] = useState("orchestrator");
  const [query, setQuery] = useState("");
  const [viz, setViz] = useState(t.defaultViz);
  const [liveLogs, setLiveLogs] = useState(() => {
    const o = {}; Object.keys(LOGS).forEach((k) => { o[k] = LOGS[k].slice(); }); return o;
  });
  const [commits, setCommits] = useState(COMMITS);
  const [spawning, setSpawning] = useState(false);      // false | {project}
  const [addingProject, setAddingProject] = useState(false);
  const [running, setRunning] = useState(true);
  const [toast, setToast] = useState("");
  const [menu, setMenu] = useState(null);               // {x,y,items}
  const [openFile, setOpenFile] = useState(null);       // {agentId, path}
  const [gitView, setGitView] = useState(null);         // {agentId, kind, ...}
  const [overlay, setOverlay] = useState(null);         // command palette / search / recent / goto / keymap
  const [recentFiles, setRecentFiles] = useState([]);   // ⌃E recent-files stack
  const [asleep, setAsleep] = useState({});             // agentId → true (A1.4 slept terminals)
  const [jump, setJump] = useState(null);               // {path, line, col, at} — go-to-line target
  const [panelTool, setPanelTool] = useState(null);      // bottom tool window: {kind, mode}
  const [scopeAgentId, setScopeAgentId] = useState("a1"); // right-panel scope
  const [compact, setCompact] = useState(false);        // sidebar compact rail
  const [settingsOpen, setSettingsOpen] = useState(false);
  const [settings, setSettings] = useState(() => JSON.parse(JSON.stringify(window.SETTINGS_DEFAULTS)));
  // editor/keymap prefs live in Settings and are read straight from there
  const patchSettings = (k, v, cur) => setSettings((s) => ({ ...s, [k]: typeof v === "function" ? v(cur) : v }));
  const keymap = settings.keymap || "intellij";
  const setKeymap = (v) => patchSettings("keymap", v, keymap);
  const autosave = !!settings.autosave;
  const setAutosave = (v) => patchSettings("autosave", v, autosave);
  const structure = settings.structureView !== false;
  const setStructure = (v) => patchSettings("structureView", v, structure);
  const [notifs, setNotifs] = useState([]);             // OS-style notifications (B6.4)
  const [win, setWin] = useState("max");                // window state: max | normal | min | hidden | closed
  const [loading, setLoading] = useState(true);         // true while projects/agents are being fetched
  const [update, setUpdate] = useState({ phase: "idle" }); // idle | available | installing
  const [whatsNew, setWhatsNew] = useState(false);      // post-update "what's new" digest
  const [installedVersion, setInstalledVersion] = useState(null); // overrides build.version after a (simulated) update
  const NEWEST = (window.RELEASES || [])[0] || null;
  const updTo = NEWEST ? NEWEST.tag.replace(/^v/, "") : BUILD.version;
  const build = { ...BUILD, channel: t.channel || BUILD.channel, version: installedVersion || BUILD.version };
  const lastShown = useRef("max");
  const streamIdx = useRef({});
  const spawnCount = useRef(0);
  const ticketCount = useRef(TICKETS.length);
  const commentCount = useRef(100);
  const projCount = useRef(0);
  const tabCount = useRef(1);
  const paneHint = useRef({});
  const simulatedOnce = useRef(false);                  // gate: auto-offer an update once per session

  useEffect(() => {
    const r = document.documentElement;
    r.setAttribute("data-theme", t.theme);
    r.setAttribute("data-density", t.density);
    r.setAttribute("data-motion", t.motion ? "on" : "off");
    const [a1, a2] = t.palette;
    r.style.setProperty("--accent", a1);
    r.style.setProperty("--accent-2", a2);
    r.style.setProperty("--accent-rgb", hexRgb(a1));
    r.style.setProperty("--accent-2-rgb", hexRgb(a2));
  }, [t.theme, t.density, t.palette, t.motion]);

  useEffect(() => { setViz(t.defaultViz); }, [t.defaultViz]);

  // Simulate the initial async fetch of projects/agents (Tauri invoke → git
  // scan → agent registry). Lists render skeletons until this resolves.
  // `reloadData` re-triggers it (manual refresh from the sidebar).
  const reloadData = React.useCallback(() => {
    setLoading(true);
    clearTimeout(reloadData._t);
    reloadData._t = setTimeout(() => setLoading(false), 1100);
  }, []);
  useEffect(() => {
    // Hold skeletons until the boot splash is gone, then run the "fetch" so the
    // loading state is actually visible (boot covers the first paint otherwise).
    let fetchT, pollT;
    const startFetch = () => { fetchT = setTimeout(() => setLoading(false), 1100); };
    if (!document.getElementById("boot")) {
      startFetch();
    } else {
      pollT = setInterval(() => {
        if (!document.getElementById("boot")) { clearInterval(pollT); startFetch(); }
      }, 120);
    }
    return () => { clearTimeout(fetchT); clearInterval(pollT); };
  }, []);

  const flash = (msg) => { setToast(msg); clearTimeout(flash._t); flash._t = setTimeout(() => setToast(""), 2600); };

  // ----- updates / what's new -----
  const startInstall = () => { setSettingsOpen(false); setUpdate({ phase: "installing" }); };
  const finishInstall = () => {
    setInstalledVersion(updTo);
    setUpdate({ phase: "idle" });
    if (NEWEST) wnMarkSeen(NEWEST.tag);
    setWhatsNew(true);
    flash("updated to v" + updTo);
  };
  const showWhatsNew = () => setWhatsNew(true);
  const openLogFile = () => flash("opening " + WORKTREE_ROOT.replace(/worktrees$/, "") + "logs/orrery.log");
  // demo: reset and re-offer the update (driven from Tweaks)
  const replayUpdate = () => { setInstalledVersion(null); setWhatsNew(false); setUpdate({ phase: "available" }); };

  // Simulate a remote build landing: a short, randomized beat after the boot
  // splash clears, Orrery offers the update once. Toggle off via Tweaks.
  useEffect(() => {
    if (!t.simulateUpdates || simulatedOnce.current || installedVersion) return;
    let delayT, pollT;
    const arm = () => {
      delayT = setTimeout(() => {
        if (simulatedOnce.current) return;
        simulatedOnce.current = true;
        setUpdate((u) => (u.phase === "idle" ? { phase: "available" } : u));
      }, 7000 + Math.random() * 5000);
    };
    if (!document.getElementById("boot")) arm();
    else pollT = setInterval(() => { if (!document.getElementById("boot")) { clearInterval(pollT); arm(); } }, 200);
    return () => { clearTimeout(delayT); clearInterval(pollT); };
  }, [t.simulateUpdates, installedVersion]);

  // ----- window chrome -----
  const minimizeWin = () => { setWin("min"); flash("minimized to taskbar"); };
  const hideWin = () => { setWin("hidden"); flash("hidden to tray"); };
  const toggleMax = () => setWin((w) => { const nx = w === "normal" ? "max" : "normal"; lastShown.current = nx; return nx; });
  const closeWin = () => setWin("closed");
  const showWin = () => setWin(lastShown.current);
  const relaunch = () => window.location.reload();

  useEffect(() => {
    if (!running) return;
    const iv = setInterval(() => {
      setLiveLogs((prev) => {
        const next = { ...prev }; let changed = false;
        agents.forEach((ag) => {
          if (ag.status !== "running") return;
          const pool = STREAM[ag.id]; if (!pool) return;
          const idx = streamIdx.current[ag.id] || 0;
          if (idx < pool.length && Math.random() > 0.35) {
            next[ag.id] = [...(next[ag.id] || []), pool[idx]];
            streamIdx.current[ag.id] = idx + 1; changed = true;
          }
        });
        return changed ? next : prev;
      });
      setAgents((prev) => prev.map((ag) => ag.status === "running" ? { ...ag, elapsed: ag.elapsed + 1, progress: Math.min(0.98, ag.progress + 0.004) } : ag));
    }, 1100);
    return () => clearInterval(iv);
  }, [running, agents]);

  // ---- loop detector (A5.4) — the hook stream repeating itself is the tell ----
  const loopSeen = useRef({});
  useEffect(() => {
    if (loading || loopSeen.current.seeded) return;
    const cand = agents.find((a) => a.status === "running" && a.id !== "a1" && STREAM[a.id]);
    if (!cand) return;
    loopSeen.current.seeded = true;
    const cmd = "pnpm vitest run src/reconcile --reporter=dot";
    const err = "FAIL src/reconcile/ledger.test.ts — expected 2 calls, got 0";
    for (let i = 0; i < 3; i++) STREAM[cand.id].push({ t: "cmd", s: cmd }, { t: "err", s: err }, { t: "out", s: "clearing the cache and trying again…" });
  }, [loading, agents.length]);

  useEffect(() => {
    agents.forEach((ag) => {
      if (ag.status !== "running" || loopSeen.current[ag.id]) return;
      if ((ag.pending || []).some((p) => p.kind === "loop")) return;
      const lines = (liveLogs[ag.id] || []).slice(-10).filter((l) => l.t === "err" || l.t === "cmd").map((l) => l.s);
      const counts = {}; let dup = null;
      lines.forEach((s) => { counts[s] = (counts[s] || 0) + 1; if (counts[s] >= 3) dup = s; });
      if (!dup) return;
      loopSeen.current[ag.id] = true;
      setAgents((prev) => prev.map((a) => a.id === ag.id
        ? { ...a, status: "waiting", pending: [...(a.pending || []), { id: "loop-" + ag.id, kind: "loop", title: "Looping — same command failed " + counts[dup] + "×", cmd: dup, when: "just now" }] } : a));
      flash("loop detected · " + ag.name + " paused before it burned the window");
    });
  }, [liveLogs]);

  // ---- native notifications (B6.4): only when the window isn't in front ----
  const prevStatus = useRef({});
  useEffect(() => {
    const away = win === "min" || win === "hidden";
    const next = {};
    const fired = [];
    agents.forEach((a) => {
      next[a.id] = a.status;
      const was = prevStatus.current[a.id];
      if (!was || was === a.status) return;
      const kind = a.status === "blocked" ? "question" : a.status === "done" ? "finished" : (a.pending || []).some((p) => p.kind === "permission") ? "permission" : null;
      if (!kind) return;
      if (!settings.osNotifications || !settings.events[kind]) return;
      if (!away) return;
      fired.push({ id: a.id + "-" + Date.now(), agentId: a.id, kind,
        title: kind === "finished" ? a.name + " finished" : kind === "question" ? a.name + " needs a decision" : a.name + " needs approval",
        body: kind === "finished" ? (a.commits || 0) + " commits on " + a.branch : (a.blockReason || a.task || "").slice(0, 90) });
    });
    prevStatus.current = next;
    if (fired.length) setNotifs((n) => [...fired, ...n].slice(0, 3));
  }, [agents, win, settings.osNotifications]);
  useEffect(() => {
    if (!notifs.length) return;
    const t = setTimeout(() => setNotifs((n) => n.slice(0, n.length - 1)), 6000);
    return () => clearTimeout(t);
  }, [notifs]);

  // ---- tab helpers ----
  const tabOf = (tabId) => tabs.find((tb) => tb.id === tabId);
  const tabAgentIds = (tb) => tb && tb.kind === "agent" ? window.treeAgentIds(tb.root) : [];
  const findAgentTab = (agentId) => tabs.find((tb) => tb.kind === "agent" && window.treeAgentIds(tb.root).includes(agentId));
  const newTabId = () => "tab" + (++tabCount.current);

  const selectTab = (tabId) => {
    setActiveTab(tabId);
    const tb = tabOf(tabId);
    if (tb && tb.kind === "agent") { const ids = window.treeAgentIds(tb.root); if (ids.length) setScopeAgentId(ids[0]); }
  };

  const openAgent = (id, pane) => {
    if (pane) paneHint.current[id] = pane;
    setScopeAgentId(id);
    setTabs((prev) => {
      const existing = prev.find((tb) => tb.kind === "agent" && window.treeAgentIds(tb.root).includes(id));
      if (existing) { setActiveTab(existing.id); return prev; }
      const tid = newTabId();
      setActiveTab(tid);
      return [...prev, { id: tid, kind: "agent", root: makeLeaf(id, pane || "terminal") }];
    });
  };

  const closeTab = (tabId) => {
    setTabs((prev) => prev.filter((x) => x.id !== tabId));
    setActiveTab((cur) => cur === tabId ? "orchestrator" : cur);
  };

  // set the root of the active (group) tab
  const setActiveRoot = (updater) => setTabs((prev) => prev.map((tb) =>
    tb.id === activeTab && tb.kind === "agent" ? { ...tb, root: typeof updater === "function" ? updater(tb.root) : updater } : tb));

  // merge dragged tab into target → grouped tab whose content tiles
  const mergeTabs = (srcId, dstId) => {
    if (srcId === dstId) return;
    setTabs((prev) => {
      const src = prev.find((t) => t.id === srcId), dst = prev.find((t) => t.id === dstId);
      if (!src || !dst || src.kind !== "agent" || dst.kind !== "agent") return prev;
      const root = { type: "split", id: "g" + Date.now(), dir: "v", ratio: 0.5, a: dst.root, b: src.root };
      return prev.filter((t) => t.id !== srcId).map((t) => t.id === dstId ? { ...t, root } : t);
    });
    setActiveTab(dstId);
    flash("grouped into tiled tab");
  };

  // add a sidebar agent into an existing tab as a new tiled terminal pane
  const addAgentToTab = (agentId, tabId) => {
    const tb = tabs.find((t) => t.id === tabId);
    const already = tb && tb.kind === "agent" && window.treeAgentIds(tb.root).includes(agentId);
    setScopeAgentId(agentId);
    if (tb && tb.kind === "agent" && !already) {
      setTabs((prev) => prev.map((x) => x.id === tabId && x.kind === "agent"
        ? { ...x, root: { type: "split", id: "g" + Date.now(), dir: "v", ratio: 0.5, a: x.root, b: makeLeaf(agentId, "terminal") } } : x));
    }
    setActiveTab(tabId);
    const ag = agents.find((a) => a.id === agentId);
    flash(already ? (ag ? ag.name : agentId) + " already in tab" : "added " + (ag ? ag.name : agentId) + " terminal");
  };

  const reorderTab = (srcId, dstId, before) => {
    setTabs((prev) => {
      const src = prev.find((t) => t.id === srcId); if (!src) return prev;
      const arr = prev.filter((t) => t.id !== srcId);
      const idx = arr.findIndex((t) => t.id === dstId);
      if (idx < 0) return prev;
      arr.splice(before ? idx : idx + 1, 0, src);
      return arr;
    });
  };

  const ungroupTab = (tabId) => {
    setTabs((prev) => {
      const tb = prev.find((t) => t.id === tabId); if (!tb || tb.kind !== "agent") return prev;
      const ids = window.treeAgentIds(tb.root); if (ids.length < 2) return prev;
      const made = ids.map((aid) => ({ id: newTabId(), kind: "agent", root: makeLeaf(aid, "terminal") }));
      const idx = prev.findIndex((t) => t.id === tabId);
      const copy = [...prev]; copy.splice(idx, 1, ...made);
      setActiveTab(made[0].id);
      return copy;
    });
    flash("ungrouped");
  };

  const detachAgent = (tabId, agentId) => {
    setTabs((prev) => {
      const tb = prev.find((t) => t.id === tabId); if (!tb) return prev;
      const ids = window.treeAgentIds(tb.root); if (!ids.includes(agentId) || ids.length < 2) return prev;
      const remaining = { ...tb, root: window.dropAgent(tb.root, agentId) };
      const nt = { id: newTabId(), kind: "agent", root: makeLeaf(agentId, "terminal") };
      const idx = prev.findIndex((t) => t.id === tabId);
      const copy = [...prev]; copy[idx] = remaining; copy.splice(idx + 1, 0, nt);
      setActiveTab(nt.id);
      return copy;
    });
  };

  const openFileInViewer = (agentId, path, line) => {
    setScopeAgentId(agentId);
    setOpenFile({ agentId, path });
    const ag = agents.find((a) => a.id === agentId);
    setRecentFiles((prev) => [{ path, agentId, agentName: ag ? ag.name : null }, ...prev.filter((r) => r.path !== path)].slice(0, 24));
    if (line) setJump({ path, line, col: 1, at: Date.now() });
    setTabs((prev) => {
      const single = prev.find((tb) => tb.kind === "agent" && window.countLeaves(tb.root) === 1 && window.treeAgentIds(tb.root)[0] === agentId);
      if (single) { setActiveTab(single.id); return prev; }
      const group = prev.find((tb) => tb.kind === "agent" && window.treeAgentIds(tb.root).includes(agentId));
      if (group) {
        setActiveTab(group.id);
        return prev.map((tb) => tb.id === group.id ? { ...tb, root: window.setLeafViewByAgent(tb.root, agentId, "file") } : tb);
      }
      const tid = newTabId();
      setActiveTab(tid);
      return [...prev, { id: tid, kind: "agent", root: makeLeaf(agentId, "diff") }];
    });
  };

  // open a git-inspection view (commit diff / range diff / blame target /
  // conflict resolution / file history) in the agent's center workspace
  const openGitView = (agentId, view) => {
    setScopeAgentId(agentId);
    setGitView(Object.assign({ agentId }, view));
    setTabs((prev) => {
      const single = prev.find((tb) => tb.kind === "agent" && window.countLeaves(tb.root) === 1 && window.treeAgentIds(tb.root)[0] === agentId);
      if (single) { setActiveTab(single.id); return prev; }
      const group = prev.find((tb) => tb.kind === "agent" && window.treeAgentIds(tb.root).includes(agentId));
      if (group) {
        setActiveTab(group.id);
        // force this agent's pane(s) to the diff view so the inspection never lands on the terminal
        return prev.map((tb) => tb.id === group.id ? { ...tb, root: window.setLeafViewByAgent(tb.root, agentId, "diff") } : tb);
      }
      const tid = newTabId();
      setActiveTab(tid);
      return [...prev, { id: tid, kind: "agent", root: makeLeaf(agentId, "diff") }];
    });
  };

  const act = (id, action) => {
    const ag = agents.find((a) => a.id === id);
    const nm = ag ? ag.name : id;
    const proj = ag ? projects.find((p) => p.id === ag.projectId) : null;
    setAgents((prev) => prev.map((a) => {
      if (a.id !== id) return a;
      if (action === "pause") return { ...a, status: "waiting" };
      if (action === "resume" || action === "start") return { ...a, status: "running" };
      if (action === "merge") return { ...a, status: "done", progress: 1 };
      if (action === "commit") return { ...a, commits: a.commits + 1, files: [] };
      if (action === "discard") return { ...a, files: [] };
      return a;
    }));
    const FLASH = { merge: "merged " + nm + " → " + (proj ? proj.branch : "main"), commit: "committed in " + nm, start: "started " + nm, resume: "resumed " + nm, pause: "paused " + nm, push: "pushed " + nm + " to origin", pr: "opened PR for " + nm, discard: "discarded changes in " + nm };
    if (FLASH[action]) flash(FLASH[action]);
    // lifecycle: when an attached agent merges, its ticket auto-moves to Done
    if (action === "merge") setTickets((prev) => prev.map((t) => (t.agentId === id && t.status !== "done") ? { ...t, status: "done" } : t));
    if (action === "merge") setCommits((c) => [{ agent: id, projectId: ag ? ag.projectId : null, sha: Math.random().toString(16).slice(2, 9), msg: "merge: " + nm + " into " + (proj ? proj.branch : "main"), when: "now", files: ag ? ag.files.length : 1 }, ...c]);
    if (action === "commit") setCommits((c) => [{ agent: id, projectId: ag ? ag.projectId : null, sha: Math.random().toString(16).slice(2, 9), msg: "wip: " + nm, when: "now", files: ag ? ag.files.length : 1 }, ...c]);
  };

  // send an inline-review batch to a live agent as one structured message
  const sendReview = (agentId, { comments, global }) => {
    const ag = agents.find((a) => a.id === agentId);
    const lines = [{ t: "sys", s: "▶ review feedback from you — " + comments.length + " comment" + (comments.length !== 1 ? "s" : "") }];
    if (global) lines.push({ t: "out", s: "[global] " + global });
    comments.forEach((c) => {
      const ref = c.file + ":" + (c.fromLine === c.toLine ? c.fromLine : c.fromLine + "-" + c.toLine);
      lines.push({ t: "out", s: ref + (c.block ? "  (block)" : "  `" + (c.snippet || "") + "`") });
      lines.push({ t: "out", s: "  → " + c.note });
    });
    setLiveLogs((p) => ({ ...p, [agentId]: [...(p[agentId] || []), ...lines] }));
    setAgents((prev) => prev.map((a) => (a.id === agentId && a.status !== "done") ? { ...a, status: "running" } : a));
    flash("sent " + comments.length + " review comment" + (comments.length !== 1 ? "s" : "") + " to " + (ag ? ag.name : agentId));
  };

  // inbox pending action (permission allow/deny, decision answer, review merge/diff)
  const handleInbox = (id, item, action) => {
    const ag = agents.find((a) => a.id === id);
    const drop = () => setAgents((prev) => prev.map((a) => a.id === id ? { ...a, pending: (a.pending || []).filter((p) => p.id !== item.id) } : a));
    if (action === "allow" || action === "always") {
      drop();
      setLiveLogs((p) => ({ ...p, [id]: [...(p[id] || []), { t: "sys", s: (action === "always" ? "✓ always allow · " : "✓ allowed · ") + item.cmd }, { t: "cmd", s: item.cmd }] }));
      flash((action === "always" ? "always allow · " : "allowed · ") + (ag ? ag.name : id));
    } else if (action === "deny") {
      drop();
      setLiveLogs((p) => ({ ...p, [id]: [...(p[id] || []), { t: "err", s: "✗ denied by user · " + item.cmd }] }));
      flash("denied · " + (ag ? ag.name : id));
    } else if (action === "merge") {
      drop(); act(id, "merge");
    } else if (action === "diff") {
      openAgent(id);
    } else if (item.kind === "loop") {
      drop();
      if (action === "hint") {
        setAgents((prev) => prev.map((a) => a.id === id ? { ...a, status: "running" } : a));
        setLiveLogs((p) => ({ ...p, [id]: [...(p[id] || []), { t: "sys", s: "▶ human hint: the command keeps failing — read the failure before retrying it" }] }));
        flash("sent a hint to " + (ag ? ag.name : id));
      } else if (action === "continue") {
        setAgents((prev) => prev.map((a) => a.id === id ? { ...a, status: "running" } : a));
        flash((ag ? ag.name : id) + " continuing");
      } else {
        openAgent(id, "terminal");
        flash("inspecting " + (ag ? ag.name : id));
      }
    } else if (item.kind === "decision") {
      // `action` is the chosen option label
      drop();
      setAgents((prev) => prev.map((a) => a.id === id ? { ...a, status: "running" } : a));
      streamIdx.current[id] = 0;
      setLiveLogs((p) => ({ ...p, [id]: [...(p[id] || []), { t: "sys", s: "▶ resumed — decision: " + action }, { t: "cmd", s: "continuing task…" }] }));
      flash((ag ? ag.name : id) + " · " + action);
    }
  };

  const spawn = ({ projectId, branch, toolId, model, effort, prompt, name: nameIn, ticketId, open = true }) => {
    spawnCount.current += 1;
    const name = (nameIn && nameIn.trim()) || NAMES[(spawnCount.current - 1) % NAMES.length] + (spawnCount.current > NAMES.length ? "-" + spawnCount.current : "");
    const id = "s" + spawnCount.current;
    const proj = projects.find((p) => p.id === projectId);
    const ag = { id, projectId, tool: toolId, model, effort, name, task: prompt, status: "running", branch: "agent/" + name, worktree: proj.id + "-" + id, base: proj.head, commits: 0, elapsed: 0, progress: 0.02, files: [], pending: [] };
    setAgents((prev) => [...prev, ag]);
    // link the ticket → moves it to In progress and attaches this agent
    if (ticketId) setTickets((prev) => prev.map((t) => t.id === ticketId ? { ...t, status: "inprogress", agentId: id } : t));
    LOGS[id] = [
      { t: "sys", s: "allocating worktree → " + WORKTREE_ROOT + "/" + proj.id + "-" + id + " (" + toolMeta(toolId).name + ")" },
      { t: "cmd", s: "git worktree add ./" + proj.id + "-" + id + " -b agent/" + name + " " + branch },
      { t: "ok", s: "Preparing worktree (new branch 'agent/" + name + "' from " + branch + ")" },
      { t: "cmd", s: "cd " + proj.path + " && pnpm install" },
    ];
    STREAM[id] = [
      { t: "ok", s: "dependencies ready in 1.4s" },
      { t: "sys", s: "analyzing task: " + prompt.slice(0, 48) + (prompt.length > 48 ? "…" : "") },
      { t: "out", s: "scanning repository for relevant modules…" },
      { t: "cmd", s: "rg -l --type ts" },
      { t: "out", s: "drafting implementation plan (3 steps)" },
    ];
    streamIdx.current[id] = 0;
    setLiveLogs((p) => ({ ...p, [id]: LOGS[id].slice() }));
    setSpawning(false);
    if (open) {
      openAgent(id, "terminal");
      flash("spawned " + name + " in " + proj.name);
    } else {
      setScopeAgentId(id);
      flash("created " + name + " in " + proj.name + " — no terminal opened");
    }
  };

  const duplicateAgent = (src) => {
    spawn({ projectId: src.projectId, branch: src.base ? "main" : "main", toolId: src.tool, model: src.model, effort: src.effort, prompt: src.task });
  };

  // ----- tickets / backlog -----
  const openTicket = (id) => {
    const tid = id === "new" ? "ticket-new" : "ticket-" + id;
    setTabs((prev) => prev.find((t) => t.id === tid) ? prev : [...prev, { id: tid, kind: "ticket", ticketId: id }]);
    setActiveTab(tid);
  };
  const createTicket = ({ title, notes, projectId, tags }) => {
    ticketCount.current += 1;
    const id = "t" + ticketCount.current;
    setTickets((prev) => [{ id, status: "todo", projectId: projectId || null, title, notes: notes || "", tags: tags || [], created: "just now", comments: [] }, ...prev]);
    setTabs((prev) => prev.map((t) => t.id === "ticket-new" ? { id: "ticket-" + id, kind: "ticket", ticketId: id } : t));
    setActiveTab("ticket-" + id);
    flash("created ticket · " + title);
  };
  const updateTicket = (id, patch) => { setTickets((prev) => prev.map((t) => t.id === id ? { ...t, ...patch } : t)); flash("saved ticket"); };
  const moveTicket = (id, status) => {
    setTickets((prev) => prev.map((t) => t.id === id ? { ...t, status } : t));
    flash("moved ticket → " + ({ todo: "To do", inprogress: "In progress", done: "Done" }[status] || status));
  };
  const deleteTicket = (id) => {
    setTickets((prev) => prev.filter((t) => t.id !== id));
    setTabs((prev) => prev.filter((t) => !(t.kind === "ticket" && t.ticketId === id)));
    setActiveTab((cur) => cur === "ticket-" + id ? "backlog" : cur);
    flash("deleted ticket");
  };
  const addComment = (id, { body }) => {
    commentCount.current += 1;
    setTickets((prev) => prev.map((t) => t.id === id ? { ...t, comments: [...(t.comments || []), { id: "uc" + commentCount.current, author: "You", role: "human", when: "just now", body }] } : t));
  };
  const dispatchTicket = (ticket) => setSpawning({ project: ticket.projectId, ticketId: ticket.id });

  const removeAgent = (id) => {
    const ag = agents.find((a) => a.id === id);
    setAgents((prev) => prev.filter((a) => a.id !== id));
    setTabs((prev) => prev.map((tb) => {
      if (tb.kind !== "agent") return tb;
      const ids = window.treeAgentIds(tb.root);
      if (!ids.includes(id)) return tb;
      if (ids.length === 1) return null;
      return { ...tb, root: window.dropAgent(tb.root, id) };
    }).filter(Boolean));
    setActiveTab((cur) => tabs.find((tb) => tb.id === cur && tb.kind === "agent" && window.treeAgentIds(tb.root).includes(id) && window.treeAgentIds(tb.root).length === 1) ? "orchestrator" : cur);
    flash("removed worktree " + (ag ? ag.name : id));
  };

  const removeProject = (id) => {
    const p = projects.find((x) => x.id === id);
    setProjects((prev) => prev.filter((x) => x.id !== id));
    setAgents((prev) => prev.filter((a) => a.projectId !== id));
    flash("removed project " + (p ? p.name : id));
  };

  const addProject = ({ path, name, icon, color, gitInit }) => {
    projCount.current += 1;
    const id = "pn" + projCount.current;
    const head = gitInit ? "0000000" : Math.random().toString(16).slice(2, 9);
    const proj = { id, name, org: ORG, icon, color, path, branch: "main", head, hasGit: true, branches: ["main"], files: [] };
    setProjects((prev) => [...prev, proj]);
    setAddingProject(false);
    flash(gitInit ? "initialized git + added " + name : "added project " + name);
  };

  // ----- context menus -----
  const openMenu = (e, items) => { e.preventDefault(); e.stopPropagation(); setMenu({ x: e.clientX, y: e.clientY, items }); };

  const agentMenu = (id) => {
    const ag = agents.find((a) => a.id === id); if (!ag) return [];
    const proj = projects.find((p) => p.id === ag.projectId);
    const branchTarget = proj ? proj.branch : "main";
    return [
      { label: "Open workspace", icon: "enter", onClick: () => openAgent(id) },
      { label: "Open terminal", icon: "terminal", onClick: () => openAgent(id, "terminal") },
      { label: "View diff", icon: "diff", onClick: () => openAgent(id, "diff") },
      { sep: true },
      ag.status === "running"
        ? { label: "Pause agent", icon: "pause", onClick: () => act(id, "pause") }
        : { label: "Resume agent", icon: "play", disabled: ag.status === "done", onClick: () => act(id, "resume") },
      { label: "Commit changes", icon: "commit", disabled: !ag.files.length, onClick: () => act(id, "commit") },
      { label: "Push to origin", icon: "push", disabled: !ag.commits, onClick: () => act(id, "push") },
      { label: "Open pull request", icon: "pr", disabled: !ag.commits, onClick: () => act(id, "pr") },
      { label: "Merge → " + branchTarget, icon: "merge", accent: "var(--st-done)", disabled: !ag.commits, onClick: () => act(id, "merge") },
      { sep: true },
      { label: "Rename branch", icon: "rename", onClick: () => flash("rename " + ag.branch) },
      { label: "Duplicate agent", icon: "dup", onClick: () => duplicateAgent(ag) },
      { sep: true },
      { label: "Discard changes", icon: "discard", disabled: !ag.files.length, onClick: () => act(id, "discard") },
      { label: "Delete worktree", icon: "trash", danger: true, onClick: () => removeAgent(id) },
    ];
  };

  const projectMenu = (id) => {
    const p = projects.find((x) => x.id === id); if (!p) return [];
    return [
      { label: "Spawn agent here", icon: "bolt", accent: "var(--accent)", onClick: () => setSpawning({ project: id }) },
      { sep: true },
      { label: "Pull latest", icon: "refresh", onClick: () => flash("pulled " + p.name) },
      { label: "Open in terminal", icon: "terminal", onClick: () => flash("opened terminal · " + p.path) },
      { label: "Copy path", icon: "dup", onClick: () => flash(p.path) },
      { sep: true },
      { label: "Remove project", icon: "trash", danger: true, onClick: () => removeProject(id) },
    ];
  };

  const tabMenu = (tab) => {
    if (tab.kind !== "agent") return [];
    const ids = window.treeAgentIds(tab.root);
    if (ids.length <= 1) return agentMenu(ids[0]);
    // group tab menu
    const items = [];
    ids.forEach((aid) => {
      const a = agents.find((x) => x.id === aid);
      if (a) items.push({ label: "Detach " + a.name, icon: "enter", onClick: () => detachAgent(tab.id, aid) });
    });
    items.push({ sep: true });
    items.push({ label: "Ungroup all", icon: "columns", accent: "var(--accent)", onClick: () => ungroupTab(tab.id) });
    items.push({ label: "Close group", icon: "x", danger: true, onClick: () => closeTab(tab.id) });
    return items;
  };

  const activeTabObj = tabs.find((tb) => tb.id === activeTab);
  const activeIds = activeTabObj && activeTabObj.kind === "agent" ? window.treeAgentIds(activeTabObj.root) : [];
  const isGroup = activeIds.length > 1;
  const soloAgent = activeTabObj && activeTabObj.kind === "agent" && activeIds.length === 1 ? agents.find((a) => a.id === activeIds[0]) : null;
  const scopeAgent = activeTabObj && activeTabObj.kind === "agent"
    ? (agents.find((a) => a.id === scopeAgentId && activeIds.includes(a.id)) || agents.find((a) => a.id === activeIds[0]))
    : null;
  const sidebarActive = scopeAgent ? scopeAgent.id : null;
  const runningCount = agents.filter((a) => a.status === "running").length;
  const navView = (!activeTabObj || activeTabObj.kind === "orchestrator") ? "orchestrator" : (activeTabObj.kind === "backlog" || activeTabObj.kind === "ticket") ? "backlog" : "agent";
  const openTicketCount = tickets.filter((tk) => tk.status !== "done").length;
  const allTags = allTagsOf(tickets);

  // ---- navigation stack (B2.6): every location change is recorded so back /
  // forward walk files, diffs, commits and tabs alike ----
  const navRef = useRef({ stack: [], i: -1, lock: false });
  const [navVer, setNavVer] = useState(0);
  useEffect(() => {
    const n = navRef.current;
    if (n.lock) { n.lock = false; return; }
    const loc = { tab: activeTab, file: openFile, git: gitView, scope: scopeAgentId };
    const cur = n.stack[n.i];
    const key = (l) => l && [l.tab, l.file && l.file.path, l.git && (l.git.kind + (l.git.sha || l.git.path || ""))].join("|");
    if (cur && key(cur) === key(loc)) return;
    n.stack = n.stack.slice(0, n.i + 1).concat([loc]).slice(-60);
    n.i = n.stack.length - 1;
    setNavVer((v) => v + 1);
  }, [activeTab, openFile, gitView, scopeAgentId]);
  const applyLoc = (loc) => {
    navRef.current.lock = true;
    setActiveTab(loc.tab); setOpenFile(loc.file); setGitView(loc.git); setScopeAgentId(loc.scope);
  };
  const navBack = () => { const n = navRef.current; if (n.i <= 0) return; n.i -= 1; applyLoc(n.stack[n.i]); setNavVer((v) => v + 1); };
  const navForward = () => { const n = navRef.current; if (n.i >= n.stack.length - 1) return; n.i += 1; applyLoc(n.stack[n.i]); setNavVer((v) => v + 1); };

  // ---- command-layer context: state + every handler the registry can invoke ----
  const scopeFileAgent = scopeAgent || agents[0] || null;
  const appendLog = (id, lines) => setLiveLogs((p) => ({ ...p, [id]: [...(p[id] || []), ...lines] }));
  const sendPrompt = (id, text) => {
    const ag = agents.find((a) => a.id === id);
    appendLog(id, [{ t: "cmd", s: text }, { t: "sys", s: "▶ message delivered — agent resumed" }]);
    setAgents((prev) => prev.map((a) => a.id === id && a.status !== "done" ? { ...a, status: "running" } : a));
    flash("sent to " + (ag ? ag.name : id));
  };
  const openFileAgent = openFile ? agents.find((a) => a.id === openFile.agentId) : null;
  const openFileLines = openFile ? (window.getFileLines(openFile.path, openFileAgent || {}) || []).length : 0;
  const toggleSleep = (id) => {
    if (!id) return;
    const ag = agents.find((a) => a.id === id);
    setAsleep((m) => { const nx = { ...m }; if (nx[id]) delete nx[id]; else nx[id] = true; return nx; });
    flash(asleep[id] ? "woke " + (ag ? ag.name : id) + " — replaying scrollback ring" : "slept " + (ag ? ag.name : id) + " — freed ~6MB terminal");
  };
  const sleepIdleAgents = () => {
    const targets = agents.filter((a) => a.status !== "running" && !activeIds.includes(a.id) && !asleep[a.id]);
    if (!targets.length) return flash("nothing to sleep — every agent is running or on screen");
    setAsleep((m) => { const nx = { ...m }; targets.forEach((a) => { nx[a.id] = true; }); return nx; });
    flash("slept " + targets.length + " idle agent" + (targets.length === 1 ? "" : "s") + " · freed ~" + targets.length * 6 + " MB");
  };
  const gitAction = (kind) => {
    const id = scopeFileAgent && scopeFileAgent.id;
    if (!id) return flash("no agent in scope");
    if (["commit", "push", "merge", "pause", "resume", "discard", "pr"].includes(kind)) return act(id, kind);
    if (window.GIT_OPS && window.GIT_OPS[kind]) return window.GIT_OPS[kind](id);
    flash(kind + " · " + (scopeFileAgent.name || id));
  };
  const cmdCtx = {
    agents, projects, tickets, running, keymap, autosave, scopeAgent: scopeFileAgent, openFile, openFileLines,
    visibleAgentIds: activeIds,
    recentFiles, canBack: navRef.current.i > 0, canForward: navRef.current.i < navRef.current.stack.length - 1,
    hasConflict: !!(scopeFileAgent && window.AGENT_GIT && window.AGENT_GIT.agentHasConflict(scopeFileAgent)),
    canCloseTab: !!(activeTabObj && activeTabObj.kind !== "orchestrator"),
    flash, navBack, navForward,
    openOverlay: (kind, tab) => setOverlay({ kind, tab }),
    goTab: (k) => selectTab(k),
    closeActiveTab: () => activeTabObj && activeTabObj.kind !== "orchestrator" && closeTab(activeTabObj.id),
    openFileAt: (path, line) => openFileInViewer((scopeFileAgent && scopeFileAgent.id) || "a1", path, line),
    openAgent, openTicket,
    openCommit: (sha) => openGitView((scopeFileAgent && scopeFileAgent.id) || "a1", { kind: "commit", sha }),
    openRange: (shas) => openGitView((scopeFileAgent && scopeFileAgent.id) || "a1", { kind: "range", shas }),
    appendLog,
    goToLine: (line, col) => { if (openFile) { setJump({ path: openFile.path, line, col, at: Date.now() }); flash("→ " + fileName(openFile.path) + ":" + line); } },
    gitAction, isAsleep: (id) => !!asleep[id], toggleSleep,
    spawnAgent: () => setSpawning({ project: scopeFileAgent ? scopeFileAgent.projectId : null }),
    newTicket: () => openTicket("new"),
    deleteAgent: (id) => id && removeAgent(id),
    toggleRunAll: () => { setRunning((r) => !r); flash(running ? "paused all agents" : "resumed all agents"); },
    toggleTheme: () => setTweak("theme", t.theme === "dark" ? "light" : "dark"),
    toggleSidebar: () => setCompact((v) => !v),
    toggleRightPanel: () => setTweak("rightPanel", !t.rightPanel),
    toggleStructure: () => { setStructure((v) => !v); flash(structure ? "structure view hidden" : "structure view shown"); },
    toggleAutosave: () => { setAutosave((v) => !v); flash(autosave ? "autosave off" : "autosave on · saves 2s after you stop typing"); },
    toggleBlame: () => { window.dispatchEvent(new CustomEvent("orrery:blame")); },
    saveFile: () => window.dispatchEvent(new CustomEvent("orrery:save")),
    revertHunk: () => window.dispatchEvent(new CustomEvent("orrery:revert-hunk")),
    searchScrollback: () => window.dispatchEvent(new CustomEvent("orrery:scrollback-search")),
    splitTerminal: (dir) => { if (activeTabObj && activeTabObj.kind === "agent") { window.dispatchEvent(new CustomEvent("orrery:split", { detail: { dir } })); } else flash("open an agent tab first"); },
    openSearchPanel: (mode) => setPanelTool({ kind: "search", mode }),
    openBranches: () => setPanelTool({ kind: "branches" }),
    openCommitGraph: () => setPanelTool({ kind: "graph" }),
    openLocalHistory: () => setPanelTool({ kind: "history" }),
    openConflicts: () => scopeFileAgent && openGitView(scopeFileAgent.id, { kind: "conflict" }),
    openPerf: () => window.dispatchEvent(new CustomEvent("orrery:devpanel", { detail: { tab: "procs" } })),
    openCost: () => setPanelTool({ kind: "cost" }),
    openGithub: () => setPanelTool({ kind: "github" }),
    openSettings: () => setSettingsOpen(true),
    openKeymap: () => setOverlay({ kind: "keymap" }),
    setKeymap: (k) => { setKeymap(k); flash("keymap → " + (k === "intellij" ? "IntelliJ IDEA" : "VS Code")); },
    showWhatsNew,
  };

  return (
    <React.Fragment>
      <div className={"os win-" + win}>
        <div className="desktop" />
        <div className="appwin">
          <div className="bg-texture" />
          <div className="bg-glow" />
          <div className="shell">
            <TopBar tabs={tabs} activeTab={activeTab} onTab={selectTab} onCloseTab={closeTab}
              nav={{ canBack: cmdCtx.canBack, canForward: cmdCtx.canForward, onBack: navBack, onForward: navForward, keymap }}
              onSearchEverywhere={() => setOverlay({ kind: "search" })}
              onTabContext={(e, tab) => openMenu(e, tabMenu(tab))}
              agents={agents} projects={projects} theme={t.theme} tickets={tickets}
              onTheme={() => setTweak("theme", t.theme === "dark" ? "light" : "dark")}
              onMergeTabs={mergeTabs} onReorderTab={reorderTab} onAddAgent={addAgentToTab}
              win={win} onHide={hideWin} onMinimize={minimizeWin} onMaximize={toggleMax} onClose={closeWin}
              build={build} compact={compact} rightPanel={t.rightPanel}
              onSettings={() => setSettingsOpen(true)} onShowWhatsNew={showWhatsNew}
              running={running} onRunAll={() => { setRunning((r) => !r); flash(running ? "paused all agents" : "resumed all agents"); }} />

        <div className={"workspace" + (t.rightPanel ? "" : " no-right") + (compact ? " compact" : "")}>
          {compact
            ? <CompactRail projects={projects} agents={agents} activeAgent={sidebarActive} onOpen={openAgent} loading={loading}
                onContext={(e, id) => openMenu(e, agentMenu(id))} onProjectContext={(e, id) => openMenu(e, projectMenu(id))}
                view={navView} onNav={(k) => setActiveTab(k)} openTicketCount={openTicketCount}
                onSpawn={(pid) => setSpawning({ project: pid })} onAddProject={() => setAddingProject(true)} onExpand={() => setCompact(false)} />
            : <Sidebar projects={projects} agents={agents} activeAgent={sidebarActive} onOpen={openAgent} loading={loading} onReload={reloadData}
                onContext={(e, id) => openMenu(e, agentMenu(id))}
                onProjectContext={(e, id) => openMenu(e, projectMenu(id))}
                onSpawn={(pid) => setSpawning({ project: pid })} onAddProject={() => setAddingProject(true)}
                view={navView} onNav={(k) => setActiveTab(k)} openTicketCount={openTicketCount}
                onToggleCompact={() => setCompact(true)} query={query} setQuery={setQuery} />}

          <div style={{ display: "grid", gridTemplateRows: panelTool ? "minmax(0,1fr) auto" : "minmax(0,1fr)", minWidth: 0, minHeight: 0, overflow: "hidden" }}>
          {!activeTabObj || activeTabObj.kind === "orchestrator"
            ? <Overview agents={agents} projects={projects} viz={viz} setViz={setViz} loading={loading} onOpen={openAgent} onAct={act} onSpawn={() => setSpawning({ project: null })} asleep={asleep} onSleep={(id) => toggleSleep(id)} />
            : activeTabObj.kind === "backlog"
            ? <BacklogView tickets={tickets} agents={agents} projects={projects} allTags={allTags} onOpen={openTicket} onNew={() => openTicket("new")} onDispatch={dispatchTicket} onOpenAgent={openAgent} onMove={moveTicket} />
            : activeTabObj.kind === "ticket"
            ? (() => {
                const draft = activeTabObj.ticketId === "new";
                const tk = draft ? { id: "new", status: "todo", projectId: null, title: "", notes: "", tags: [], comments: [] } : tickets.find((t) => t.id === activeTabObj.ticketId);
                return tk
                  ? <TicketPage key={activeTabObj.id} ticket={tk} draft={draft} agents={agents} projects={projects} allTags={allTags}
                      onCreate={createTicket} onUpdate={updateTicket} onDelete={deleteTicket} onDispatch={dispatchTicket}
                      onOpenAgent={openAgent} onMove={moveTicket} onAddComment={addComment} onClose={() => closeTab(activeTabObj.id)} />
                  : <div style={{ display: "grid", placeItems: "center", color: "var(--ink-4)" }}>ticket not found</div>;
              })()
            : isGroup
            ? <PaneManager key={activeTab} root={activeTabObj.root} setRoot={setActiveRoot} agents={agents} projects={projects} liveLogs={liveLogs} onFocusAgent={setScopeAgentId}
                onAct={act} onFlash={flash} gitView={gitView} openFile={openFile} onOpenGit={openGitView} onCloseGitView={() => setGitView(null)} onCloseFile={() => setOpenFile(null)}
                onAppendLog={appendLog} onSendPrompt={sendPrompt} autosave={autosave} jump={jump} />
            : soloAgent
            ? <Workspace key={soloAgent.id} ag={soloAgent} project={projects.find((p) => p.id === soloAgent.projectId)} liveLogs={liveLogs[soloAgent.id] || []} onAct={act} onFlash={flash} onAppendLog={appendLog} initialPane={paneHint.current[soloAgent.id]} openFile={openFile && openFile.agentId === soloAgent.id ? openFile.path : null} onCloseFile={() => setOpenFile(null)} gitView={gitView && gitView.agentId === soloAgent.id ? gitView : null} onCloseGitView={() => setGitView(null)} onOpenGit={openGitView} onSendReview={sendReview} asleep={!!asleep[soloAgent.id]} onWake={toggleSleep} structureOpen={structure} autosave={autosave} jump={jump} onSendPrompt={sendPrompt} />
            : <div style={{ display: "grid", placeItems: "center", color: "var(--ink-4)" }}>empty tab</div>}
          {panelTool && <ToolWindow tool={panelTool} ctx={cmdCtx} onSelect={setPanelTool} onClose={() => setPanelTool(null)} />}
          </div>

          {t.rightPanel && <RightPanel agents={agents} scopeAgent={scopeAgent} onOpen={openAgent} onResolve={handleInbox} onAct={act} commits={commits} onOpenFile={openFileInViewer} openFile={openFile} onOpenGit={openGitView} />}
        </div>

        <StatusBar agents={agents} projects={projects} toast={toast} build={build} onShowWhatsNew={showWhatsNew} onOpenLog={openLogFile}
          scopeAgent={scopeFileAgent} asleep={asleep} onOpenCost={() => setPanelTool({ kind: "cost" })}
          onOpenPerf={() => window.dispatchEvent(new CustomEvent("orrery:devpanel", { detail: { tab: "procs" } }))} />
          </div>
        </div>

        {/* minimized → taskbar pill */}
        {win === "min" && (
          <button className="os-float rise" onClick={showWin}
            style={{ left: 16, bottom: 16, padding: "8px 14px 8px 10px" }}>
            <Logo size={18} />
            <span style={{ fontFamily: "var(--font-disp)", fontWeight: 600, fontSize: 13 }}>
              <span style={{ color: "var(--accent)" }}>O</span>rrery
            </span>
            <span className="dot running" style={{ background: "var(--st-running)", marginLeft: 2 }} />
            <span style={{ fontSize: 10, color: "var(--ink-3)" }}>{runningCount} running</span>
          </button>
        )}

        {/* native notifications — shown while the window is minimized or in the tray */}
        {notifs.length > 0 && (
          <div style={{ position: "absolute", top: 16, right: 16, zIndex: 60, display: "flex", flexDirection: "column", gap: 8, width: 300 }}>
            {notifs.map((n) => (
              <div key={n.id} className="surface rise" style={{ padding: 11, boxShadow: "var(--shadow)", background: "var(--elev)", display: "flex", gap: 10 }}>
                <Logo size={22} />
                <div style={{ flex: 1, minWidth: 0 }}>
                  <div style={{ display: "flex", alignItems: "baseline", gap: 6 }}>
                    <span style={{ fontSize: 11.5, fontWeight: 600, color: "var(--ink)" }}>{n.title}</span>
                    <span style={{ marginLeft: "auto", fontSize: 9, color: "var(--ink-4)" }}>Orrery</span>
                  </div>
                  <div style={{ fontSize: 10.5, color: "var(--ink-3)", marginTop: 4, lineHeight: 1.45 }}>{n.body}</div>
                  <div style={{ display: "flex", gap: 6, marginTop: 8 }}>
                    <button className="btn primary" style={{ padding: "3px 9px", fontSize: 10.5 }}
                      onClick={() => { setNotifs((x) => x.filter((y) => y.id !== n.id)); showWin(); openAgent(n.agentId); }}>Open</button>
                    <button className="btn ghost-hair" style={{ padding: "3px 9px", fontSize: 10.5 }}
                      onClick={() => setNotifs((x) => x.filter((y) => y.id !== n.id))}>Dismiss</button>
                  </div>
                </div>
              </div>
            ))}
          </div>
        )}

        {/* hidden → tray chip */}
        {win === "hidden" && (
          <button className="os-float rise" onClick={showWin}
            style={{ top: 18, left: "50%", transform: "translateX(-50%)", borderRadius: 999, padding: "8px 15px", color: "var(--ink-2)", fontSize: 11 }}>
            <Logo size={16} />
            <span>Orrery is hidden in the tray</span>
            <span style={{ color: "var(--ink-4)" }}>·</span>
            <span style={{ color: "var(--accent-2)" }}>click to show</span>
          </button>
        )}

        {/* closed → relaunch card */}
        {win === "closed" && (
          <div style={{ position: "absolute", inset: 0, zIndex: 55, display: "grid", placeItems: "center" }}>
            <div className="surface rise" style={{ padding: "32px 40px", textAlign: "center", display: "flex", flexDirection: "column", alignItems: "center", gap: 16, boxShadow: "var(--shadow)" }}>
              <Logo size={50} />
              <div>
                <div style={{ fontFamily: "var(--font-disp)", fontWeight: 600, fontSize: 19 }}>
                  <span style={{ color: "var(--accent)" }}>O</span>rrery
                </div>
                <div style={{ fontSize: 11, color: "var(--ink-3)", marginTop: 5 }}>
                  Session ended · {agents.length} worktrees preserved on disk
                </div>
              </div>
              <div style={{ display: "flex", gap: 9 }}>
                <button className="btn primary" onClick={relaunch}><Icon name="play" size="sm" />Relaunch Orrery</button>
                <button className="btn ghost-hair" onClick={showWin}><Icon name="enter" size="sm" />Reopen window</button>
              </div>
            </div>
          </div>
        )}
      </div>

      {spawning && <SpawnModal projects={projects} tickets={tickets} defaultProject={spawning.project} defaultTicket={spawning.ticketId} onClose={() => setSpawning(false)} onSpawn={spawn} />}
      {addingProject && <AddProjectModal onClose={() => setAddingProject(false)} onAdd={addProject} />}
      {menu && <ContextMenu x={menu.x} y={menu.y} items={menu.items} onClose={() => setMenu(null)} />}

      {settingsOpen && <SettingsModal settings={settings} setSettings={setSettings} onClose={() => setSettingsOpen(false)}
        projects={projects} agents={agents} onOpenKeymap={() => { setSettingsOpen(false); setOverlay({ kind: "keymap" }); }} onSleepIdle={sleepIdleAgents}
        onOpenLog={openLogFile} onShowWhatsNew={showWhatsNew} onInstallUpdate={startInstall} />}

      {update.phase === "available" && installedVersion == null &&
        <UpdateToast fromVersion={BUILD.version} toVersion={updTo} size="18.4 MB"
          onInstall={startInstall} onLater={() => setUpdate({ phase: "idle" })} />}
      {update.phase === "installing" && <UpdaterBanner toVersion={updTo} onDone={finishInstall} />}
      {whatsNew && <WhatsNewModal releases={wnReleasesSince(BUILD.version)} fromVersion={BUILD.version} onClose={() => setWhatsNew(false)} />}

      <DevConsole agents={agents} projects={projects} channel={build.channel} loading={loading} ctx={cmdCtx} />

      <CommandCenter ctx={cmdCtx} overlay={overlay} setOverlay={setOverlay} />

      <TweaksPanel>
        <TweakSection label="Theme" />
        <TweakRadio label="Mode" value={t.theme} options={["dark", "light"]} onChange={(v) => setTweak("theme", v)} />
        <TweakColor label="Accent palette" value={t.palette} options={Object.values(PALETTES)} onChange={(v) => setTweak("palette", v)} />
        <TweakSection label="Layout" />
        <TweakRadio label="Density" value={t.density} options={["compact", "regular", "comfy"]} onChange={(v) => setTweak("density", v)} />
        <TweakToggle label="Compact sidebar" value={compact} onChange={setCompact} />
        <TweakToggle label="Right panel" value={t.rightPanel} onChange={(v) => setTweak("rightPanel", v)} />
        <TweakSection label="Build" />
        <TweakRadio label="Channel" value={build.channel} options={["dev", "prod"]} onChange={(v) => setTweak("channel", v)} />
        <TweakToggle label="Simulate updates" value={t.simulateUpdates} onChange={(v) => setTweak("simulateUpdates", v)} />
        <div style={{ display: "flex", gap: 6, marginTop: 2 }}>
          <TweakButton label="Replay update" onClick={replayUpdate} />
          <TweakButton label="What’s new" secondary onClick={showWhatsNew} />
        </div>
        <TweakSection label="Orchestrator" />
        <TweakSelect label="Agent visualization" value={t.defaultViz} options={["grid", "kanban", "graph", "timeline"]} onChange={(v) => setTweak("defaultViz", v)} />
        <TweakToggle label="Live motion" value={t.motion} onChange={(v) => setTweak("motion", v)} />
      </TweaksPanel>
    </React.Fragment>
  );
}

ReactDOM.createRoot(document.getElementById("root")).render(<App />);
