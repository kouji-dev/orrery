/* global React, Icon, StatusDot, ToolBadge, toolMeta, Terminal, DiffView, FileViewer, AgentGitView, fileName, projectOf */
// ORCHESTRA — split / tiling pane manager for the central workspace

const { useState: useStateP, useRef: useRefP, useEffect: useEffectP, useCallback: useCB } = React;

let _pid = 0;
const nid = () => "pane" + (++_pid);
const leaf = (agentId, view) => ({ type: "leaf", id: nid(), agentId: agentId || null, view: view || "terminal" });

// ---- immutable tree ops ----
function splitLeaf(node, id, dir, nl) {
  if (node.type === "leaf") return node.id === id ? { type: "split", id: nid(), dir, ratio: 0.5, a: node, b: nl } : node;
  return { ...node, a: splitLeaf(node.a, id, dir, nl), b: splitLeaf(node.b, id, dir, nl) };
}
function removeLeaf(node, id) {
  if (node.type === "leaf") return node;
  const a = node.a, b = node.b;
  if (a.type === "leaf" && a.id === id) return b;
  if (b.type === "leaf" && b.id === id) return a;
  return { ...node, a: removeLeaf(a, id), b: removeLeaf(b, id) };
}
function patchLeaf(node, id, patch) {
  if (node.type === "leaf") return node.id === id ? { ...node, ...patch } : node;
  return { ...node, a: patchLeaf(node.a, id, patch), b: patchLeaf(node.b, id, patch) };
}
// force every leaf bound to an agent onto a given view (used when a commit is
// picked in the right panel → its pane must switch to diff, never the terminal)
function setLeafViewByAgent(node, agentId, view) {
  if (node.type === "leaf") return node.agentId === agentId ? { ...node, view } : node;
  return { ...node, a: setLeafViewByAgent(node.a, agentId, view), b: setLeafViewByAgent(node.b, agentId, view) };
}
function setRatio(node, id, ratio) {
  if (node.type === "leaf") return node;
  if (node.id === id) return { ...node, ratio };
  return { ...node, a: setRatio(node.a, id, ratio), b: setRatio(node.b, id, ratio) };
}
function countLeaves(node) { return node.type === "leaf" ? 1 : countLeaves(node.a) + countLeaves(node.b); }
function treeAgentIds(node) { const out = []; (function w(n) { if (n.type === "leaf") { if (n.agentId) out.push(n.agentId); } else { w(n.a); w(n.b); } })(node); return out; }
function leafAgentOf(node, paneId) { let r = null; (function w(n) { if (n.type === "leaf") { if (n.id === paneId) r = n.agentId; } else { w(n.a); w(n.b); } })(node); return r; }
function dropAgent(node, agentId) {
  if (node.type === "leaf") return node;
  const a = node.a, b = node.b;
  if (a.type === "leaf" && a.agentId === agentId) return b;
  if (b.type === "leaf" && b.agentId === agentId) return a;
  return { ...node, a: dropAgent(a, agentId), b: dropAgent(b, agentId) };
}
// split a specific leaf toward a side, inserting a new leaf (or replace on center)
function splitLeafSide(node, id, side, nl) {
  if (node.type === "leaf") {
    if (node.id !== id) return node;
    if (side === "center") return nl;
    const dir = (side === "left" || side === "right") ? "v" : "h";
    const first = (side === "left" || side === "top");
    return { type: "split", id: nid(), dir, ratio: 0.5, a: first ? nl : node, b: first ? node : nl };
  }
  return { ...node, a: splitLeafSide(node.a, id, side, nl), b: splitLeafSide(node.b, id, side, nl) };
}

