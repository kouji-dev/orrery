/* global React, Icon, fileName */
// ORCHESTRA — FileTree (worktree explorer) + ContextMenu

const { useState: useStateX, useEffect: useEffectX, useRef: useRefX } = React;

// build nested tree from flat path list + a {path: state} map
function buildTree(paths, stateMap) {
  const root = {};
  paths.forEach((p) => {
    const isDir = p.endsWith("/");
    const parts = p.replace(/\/$/, "").split("/");
    let node = root;
    parts.forEach((part, i) => {
      const last = i === parts.length - 1;
      if (!node[part]) node[part] = { __dir: !last || isDir, __children: {}, __path: parts.slice(0, i + 1).join("/") + ((!last || isDir) ? "/" : "") };
      if (last) node[part].__state = stateMap[p];
      node = node[part].__children;
    });
  });
  // convert to array, dirs first then files, alpha
  const toArr = (obj) => Object.keys(obj).map((k) => {
    const v = obj[k];
    return { name: k, dir: v.__dir, path: v.__path, state: v.__state, children: v.__dir ? toArr(v.__children) : [] };
  }).sort((a, b) => (a.dir === b.dir) ? a.name.localeCompare(b.name) : (a.dir ? -1 : 1));
  return toArr(root);
}

const STATE_COLOR = {
  A: "var(--code-add-ink)", M: "var(--accent-2)", D: "var(--code-del-ink)",
};