// ---- agent picker popover ----
function AgentPicker({ projects, agents, value, onPick, onClose }) {
  const ref = useRefP(null);
  useEffectP(() => {
    const h = (e) => { if (ref.current && !ref.current.contains(e.target)) onClose(); };
    document.addEventListener("mousedown", h);
    return () => document.removeEventListener("mousedown", h);
  }, []);
  return (
    <div ref={ref} className="rise" style={{ position: "absolute", top: "calc(100% + 4px)", left: 6, zIndex: 40, width: 230,
      background: "var(--elev)", border: "1px solid var(--hair-2)", borderRadius: "var(--r-md)", boxShadow: "var(--shadow)", padding: 5, maxHeight: 320, overflowY: "auto" }}>
      {projects.map((p) => {
        const pa = agents.filter((a) => a.projectId === p.id);
        if (!pa.length) return null;
        return (
          <div key={p.id}>
            <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "5px 8px 3px" }}>
              <span style={{ width: 6, height: 6, borderRadius: 2, background: p.color }} />
              <span className="up" style={{ fontSize: 8.5, color: "var(--ink-3)" }}>{p.name}</span>
            </div>
            {pa.map((a) => (
              <button key={a.id} onClick={() => { onPick(a.id); onClose(); }}
                style={{ display: "flex", alignItems: "center", gap: 8, width: "100%", textAlign: "left", padding: "5px 9px", borderRadius: 6, border: "none", cursor: "pointer",
                  background: value === a.id ? "var(--panel-3)" : "transparent", fontFamily: "var(--font-mono)", fontSize: 11.5, color: "var(--ink)" }}
                onMouseEnter={(e) => { if (value !== a.id) e.currentTarget.style.background = "var(--panel-2)"; }}
                onMouseLeave={(e) => { if (value !== a.id) e.currentTarget.style.background = "transparent"; }}>
                <StatusDot status={a.status} />
                <span style={{ flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{a.name}</span>
                <ToolBadge tool={a.tool} size={13} />
              </button>
            ))}
          </div>
        );
      })}
    </div>
  );
}

// ---- single leaf pane ----
function Pane({ node, agents, projects, liveLogs, onSplit, onClose, onAgent, onView, onFocus, canClose, focused, onDropOver, onPaneDrop, dropTarget, onAct, onFlash, gitView, openFile, onOpenGit, onCloseGitView, onCloseFile, onAppendLog, onSendPrompt, autosave, jump }) {
  const [pick, setPick] = useStateP(false);
  const [treeMode, setTreeMode] = useStateP(false);
  const fileStripRef = useRefP(null);
  const agent = agents.find((a) => a.id === node.agentId);
  const proj = agent ? projectOf(agent.projectId) : null;
  const dropSide = dropTarget && dropTarget.paneId === node.id ? dropTarget.side : null;
  const myGit = agent && gitView && gitView.agentId === agent.id ? gitView : null;
  const myFile = agent && openFile && openFile.agentId === agent.id ? openFile.path : null;
  const onOpenCommit = (sha) => onOpenGit && agent && onOpenGit(agent.id, { kind: "commit", sha });
  const gitBack = () => { onCloseGitView && onCloseGitView(); onView(node.id, "diff"); };

  const onDragOver = (e) => {
    if (!window.__omDrag || !window.__omDrag.agentId) return;
    e.preventDefault();
    const r = e.currentTarget.getBoundingClientRect();
    const px = (e.clientX - r.left) / r.width, py = (e.clientY - r.top) / r.height;
    const dl = px, dr = 1 - px, dt = py, db = 1 - py;
    const m = Math.min(dl, dr, dt, db);
    let side = "center";
    if (m < 0.26) side = m === dl ? "left" : m === dr ? "right" : m === dt ? "top" : "bottom";
    onDropOver(node.id, side);
  };
  const onDragLeave = (e) => { if (!e.currentTarget.contains(e.relatedTarget)) onDropOver(null); };
  const onDrop = (e) => { e.preventDefault(); onPaneDrop(node.id); };

  const zoneStyle = {
    center: { inset: 0 },
    left: { left: 0, top: 0, bottom: 0, width: "50%" },
    right: { right: 0, top: 0, bottom: 0, width: "50%" },
    top: { left: 0, right: 0, top: 0, height: "50%" },
    bottom: { left: 0, right: 0, bottom: 0, height: "50%" },
  }[dropSide];

  return (
    <div onMouseDown={() => onFocus(node.id)} onDragOver={onDragOver} onDragLeave={onDragLeave} onDrop={onDrop}
      style={{ flex: 1, minWidth: 0, minHeight: 0, display: "flex", flexDirection: "column", background: "var(--panel-2)", position: "relative",
        border: "1px solid " + (focused ? "color-mix(in oklch, var(--accent), transparent 55%)" : "var(--hair)"), borderRadius: "var(--r-md)", overflow: "hidden",
        boxShadow: focused ? "0 0 0 1px color-mix(in oklch, var(--accent), transparent 70%)" : "none" }}>
      {/* pane header */}
      <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "5px 6px 5px 8px", background: "var(--panel)", borderBottom: "1px solid var(--hair)", position: "relative", flex: "none" }}>
        {proj && <span style={{ width: 6, height: 6, borderRadius: 2, background: proj.color, flex: "none" }} title={proj.name} />}
        <button onClick={(e) => { e.stopPropagation(); setPick((v) => !v); }}
          style={{ display: "flex", alignItems: "center", gap: 6, background: "transparent", border: "none", cursor: "pointer", color: "var(--ink)", fontFamily: "var(--font-mono)", fontSize: 11.5, padding: "2px 4px", borderRadius: 5, minWidth: 0 }}
          onMouseEnter={(e) => e.currentTarget.style.background = "var(--panel-3)"}
          onMouseLeave={(e) => e.currentTarget.style.background = "transparent"}>
          {agent ? <StatusDot status={agent.status} /> : <Icon name="agent" size="sm" style={{ color: "var(--ink-4)" }} />}
          <span style={{ overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", maxWidth: 130 }}>{agent ? agent.name : "Assign agent"}</span>
          <Icon name="chevronD" size="sm" style={{ width: 11, height: 11, color: "var(--ink-4)" }} />
        </button>
        {pick && <AgentPicker projects={projects} agents={agents} value={node.agentId} onPick={(id) => onAgent(node.id, id)} onClose={() => setPick(false)} />}

        {/* view toggle */}
        {agent && (
          <div style={{ display: "flex", gap: 1, marginLeft: 4, padding: 2, background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: 5 }}>
            {[{ k: "terminal", icon: "terminal" }, { k: "diff", icon: "diff" }].map((v) => (
              <button key={v.k} onClick={(e) => { e.stopPropagation(); onView(node.id, v.k); }} title={v.k}
                style={{ display: "flex", padding: "2px 5px", borderRadius: 3, border: "none", cursor: "pointer",
                  background: node.view === v.k ? "var(--panel-3)" : "transparent", color: node.view === v.k ? "var(--accent)" : "var(--ink-3)" }}>
                <Icon name={v.icon} size="sm" style={{ width: 12, height: 12 }} />
              </button>
            ))}
          </div>
        )}

        {/* inline open-file tab strip — doubles as the flex spacer */}
        <div ref={fileStripRef} className="pane-filestrip"
          onWheel={(e) => { if (fileStripRef.current) { fileStripRef.current.scrollLeft += e.deltaY + e.deltaX; } }}
          style={{ flex: 1, minWidth: 0, display: "flex", alignItems: "center", gap: 3, marginLeft: 4, overflowX: "auto", scrollbarWidth: "none",
            maskImage: "linear-gradient(90deg, transparent 0, #000 10px, #000 calc(100% - 10px), transparent 100%)", WebkitMaskImage: "linear-gradient(90deg, transparent 0, #000 10px, #000 calc(100% - 10px), transparent 100%)" }}>
          {agent && myFile && (
            <div onClick={(e) => { e.stopPropagation(); onView(node.id, "file"); }}
              style={{ display: "flex", alignItems: "center", gap: 6, flex: "none", padding: "3px 5px 3px 8px", borderRadius: 5, cursor: "pointer",
                background: node.view === "file" ? "var(--panel-3)" : "transparent", border: "1px solid " + (node.view === "file" ? "var(--hair)" : "transparent") }}>
              <Icon name="file" size="sm" style={{ width: 12, height: 12, color: node.view === "file" ? "var(--accent)" : "var(--ink-4)" }} />
              <span style={{ fontFamily: "var(--font-mono)", fontSize: 10.5, color: node.view === "file" ? "var(--ink)" : "var(--ink-3)", whiteSpace: "nowrap" }}>{fileName(myFile)}</span>
              <button onClick={(e) => { e.stopPropagation(); onCloseFile && onCloseFile(); if (node.view === "file") onView(node.id, "terminal"); }} title="Close file"
                style={{ display: "flex", padding: 1, border: "none", background: "transparent", cursor: "pointer", color: "var(--ink-4)", borderRadius: 3 }}
                onMouseEnter={(e) => e.currentTarget.style.color = "var(--ink)"}
                onMouseLeave={(e) => e.currentTarget.style.color = "var(--ink-4)"}>
                <Icon name="x" size="sm" style={{ width: 11, height: 11 }} />
              </button>
            </div>
          )}
        </div>
        {/* play / pause */}
        {agent && (
          <button className="pane-btn" onClick={(e) => { e.stopPropagation(); onAct && onAct(agent.id, agent.status === "running" ? "pause" : "resume"); }}
            title={agent.status === "running" ? "Pause agent" : "Resume agent"} disabled={agent.status === "done"}
            style={{ color: "var(--accent)", opacity: agent.status === "done" ? 0.4 : 1 }}>
            <Icon name={agent.status === "running" ? "pause" : "play"} size="sm" style={{ width: 13, height: 13 }} />
          </button>
        )}
        {/* pane controls */}
        <button className="pane-btn" onClick={(e) => { e.stopPropagation(); onSplit(node.id, "v"); }} title="Split right">
          <Icon name="splitCol" size="sm" style={{ width: 13, height: 13 }} />
        </button>
        <button className="pane-btn" onClick={(e) => { e.stopPropagation(); onSplit(node.id, "h"); }} title="Split down">
          <Icon name="splitRow" size="sm" style={{ width: 13, height: 13 }} />
        </button>
        {canClose && (
          <button className="pane-btn" onClick={(e) => { e.stopPropagation(); onClose(node.id); }} title="Close pane">
            <Icon name="x" size="sm" style={{ width: 13, height: 13 }} />
          </button>
        )}
      </div>
      {/* pane body */}
      <div style={{ flex: 1, minHeight: 0, display: "flex", flexDirection: "column", overflow: "hidden" }}>
        {!agent ? (
          <div style={{ flex: 1, display: "grid", placeItems: "center", color: "var(--ink-4)" }}>
            <button className="btn ghost-hair" onClick={(e) => { e.stopPropagation(); setPick(true); }}><Icon name="plus" size="sm" />Assign an agent</button>
          </div>
        ) : node.view === "file" && myFile ? (
          <EditorPane path={myFile} agent={agent} onFlash={onFlash || (() => {})} structureOpen={false} autosave={autosave} jump={jump} onOpenCommit={onOpenCommit} />
        ) : node.view === "terminal" ? (
          <TerminalStack ag={agent} lines={liveLogs[agent.id] || []} streaming={agent.status === "running"} onFlash={onFlash} onSend={onSendPrompt || (() => {})} />
        ) : myGit ? (
          <React.Fragment>
            <div style={{ display: "flex", alignItems: "center", gap: 9, padding: "6px 10px", background: "var(--panel)", borderBottom: "1px solid var(--hair)", flex: "none" }}>
              <button className="btn ghost-hair" onClick={(e) => { e.stopPropagation(); gitBack(); }} title="Back to working changes"
                style={{ padding: "3px 9px", fontSize: 10.5, color: "var(--ink-2)" }}>
                <Icon name="chevron" size="sm" style={{ width: 11, height: 11, transform: "rotate(180deg)" }} />Working changes
              </button>
              <span style={{ display: "flex", alignItems: "center", gap: 5, fontSize: 10.5, color: "var(--ink-4)" }} className="tnum">
                <Icon name="branch" size="sm" style={{ width: 12, height: 12, color: "var(--accent-2)" }} />{agent.branch}
              </span>
            </div>
            <AgentGitView ag={agent} project={proj} gitView={myGit} onFlash={onFlash || (() => {})} onOpenCommit={onOpenCommit} />
          </React.Fragment>
        ) : (
          <DiffView ag={agent} project={proj} treeMode={treeMode} setTreeMode={setTreeMode} onAct={onAct} onOpenCommit={onOpenCommit} onFlash={onFlash} onAppendLog={onAppendLog} />
        )}
      </div>
      {dropSide && zoneStyle && (
        <div style={{ position: "absolute", inset: 0, zIndex: 20, pointerEvents: "none" }}>
          <div style={{ position: "absolute", ...zoneStyle,
            background: "color-mix(in oklch, var(--accent), transparent 78%)",
            border: "2px solid var(--accent)", borderRadius: "var(--r-md)",
            boxShadow: "0 0 24px -4px rgba(var(--accent-rgb), 0.6) inset", transition: "all 0.08s ease" }}>
            <div style={{ position: "absolute", top: "50%", left: "50%", transform: "translate(-50%,-50%)",
              display: "flex", alignItems: "center", gap: 6, padding: "4px 9px", borderRadius: 999,
              background: "var(--accent)", color: "#06070b", fontSize: 10.5, fontWeight: 600, whiteSpace: "nowrap" }}>
              <Icon name={dropSide === "center" ? "swap" : (dropSide === "left" || dropSide === "right") ? "splitCol" : "splitRow"} size="sm" style={{ width: 12, height: 12 }} />
              {dropSide === "center" ? "Replace" : "Split " + dropSide}
            </div>
          </div>
        </div>
      )}
    </div>
  );
}