function TreeNode({ node, depth, openMap, toggle, onOpenFile, activePath }) {
  const open = openMap[node.path] !== false; // default open
  const pad = 8 + depth * 13;
  if (node.dir) {
    const changed = countChanged(node);
    return (
      <div>
        <div onClick={() => toggle(node.path)} className="tree-row"
          style={{ display: "flex", alignItems: "center", gap: 6, padding: "3px 8px", paddingLeft: pad, cursor: "pointer", borderRadius: 5 }}>
          <Icon name={open ? "chevronD" : "chevron"} size="sm" style={{ width: 11, height: 11, color: "var(--ink-4)", flex: "none" }} />
          <Icon name={open ? "folderOpen" : "folder"} size="sm" style={{ color: node.state ? STATE_COLOR[node.state] : "var(--accent)", flex: "none", width: 13, height: 13 }} />
          <span style={{ fontSize: 11.5, color: "var(--ink-2)" }}>{node.name}</span>
          {node.state && <span style={{ fontSize: 9, fontWeight: 700, color: STATE_COLOR[node.state] }}>{node.state}</span>}
          {changed > 0 && <span className="tnum" style={{ marginLeft: "auto", fontSize: 9, color: "var(--ink-4)" }}>{changed}</span>}
        </div>
        {open && node.children.map((c) => <TreeNode key={c.path} node={c} depth={depth + 1} openMap={openMap} toggle={toggle} onOpenFile={onOpenFile} activePath={activePath} />)}
      </div>
    );
  }
  const active = activePath === node.path;
  return (
    <div className="tree-row" onClick={() => onOpenFile && onOpenFile(node.path)}
      draggable onDragStart={(e) => { window.__omDrag = { kind: "file", path: node.path }; e.dataTransfer.setData("text/plain", node.path); e.dataTransfer.effectAllowed = "copy"; }}
      onDragEnd={() => { window.__omDrag = null; }}
      title={"drag into an agent prompt to reference " + node.path}
      style={{ display: "flex", alignItems: "center", gap: 6, padding: "3px 8px", paddingLeft: pad + 17, cursor: "pointer", borderRadius: 5, background: active ? "var(--panel-3)" : "transparent" }}>
      <Icon name="file" size="sm" style={{ width: 12, height: 12, color: node.state ? STATE_COLOR[node.state] : "var(--ink-4)", flex: "none" }} />
      <span style={{ fontSize: 11.5, color: node.state || active ? "var(--ink)" : "var(--ink-3)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{node.name}</span>
      {node.state && <span style={{ marginLeft: "auto", fontSize: 9, fontWeight: 700, color: STATE_COLOR[node.state] }}>{node.state}</span>}
    </div>
  );
}

function countChanged(node) {
  let n = 0;
  const walk = (x) => { if (!x.dir && x.state) n++; if (x.state && x.dir) n++; x.children.forEach(walk); };
  node.children.forEach(walk);
  return n;
}

function FileTree({ project, agent, onOpenFile, activePath }) {
  const stateMap = {};
  (agent.files || []).forEach((f) => { stateMap[f.path] = f.state; });
  const allPaths = Array.from(new Set([...(project ? project.files : []), ...(agent.files || []).map((f) => f.path)]));
  const tree = buildTree(allPaths, stateMap);
  const [openMap, setOpenMap] = useStateX({});
  const toggle = (p) => setOpenMap((m) => ({ ...m, [p]: m[p] === false ? true : false }));
  const changedCount = (agent.files || []).length;
  return (
    <div style={{ display: "flex", flexDirection: "column", minHeight: 0, flex: 1 }}>
      <div style={{ display: "flex", alignItems: "center", gap: 7, padding: "8px 12px", borderBottom: "1px solid var(--hair)" }}>
        <Icon name="folder" size="sm" style={{ color: project ? project.color : "var(--accent)" }} />
        <span style={{ fontSize: 11.5, color: "var(--ink-2)" }}>{agent.worktree}</span>
        <span className="chip tnum" style={{ marginLeft: "auto", fontSize: 9, padding: "0 6px" }}>{changedCount} changed</span>
      </div>
      <div className="scroll-y" style={{ flex: 1, padding: "6px 4px" }}>
        {tree.map((n) => <TreeNode key={n.path} node={n} depth={0} openMap={openMap} toggle={toggle} onOpenFile={onOpenFile} activePath={activePath} />)}
      </div>
      <div style={{ padding: "7px 12px", borderTop: "1px solid var(--hair)", fontSize: 9.5, color: "var(--ink-4)", display: "flex", alignItems: "center", gap: 6 }}>
        <Icon name="enter" size="sm" style={{ width: 11, height: 11 }} />click a file to open it in the viewer
      </div>
    </div>
  );
}

// ---------- ContextMenu ----------
function ContextMenu({ x, y, items, onClose }) {
  const ref = useRefX(null);
  const [pos, setPos] = useStateX({ x, y });
  useEffectX(() => {
    const close = (e) => { if (ref.current && !ref.current.contains(e.target)) onClose(); };
    const esc = (e) => { if (e.key === "Escape") onClose(); };
    document.addEventListener("mousedown", close);
    document.addEventListener("keydown", esc);
    // clamp to viewport
    if (ref.current) {
      const r = ref.current.getBoundingClientRect();
      let nx = x, ny = y;
      if (x + r.width > window.innerWidth - 8) nx = window.innerWidth - r.width - 8;
      if (y + r.height > window.innerHeight - 8) ny = window.innerHeight - r.height - 8;
      setPos({ x: nx, y: ny });
    }
    return () => { document.removeEventListener("mousedown", close); document.removeEventListener("keydown", esc); };
  }, []);
  return (
    <div ref={ref} className="rise" style={{
      position: "fixed", left: pos.x, top: pos.y, zIndex: 80, minWidth: 196,
      background: "var(--elev)", border: "1px solid var(--hair-2)", borderRadius: "var(--r-md)",
      boxShadow: "var(--shadow)", padding: 5,
    }}>
      {items.map((it, i) => it.sep ? (
        <div key={i} style={{ height: 1, background: "var(--hair)", margin: "5px 6px" }} />
      ) : (
        <button key={i} disabled={it.disabled}
          onClick={() => { if (!it.disabled) { it.onClick && it.onClick(); onClose(); } }}
          style={{
            display: "flex", alignItems: "center", gap: 9, width: "100%", textAlign: "left",
            padding: "6px 9px", borderRadius: 6, border: "none", cursor: it.disabled ? "default" : "pointer",
            background: "transparent", fontFamily: "var(--font-mono)", fontSize: 12,
            color: it.disabled ? "var(--ink-4)" : it.danger ? "var(--st-blocked)" : "var(--ink-2)",
          }}
          onMouseEnter={(e) => { if (!it.disabled) e.currentTarget.style.background = it.danger ? "color-mix(in oklch, var(--st-blocked), transparent 88%)" : "var(--panel-3)"; }}
          onMouseLeave={(e) => e.currentTarget.style.background = "transparent"}>
          {it.icon && <Icon name={it.icon} size="sm" style={{ color: it.danger ? "var(--st-blocked)" : it.accent || "var(--ink-3)", flex: "none" }} />}
          <span style={{ flex: 1 }}>{it.label}</span>
          {it.kbd && <span className="kbd">{it.kbd}</span>}
        </button>
      ))}
    </div>
  );
}

Object.assign(window, { FileTree, ContextMenu, buildTree });