// ---- recursive splitter with draggable divider ----
function SplitNode(props) {
  const { node } = props;
  const ref = useRefP(null);
  const [drag, setDrag] = useStateP(false);
  if (node.type === "leaf") return <Pane node={node} {...props} />;

  const horiz = node.dir === "v"; // side-by-side columns
  const onDown = (e) => {
    e.preventDefault(); e.stopPropagation();
    const el = e.currentTarget;
    if (el.setPointerCapture) el.setPointerCapture(e.pointerId);
    setDrag(true);
    const rect = ref.current.getBoundingClientRect();
    document.body.style.userSelect = "none";
    document.body.style.cursor = horiz ? "col-resize" : "row-resize";
    const move = (ev) => {
      const r = horiz ? (ev.clientX - rect.left) / rect.width : (ev.clientY - rect.top) / rect.height;
      props.onRatio(node.id, Math.max(0.1, Math.min(0.9, r)));
    };
    const up = (ev) => {
      setDrag(false);
      document.body.style.userSelect = "";
      document.body.style.cursor = "";
      if (el.releasePointerCapture) try { el.releasePointerCapture(ev.pointerId); } catch (_) {}
      el.removeEventListener("pointermove", move);
      el.removeEventListener("pointerup", up);
      el.removeEventListener("lostpointercapture", up);
    };
    el.addEventListener("pointermove", move);
    el.addEventListener("pointerup", up);
    el.addEventListener("lostpointercapture", up);
  };

  return (
    <div ref={ref} style={{ flex: 1, minWidth: 0, minHeight: 0, display: "flex", flexDirection: horiz ? "row" : "column", gap: 0 }}>
      <div style={{ flexGrow: node.ratio, flexShrink: 1, flexBasis: 0, minWidth: 0, minHeight: 0, display: "flex" }}>
        <SplitNode {...props} node={node.a} />
      </div>
      <div onPointerDown={onDown} className={"divider " + (horiz ? "v" : "h") + (drag ? " on" : "")}
        style={{ flex: "none", [horiz ? "width" : "height"]: 8, [horiz ? "height" : "width"]: "100%", alignSelf: "stretch",
          cursor: horiz ? "col-resize" : "row-resize",
          display: "flex", alignItems: "center", justifyContent: "center", touchAction: "none", position: "relative", zIndex: 3 }}>
        <span className="grip-handle" style={{ [horiz ? "width" : "height"]: 6, [horiz ? "height" : "width"]: "100%" }} />
      </div>
      <div style={{ flexGrow: 1 - node.ratio, flexShrink: 1, flexBasis: 0, minWidth: 0, minHeight: 0, display: "flex" }}>
        <SplitNode {...props} node={node.b} />
      </div>
    </div>
  );
}

function PaneManager({ root, setRoot, agents, projects, liveLogs, onFocusAgent, onAct, onFlash, gitView, openFile, onOpenGit, onCloseGitView, onCloseFile, onAppendLog, onSendPrompt, autosave, jump }) {
  const [focus, setFocus] = useStateP(null);
  const [dropTarget, setDropTarget] = useStateP(null); // { paneId, side }

  const pickNextAgent = () => {
    const used = new Set();
    (function walk(n) { if (n.type === "leaf") { if (n.agentId) used.add(n.agentId); } else { walk(n.a); walk(n.b); } })(root);
    const free = agents.find((a) => !used.has(a.id) && a.status === "running") || agents.find((a) => !used.has(a.id)) || agents[0];
    return free ? free.id : null;
  };

  const handlers = {
    onSplit: (id, dir) => setRoot((r) => splitLeaf(r, id, dir, leaf(pickNextAgent(), "terminal"))),
    onClose: (id) => setRoot((r) => removeLeaf(r, id)),
    onAgent: (id, agentId) => { setRoot((r) => patchLeaf(r, id, { agentId })); if (onFocusAgent) onFocusAgent(agentId); },
    onView: (id, view) => setRoot((r) => patchLeaf(r, id, { view })),
    onRatio: (id, ratio) => setRoot((r) => setRatio(r, id, ratio)),
    onFocus: (id) => { setFocus(id); const aid = leafAgentOf(root, id); if (aid && onFocusAgent) onFocusAgent(aid); },
    onDropOver: (id, side) => setDropTarget(id ? { paneId: id, side } : null),
    onPaneDrop: (id) => {
      const d = window.__omDrag;
      const side = dropTarget && dropTarget.paneId === id ? dropTarget.side : "center";
      if (d && d.agentId) {
        setRoot((r) => splitLeafSide(r, id, side, leaf(d.agentId, "terminal")));
        if (onFocusAgent) onFocusAgent(d.agentId);
      }
      setDropTarget(null);
      window.__omDrag = null;
    },
    dropTarget,
  };

  return (
    <div style={{ display: "flex", flexDirection: "column", minHeight: 0, background: "var(--panel-2)" }}>
      <div style={{ flex: 1, minHeight: 0, padding: 8, display: "flex" }}>
        <SplitNode node={root} agents={agents} projects={projects} liveLogs={liveLogs}
          onAct={onAct} onFlash={onFlash} gitView={gitView} openFile={openFile}
          onOpenGit={onOpenGit} onCloseGitView={onCloseGitView} onCloseFile={onCloseFile}
          onAppendLog={onAppendLog} onSendPrompt={onSendPrompt} autosave={autosave} jump={jump}
          {...handlers} canClose={countLeaves(root) > 1} focused={focus} />
      </div>
    </div>
  );
}

Object.assign(window, { PaneManager, makeLeaf: leaf, treeAgentIds, countLeaves, dropAgent, setLeafViewByAgent });
