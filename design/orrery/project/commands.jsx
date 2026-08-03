/* global React, Icon, ToolBadge, toolMeta, fileName, fileDir, langOf, StatusDot */
// Orrery — command registry, command palette, Search Everywhere, recent files,
// go-to-line, navigation stack. Every action in the app is registered here once;
// menus/overlays render FROM the registry (roadmap B2.1 / B2.2 / B2.3 / B2.6 / B6.2).

const { useState: useStateC, useEffect: useEffectC, useRef: useRefC, useMemo: useMemoC } = React;

// ---------------------------------------------------------------- keybindings
const IS_MAC = typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform || navigator.userAgent);

function parseBinding(str) {
  if (!str) return null;
  const parts = str.split("+").map((s) => s.trim());
  const b = { mod: false, ctrl: false, alt: false, shift: false, key: "" };
  parts.forEach((p) => {
    const l = p.toLowerCase();
    if (l === "mod") b.mod = true;
    else if (l === "ctrl") b.ctrl = true;
    else if (l === "alt") b.alt = true;
    else if (l === "shift") b.shift = true;
    else b.key = l;
  });
  return b;
}

function matchBinding(e, str) {
  const b = parseBinding(str);
  if (!b) return false;
  const k = (e.key || "").toLowerCase();
  if (k !== b.key) return false;
  const primary = IS_MAC ? e.metaKey : e.ctrlKey;
  if (b.mod && !primary) return false;
  if (!b.mod && !b.ctrl && (e.metaKey || (e.ctrlKey && k.length === 1))) return false;
  if (b.ctrl && !e.ctrlKey) return false;
  if (!b.ctrl && !b.mod && e.ctrlKey) return false;
  if (!!b.shift !== !!e.shiftKey) return false;
  if (!!b.alt !== !!e.altKey) return false;
  return true;
}

// "Mod+Shift+P" → "⌘⇧P" (mac) / "Ctrl+Shift+P"
function kbdLabel(str) {
  if (!str) return "";
  if (str === "Shift Shift") return IS_MAC ? "⇧⇧" : "Shift Shift";
  const map = IS_MAC
    ? { mod: "⌘", ctrl: "⌃", alt: "⌥", shift: "⇧" }
    : { mod: "Ctrl+", ctrl: "Ctrl+", alt: "Alt+", shift: "Shift+" };
  return str.split("+").map((p) => {
    const l = p.trim().toLowerCase();
    if (map[l]) return map[l];
    const K = { arrowup: "↑", arrowdown: "↓", arrowleft: "←", arrowright: "→", enter: "⏎", escape: "esc", "[": "[", "]": "]", "/": "/" }[l];
    return K || p.trim().toUpperCase();
  }).join("");
}

// ------------------------------------------------------------- fuzzy matching
// subsequence match with word-start + run bonuses; returns {score, idx} or null
function fzMatch(text, q) {
  if (!q) return { score: 0, idx: [] };
  const t = text.toLowerCase(), s = q.toLowerCase().replace(/\s+/g, "");
  let ti = 0, score = 0, run = 0;
  const idx = [];
  for (let qi = 0; qi < s.length; qi++) {
    const c = s[qi];
    let found = -1;
    for (let i = ti; i < t.length; i++) { if (t[i] === c) { found = i; break; } }
    if (found < 0) return null;
    const prev = found ? t[found - 1] : "";
    const boundary = found === 0 || prev === "/" || prev === "." || prev === "-" || prev === "_" || prev === " " || prev === ":";
    score += 6 + (boundary ? 10 : 0) + (found === ti ? run * 5 : 0);
    run = found === ti ? run + 1 : 1;
    idx.push(found);
    ti = found + 1;
  }
  score -= Math.max(0, text.length - s.length) * 0.12;
  if (t.startsWith(s)) score += 14;
  return { score, idx };
}

function FzText({ text, idx, style }) {
  if (!idx || !idx.length) return <span style={style}>{text}</span>;
  const set = new Set(idx);
  return <span style={style}>{text.split("").map((ch, i) => set.has(i)
    ? <b key={i} style={{ color: "var(--accent-2)", fontWeight: 600 }}>{ch}</b>
    : <span key={i}>{ch}</span>)}</span>;
}

// -------------------------------------------------------------- search corpus
const GITD = () => window.GIT || {};

function repoFilePaths() {
  const set = new Set(Object.keys(window.FILE_CONTENTS || {}));
  (GITD().workingTree || []).forEach((f) => set.add(f.path));
  (window.AGENTS || []).forEach((a) => (a.files || []).forEach((f) => set.add(f.path)));
  Object.values(window.DIFFS || {}).forEach((d) => d && set.add(d.file));
  (GITD().history || []).forEach((c) => (c.files || []).forEach((f) => set.add(f.path)));
  if (GITD().fileHistory) set.add(GITD().fileHistory.path);
  if (GITD().blameFile) set.add(GITD().blameFile.path);
  Object.keys(GITD().commitDiffs || {}).forEach((k) => set.add(k.split("/").slice(1).join("/")));
  return Array.from(set).filter(Boolean).sort();
}

const SYM_RE = [
  [/^\s*export\s+(?:default\s+)?(?:async\s+)?function\s+([A-Za-z_$][\w$]*)/, "function"],
  [/^\s*(?:async\s+)?function\s+([A-Za-z_$][\w$]*)/, "function"],
  [/^\s*export\s+(?:abstract\s+)?class\s+([A-Za-z_$][\w$]*)/, "class"],
  [/^\s*class\s+([A-Za-z_$][\w$]*)/, "class"],
  [/^\s*export\s+interface\s+([A-Za-z_$][\w$]*)/, "interface"],
  [/^\s*export\s+type\s+([A-Za-z_$][\w$]*)/, "type"],
  [/^\s*export\s+const\s+([A-Za-z_$][\w$]*)/, "const"],
  [/^\s*const\s+([A-Za-z_$][\w$]*)\s*=\s*(?:async\s*)?\(/, "function"],
  [/^\s*resource\s+"([^"]+)"\s+"([^"]+)"/, "resource"],
  [/^\s*CREATE\s+TABLE\s+(?:IF\s+NOT\s+EXISTS\s+)?([\w.]+)/i, "table"],
  [/^\s*(?:describe|it)\(['"]([^'"]+)/, "test"],
];

// symbols of one file (structure view + Search Everywhere both use this)
function symbolsOf(path) {
  const src = (window.FILE_CONTENTS || {})[path];
  const lines = src != null ? src.split("\n") : (window.getFileLines ? window.getFileLines(path, {}).map((l) => l.s) : []);
  const out = [];
  lines.forEach((s, i) => {
    for (const [re, kind] of SYM_RE) {
      const m = s.match(re);
      if (m) { out.push({ name: kind === "resource" ? m[1] + "." + m[2] : m[1], kind, line: i + 1, path, indent: (s.match(/^\s*/) || [""])[0].length }); break; }
    }
  });
  return out;
}

function allSymbols() {
  const out = [];
  Object.keys(window.FILE_CONTENTS || {}).forEach((p) => out.push(...symbolsOf(p)));
  return out;
}

const SYM_ICON = { function: "bolt", class: "box", interface: "layers", type: "tag", const: "commit", resource: "server", table: "database", test: "check" };

// ------------------------------------------------------------ command registry
// ctx supplies live state + every handler; commands stay declarative.
function buildCommandRegistry(ctx) {
  const ag = ctx.scopeAgent;
  const proj = ag ? (ctx.projects || []).find((p) => p.id === ag.projectId) : null;
  const cmd = (o) => o;
  const list = [
    // ---- navigate
    cmd({ id: "search.everywhere", label: "Search Everywhere", group: "Navigate", icon: "search", keys: { vscode: "Shift Shift", intellij: "Shift Shift" }, run: () => ctx.openOverlay("search") }),
    cmd({ id: "palette.open", label: "Show All Commands", group: "Navigate", icon: "bolt", keys: { vscode: "Mod+Shift+p", intellij: "Mod+Shift+a" }, run: () => ctx.openOverlay("palette") }),
    cmd({ id: "recent.files", label: "Recent Files", group: "Navigate", icon: "clock", keys: { vscode: "Mod+e", intellij: "Ctrl+e" }, run: () => ctx.openOverlay("recent") }),
    cmd({ id: "goto.line", label: "Go to Line…", group: "Navigate", icon: "enter", keys: { vscode: "Mod+g", intellij: "Mod+l" }, enabled: !!ctx.openFile, run: () => ctx.openOverlay("goto") }),
    cmd({ id: "goto.file", label: "Go to File…", group: "Navigate", icon: "file", keys: { vscode: "Mod+p", intellij: "Mod+Shift+o" }, run: () => ctx.openOverlay("search", "files") }),
    cmd({ id: "goto.symbol", label: "Go to Symbol…", group: "Navigate", icon: "spark", keys: { vscode: "Mod+Shift+s", intellij: "Mod+alt+o" }, run: () => ctx.openOverlay("search", "symbols") }),
    cmd({ id: "nav.back", label: "Back", group: "Navigate", icon: "enter", keys: { vscode: "Mod+alt+arrowleft", intellij: "Mod+[" }, enabled: ctx.canBack, run: ctx.navBack }),
    cmd({ id: "nav.forward", label: "Forward", group: "Navigate", icon: "chevron", keys: { vscode: "Mod+alt+arrowright", intellij: "Mod+]" }, enabled: ctx.canForward, run: ctx.navForward }),
    cmd({ id: "view.structure", label: "Toggle Structure View", group: "Navigate", icon: "layers", keys: { vscode: "Mod+Shift+7", intellij: "Mod+7" }, run: ctx.toggleStructure }),
    cmd({ id: "view.orchestrator", label: "Open Orchestrator", group: "Navigate", icon: "grid", keys: { vscode: "Mod+1", intellij: "Mod+1" }, run: () => ctx.goTab("orchestrator") }),
    cmd({ id: "view.backlog", label: "Open Backlog", group: "Navigate", icon: "archive", keys: { vscode: "Mod+2", intellij: "Mod+2" }, run: () => ctx.goTab("backlog") }),
    cmd({ id: "tab.close", label: "Close Tab", group: "Navigate", icon: "x", keys: { vscode: "Mod+w", intellij: "Mod+w" }, enabled: ctx.canCloseTab, run: ctx.closeActiveTab }),

    // ---- search
    cmd({ id: "find.inFiles", label: "Find in Files", group: "Search", icon: "search", keys: { vscode: "Mod+Shift+f", intellij: "Mod+Shift+f" }, run: () => ctx.openSearchPanel("find") }),
    cmd({ id: "find.replace", label: "Replace in Files", group: "Search", icon: "rename", keys: { vscode: "Mod+Shift+h", intellij: "Mod+Shift+r" }, run: () => ctx.openSearchPanel("replace") }),
    cmd({ id: "find.terminal", label: "Search Terminal Scrollback", group: "Search", icon: "terminal", keys: { vscode: "Mod+f", intellij: "Mod+f" }, run: ctx.searchScrollback }),

    // ---- git
    cmd({ id: "git.commit", label: "Commit Changes", group: "Git", icon: "commit", keys: { vscode: "Mod+k", intellij: "Mod+k" }, enabled: !!(ag && ag.files && ag.files.length), run: () => ctx.gitAction("commit") }),
    cmd({ id: "git.push", label: "Push to origin", group: "Git", icon: "push", keys: { vscode: "Mod+Shift+k", intellij: "Mod+Shift+k" }, enabled: !!(ag && ag.commits), run: () => ctx.gitAction("push") }),
    cmd({ id: "git.fetch", label: "Fetch", group: "Git", icon: "refresh", enabled: !!ag, run: () => ctx.gitAction("fetch") }),
    cmd({ id: "git.pull", label: "Pull", group: "Git", icon: "stage", keys: { intellij: "Mod+t" }, enabled: !!ag, run: () => ctx.gitAction("pull") }),
    cmd({ id: "git.merge", label: "Merge → " + (proj ? proj.branch : "main"), group: "Git", icon: "merge", enabled: !!(ag && ag.commits), run: () => ctx.gitAction("merge") }),
    cmd({ id: "git.rebase", label: "Rebase onto " + (proj ? proj.branch : "main"), group: "Git", icon: "branch", enabled: !!ag, run: () => ctx.gitAction("rebase") }),
    cmd({ id: "git.branches", label: "Branches & Remotes", group: "Git", icon: "branch", keys: { vscode: "Mod+Shift+b", intellij: "Mod+Shift+`" }, run: ctx.openBranches }),
    cmd({ id: "git.graph", label: "Show Commit Graph", group: "Git", icon: "graph", keys: { vscode: "Mod+Shift+g", intellij: "Mod+9" }, run: ctx.openCommitGraph }),
    cmd({ id: "git.conflicts", label: "Resolve Conflicts (3-way)", group: "Git", icon: "merge", enabled: ctx.hasConflict, run: ctx.openConflicts }),
    cmd({ id: "git.localHistory", label: "Local History", group: "Git", icon: "clock", run: ctx.openLocalHistory }),
    cmd({ id: "git.annotate", label: "Annotate with Git Blame", group: "Git", icon: "git", enabled: !!ctx.openFile, run: ctx.toggleBlame }),
    cmd({ id: "git.stash", label: "Stash Changes", group: "Git", icon: "archive", enabled: !!(ag && ag.files && ag.files.length), run: () => ctx.gitAction("stash") }),
    cmd({ id: "git.discard", label: "Discard Changes", group: "Git", icon: "discard", danger: true, enabled: !!(ag && ag.files && ag.files.length), run: () => ctx.gitAction("discard") }),

    // ---- agents
    cmd({ id: "agent.spawn", label: "Spawn Agent…", group: "Agents", icon: "bolt", keys: { vscode: "Mod+Shift+n", intellij: "Mod+n" }, run: ctx.spawnAgent }),
    cmd({ id: "agent.pauseResume", label: ag && ag.status === "running" ? "Pause Agent" : "Resume Agent", group: "Agents", icon: ag && ag.status === "running" ? "pause" : "play", enabled: !!ag, run: () => ctx.gitAction(ag && ag.status === "running" ? "pause" : "resume") }),
    cmd({ id: "agent.sleep", label: ag && ctx.isAsleep(ag.id) ? "Wake Agent" : "Sleep Agent (free terminal memory)", group: "Agents", icon: "moon", enabled: !!ag, run: () => ctx.toggleSleep(ag && ag.id) }),
    cmd({ id: "agent.runAll", label: ctx.running ? "Pause All Agents" : "Resume All Agents", group: "Agents", icon: ctx.running ? "pause" : "play", run: ctx.toggleRunAll }),
    cmd({ id: "agent.delete", label: "Delete Worktree", group: "Agents", icon: "trash", danger: true, enabled: !!ag, run: () => ctx.deleteAgent(ag && ag.id) }),
    cmd({ id: "ticket.new", label: "New Ticket", group: "Agents", icon: "plus", run: ctx.newTicket }),

    // ---- editor
    cmd({ id: "file.save", label: "Save File", group: "Editor", icon: "check", keys: { vscode: "Mod+s", intellij: "Mod+s" }, enabled: !!ctx.openFile, run: ctx.saveFile }),
    cmd({ id: "file.autosave", label: (ctx.autosave ? "Disable" : "Enable") + " Autosave", group: "Editor", icon: "clock", run: ctx.toggleAutosave }),
    cmd({ id: "file.revertHunk", label: "Revert Hunk at Cursor", group: "Editor", icon: "discard", enabled: !!ctx.openFile, run: ctx.revertHunk }),

    // ---- terminal / workspace
    cmd({ id: "term.splitRight", label: "Split Terminal Right", group: "Workspace", icon: "splitCol", keys: { vscode: "Mod+\\", intellij: "Mod+Shift+\\" }, run: () => ctx.splitTerminal("v") }),
    cmd({ id: "term.splitDown", label: "Split Terminal Down", group: "Workspace", icon: "splitRow", run: () => ctx.splitTerminal("h") }),
    cmd({ id: "ws.rightPanel", label: "Toggle Right Panel", group: "Workspace", icon: "panelLeft", keys: { vscode: "Mod+alt+b", intellij: "Mod+Shift+p" }, run: ctx.toggleRightPanel }),
    cmd({ id: "ws.sidebar", label: "Toggle Compact Sidebar", group: "Workspace", icon: "columns", keys: { vscode: "Mod+b", intellij: "Mod+alt+s" }, run: ctx.toggleSidebar }),
    cmd({ id: "ws.theme", label: "Toggle Light / Dark Theme", group: "Workspace", icon: "sun", run: ctx.toggleTheme }),

    // ---- tools
    cmd({ id: "perf.panel", label: "Performance & Process Tree", group: "Tools", icon: "cpu", keys: { vscode: "Mod+Shift+u", intellij: "Mod+Shift+u" }, run: ctx.openPerf }),
    cmd({ id: "cost.dashboard", label: "Token & Cost Dashboard", group: "Tools", icon: "spark", run: ctx.openCost }),
    cmd({ id: "github.prs", label: "GitHub Pull Requests", group: "Tools", icon: "pr", run: ctx.openGithub }),
    cmd({ id: "settings.open", label: "Settings", group: "Tools", icon: "settings", keys: { vscode: "Mod+,", intellij: "Mod+," }, run: ctx.openSettings }),
    cmd({ id: "settings.keymap", label: "Keymap Preset…", group: "Tools", icon: "lock", run: ctx.openKeymap }),
    cmd({ id: "app.whatsNew", label: "What's New", group: "Tools", icon: "flag", run: ctx.showWhatsNew }),
  ];
  const preset = ctx.keymap === "intellij" ? "intellij" : "vscode";
  return list.filter((c) => c.run).map((c) => ({ ...c, kbd: (c.keys && (c.keys[preset] || c.keys.vscode || c.keys.intellij)) || "", enabled: c.enabled === undefined ? true : !!c.enabled }));
}

// ------------------------------------------------------------- overlay shells
function OverlayShell({ width = 640, top = "12vh", onClose, children, label }) {
  const box = useRefC(null);
  useEffectC(() => {
    const onDown = (e) => { if (box.current && !box.current.contains(e.target)) onClose(); };
    document.addEventListener("mousedown", onDown);
    return () => document.removeEventListener("mousedown", onDown);
  }, [onClose]);
  return (
    <div style={{ position: "fixed", inset: 0, zIndex: 80, background: "rgba(0,0,0,0.44)", backdropFilter: "blur(2px)", display: "flex", justifyContent: "center", alignItems: "flex-start", padding: "0 20px" }}>
      <div ref={box} className="surface rise" role="dialog" aria-label={label}
        style={{ width, maxWidth: "100%", marginTop: top, boxShadow: "var(--shadow)", overflow: "hidden", display: "flex", flexDirection: "column", maxHeight: "72vh" }}>
        {children}
      </div>
    </div>
  );
}

function OverlayInput({ value, onChange, placeholder, onKeyDown, icon = "search", right }) {
  const ref = useRefC(null);
  useEffectC(() => { if (ref.current) ref.current.focus(); }, []);
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "12px 14px", borderBottom: "1px solid var(--hair)", flex: "none" }}>
      <Icon name={icon} style={{ color: "var(--accent)" }} />
      <input ref={ref} value={value} onChange={(e) => onChange(e.target.value)} onKeyDown={onKeyDown} placeholder={placeholder}
        style={{ flex: 1, minWidth: 0, background: "transparent", border: "none", outline: "none", color: "var(--ink)", fontFamily: "var(--font-mono)", fontSize: 14 }} />
      {right}
    </div>
  );
}

function OverlayFooter({ hints }) {
  return (
    <div style={{ display: "flex", alignItems: "center", gap: 12, padding: "7px 14px", borderTop: "1px solid var(--hair)", background: "var(--panel-2)", flex: "none", fontSize: 9.5, color: "var(--ink-4)" }}>
      {hints.map((h, i) => <span key={i} style={{ display: "flex", gap: 5, alignItems: "center" }}><span className="kbd">{h[0]}</span>{h[1]}</span>)}
    </div>
  );
}

// keyboard list navigation shared by every overlay
function useListNav(count, onPick, onClose) {
  const [sel, setSel] = useStateC(0);
  useEffectC(() => { setSel(0); }, [count]);
  const onKeyDown = (e) => {
    if (e.key === "ArrowDown") { e.preventDefault(); setSel((s) => (count ? (s + 1) % count : 0)); }
    else if (e.key === "ArrowUp") { e.preventDefault(); setSel((s) => (count ? (s - 1 + count) % count : 0)); }
    else if (e.key === "Enter") { e.preventDefault(); if (count) onPick(sel); }
    else if (e.key === "Escape") { e.preventDefault(); onClose(); }
    else return false;
    return true;
  };
  return [sel, setSel, onKeyDown];
}

function RowShell({ active, onClick, onMouseEnter, children, danger }) {
  return (
    <div onClick={onClick} onMouseEnter={onMouseEnter}
      style={{ display: "flex", alignItems: "center", gap: 10, padding: "7px 14px", cursor: "pointer", borderLeft: "2px solid " + (active ? "var(--accent)" : "transparent"),
        background: active ? "var(--panel-3)" : "transparent", color: danger ? "var(--st-blocked)" : "inherit" }}>
      {children}
    </div>
  );
}

// ------------------------------------------------------------ command palette
function CommandPalette({ commands, onClose }) {
  const [q, setQ] = useStateC("");
  const items = useMemoC(() => {
    const scored = commands.map((c) => {
      const m = fzMatch(c.label, q) || (q ? fzMatch(c.group + " " + c.label, q) : { score: 0, idx: [] });
      return m ? { c, score: m.score + (c.enabled ? 4 : 0), idx: fzMatch(c.label, q) ? m.idx : [] } : null;
    }).filter(Boolean);
    if (q) scored.sort((a, b) => b.score - a.score);
    return scored.slice(0, 60);
  }, [commands, q]);
  const pick = (i) => { const it = items[i]; if (it && it.c.enabled) { onClose(); setTimeout(() => it.c.run(), 0); } };
  const [sel, setSel, onKeyDown] = useListNav(items.length, pick, onClose);
  const listRef = useRefC(null);
  useEffectC(() => {
    const el = listRef.current && listRef.current.children[sel];
    if (el && el.offsetTop < listRef.current.scrollTop) listRef.current.scrollTop = el.offsetTop;
    else if (el && el.offsetTop + el.offsetHeight > listRef.current.scrollTop + listRef.current.clientHeight) listRef.current.scrollTop = el.offsetTop + el.offsetHeight - listRef.current.clientHeight;
  }, [sel]);
  let lastGroup = null;
  return (
    <OverlayShell width={600} onClose={onClose} label="Command palette">
      <OverlayInput value={q} onChange={setQ} onKeyDown={onKeyDown} placeholder="Type a command…" icon="bolt"
        right={<span className="chip" style={{ fontSize: 9 }}>{commands.length} commands</span>} />
      <div ref={listRef} className="scroll-y" style={{ flex: 1, padding: "6px 0" }}>
        {items.length === 0 && <div style={{ padding: "18px 14px", fontSize: 11.5, color: "var(--ink-4)" }}>no matching command</div>}
        {items.map((it, i) => {
          const c = it.c;
          const head = !q && c.group !== lastGroup ? (lastGroup = c.group) : null;
          return (
            <React.Fragment key={c.id}>
              {head && <div className="up" style={{ fontSize: 9, color: "var(--ink-4)", padding: "8px 14px 4px" }}>{head}</div>}
              <RowShell active={i === sel} onMouseEnter={() => setSel(i)} onClick={() => pick(i)} danger={c.danger}>
                <Icon name={c.icon} size="sm" style={{ color: i === sel ? "var(--accent)" : "var(--ink-3)" }} />
                <span style={{ flex: 1, fontSize: 12.5, opacity: c.enabled ? 1 : 0.42, whiteSpace: "nowrap", overflow: "hidden", textOverflow: "ellipsis" }}>
                  <FzText text={c.label} idx={it.idx} />
                </span>
                {q && <span style={{ fontSize: 9.5, color: "var(--ink-4)", flex: "none" }}>{c.group}</span>}
                {c.kbd && <span className="kbd" style={{ flex: "none" }}>{kbdLabel(c.kbd)}</span>}
              </RowShell>
            </React.Fragment>
          );
        })}
      </div>
      <OverlayFooter hints={[["↑↓", "navigate"], ["⏎", "run"], ["esc", "close"]]} />
    </OverlayShell>
  );
}

// --------------------------------------------------------- Search Everywhere
const SE_TABS = [
  { k: "all", label: "All" }, { k: "files", label: "Files" }, { k: "symbols", label: "Symbols" },
  { k: "agents", label: "Agents" }, { k: "tickets", label: "Tickets" }, { k: "commands", label: "Actions" }, { k: "git", label: "Git" },
];

function SearchEverywhere({ ctx, commands, initialTab, onClose }) {
  const [tab, setTab] = useStateC(initialTab || "all");
  const [q, setQ] = useStateC("");
  const corpus = useMemoC(() => {
    const files = repoFilePaths().map((p) => ({ type: "file", key: "f:" + p, text: p, label: fileName(p), sub: fileDir(p), meta: langOf(p) }));
    const symbols = allSymbols().map((s) => ({ type: "symbol", key: "s:" + s.path + ":" + s.line, text: s.name, label: s.name, sub: s.path + ":" + s.line, meta: s.kind, path: s.path, line: s.line }));
    const agents = (ctx.agents || []).map((a) => ({ type: "agent", key: "a:" + a.id, text: a.name + " " + a.branch + " " + (a.task || ""), label: a.name, sub: a.branch, meta: a.status, agent: a }));
    const tickets = (ctx.tickets || []).map((t) => ({ type: "ticket", key: "t:" + t.id, text: t.title, label: t.title, sub: (t.tags || []).join(" · ") || t.status, meta: t.status, ticket: t }));
    const cmds = commands.map((c) => ({ type: "command", key: "c:" + c.id, text: c.label + " " + c.group, label: c.label, sub: c.group, meta: kbdLabel(c.kbd), cmd: c }));
    const refs = [];
    (ctx.projects || []).forEach((p) => (p.branches || []).forEach((b) => refs.push({ type: "ref", key: "r:" + p.id + b, text: b, label: b, sub: p.name, meta: "branch" })));
    (ctx.agents || []).forEach((a) => refs.push({ type: "ref", key: "r:" + a.id, text: a.branch, label: a.branch, sub: a.name, meta: "agent branch" }));
    (GITD().history || []).slice(0, 24).forEach((c) => refs.push({ type: "ref", key: "r:" + c.sha, text: c.sha + " " + (c.msg || c.summary || ""), label: c.sha, sub: c.msg || c.summary || "", meta: "commit", sha: c.sha }));
    return { files, symbols, agents, tickets, commands: cmds, git: refs };
  }, [ctx.agents, ctx.tickets, ctx.projects, commands]);

  const pool = tab === "all"
    ? [].concat(corpus.files, corpus.symbols, corpus.agents, corpus.tickets, corpus.commands, corpus.git)
    : corpus[tab] || [];
  const items = useMemoC(() => {
    const scored = pool.map((it) => {
      const m = fzMatch(it.label, q) || fzMatch(it.text, q);
      if (!m) return null;
      const bump = { agent: 8, file: 6, symbol: 4, ticket: 4, command: 2, ref: 0 }[it.type] || 0;
      return { it, score: m.score + bump, idx: fzMatch(it.label, q) ? m.idx : [] };
    }).filter(Boolean);
    scored.sort((a, b) => b.score - a.score);
    return scored.slice(0, tab === "all" ? 40 : 80);
  }, [pool, q, tab]);

  const open = (r) => {
    const it = r.it;
    onClose();
    setTimeout(() => {
      if (it.type === "file") ctx.openFileAt(it.text);
      else if (it.type === "symbol") ctx.openFileAt(it.path, it.line);
      else if (it.type === "agent") ctx.openAgent(it.agent.id);
      else if (it.type === "ticket") ctx.openTicket(it.ticket.id);
      else if (it.type === "command") { if (it.cmd.enabled) it.cmd.run(); }
      else if (it.type === "ref") { if (it.sha) ctx.openCommit(it.sha); else ctx.openBranches(); }
    }, 0);
  };
  const [sel, setSel, listKeys] = useListNav(items.length, (i) => items[i] && open(items[i]), onClose);
  const onKeyDown = (e) => {
    if (e.key === "Tab") { e.preventDefault(); const i = SE_TABS.findIndex((t) => t.k === tab); setTab(SE_TABS[(i + (e.shiftKey ? -1 + SE_TABS.length : 1)) % SE_TABS.length].k); return; }
    listKeys(e);
  };
  const listRef = useRefC(null);
  useEffectC(() => {
    const el = listRef.current && listRef.current.children[sel];
    if (!el || !listRef.current) return;
    if (el.offsetTop < listRef.current.scrollTop) listRef.current.scrollTop = el.offsetTop;
    else if (el.offsetTop + el.offsetHeight > listRef.current.scrollTop + listRef.current.clientHeight) listRef.current.scrollTop = el.offsetTop + el.offsetHeight - listRef.current.clientHeight;
  }, [sel]);

  const rowIcon = (it) => it.type === "file" ? "file" : it.type === "symbol" ? (SYM_ICON[it.meta] || "spark")
    : it.type === "ticket" ? "archive" : it.type === "command" ? "bolt" : it.type === "ref" ? (it.sha ? "commit" : "branch") : "agent";

  return (
    <OverlayShell width={720} top="9vh" onClose={onClose} label="Search everywhere">
      <OverlayInput value={q} onChange={setQ} onKeyDown={onKeyDown} placeholder="Search files, symbols, agents, tickets, actions, refs…" />
      <div style={{ display: "flex", alignItems: "center", gap: 2, padding: "0 10px", borderBottom: "1px solid var(--hair)", background: "var(--panel)", flex: "none" }}>
        {SE_TABS.map((t) => {
          const on = tab === t.k;
          const n = t.k === "all" ? null : (corpus[t.k] || []).length;
          return (
            <button key={t.k} className="btn" onClick={() => setTab(t.k)}
              style={{ padding: "8px 10px", borderRadius: 0, position: "relative", fontSize: 11.5, color: on ? "var(--ink)" : "var(--ink-3)" }}>
              {on && <span style={{ position: "absolute", left: 8, right: 8, bottom: 0, height: 2, background: "linear-gradient(90deg, var(--accent), var(--accent-2))" }} />}
              {t.label}{n != null && <span className="tnum" style={{ fontSize: 9, color: "var(--ink-4)" }}>{n}</span>}
            </button>
          );
        })}
      </div>
      <div ref={listRef} className="scroll-y" style={{ flex: 1, padding: "5px 0", minHeight: 120 }}>
        {items.length === 0 && <div style={{ padding: "18px 14px", fontSize: 11.5, color: "var(--ink-4)" }}>{q ? "nothing matches “" + q + "”" : "start typing to search everything"}</div>}
        {items.map((r, i) => {
          const it = r.it;
          return (
            <RowShell key={it.key} active={i === sel} onMouseEnter={() => setSel(i)} onClick={() => open(r)}>
              {it.type === "agent"
                ? <ToolBadge tool={it.agent.tool} size={15} />
                : <Icon name={rowIcon(it)} size="sm" style={{ color: i === sel ? "var(--accent)" : "var(--ink-3)" }} />}
              <span style={{ fontSize: 12.5, flex: "none", maxWidth: "52%", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>
                <FzText text={it.label} idx={r.idx} />
              </span>
              {it.sub && <span style={{ fontSize: 10, color: "var(--ink-4)", flex: 1, minWidth: 0, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{it.sub}</span>}
              {it.type === "agent" && <StatusDot status={it.agent.status} />}
              {it.meta && <span className="chip" style={{ fontSize: 9, flex: "none", padding: "1px 6px" }}>{it.meta}</span>}
              {tab === "all" && <span className="up" style={{ fontSize: 8.5, color: "var(--ink-4)", flex: "none", width: 52, textAlign: "right" }}>{it.type}</span>}
            </RowShell>
          );
        })}
      </div>
      <OverlayFooter hints={[["↑↓", "navigate"], ["⏎", "open"], ["⇥", "next tab"], ["esc", "close"]]} />
    </OverlayShell>
  );
}

// ------------------------------------------------------------- recent files
function RecentFiles({ items, onPick, onClose }) {
  const pick = (i) => { const it = items[i]; if (it) { onClose(); setTimeout(() => onPick(it), 0); } };
  const [sel, setSel, onKeyDown] = useListNav(items.length, pick, onClose);
  const ref = useRefC(null);
  useEffectC(() => { if (ref.current) ref.current.focus(); }, []);
  return (
    <OverlayShell width={480} top="16vh" onClose={onClose} label="Recent files">
      <div tabIndex={0} ref={ref} onKeyDown={onKeyDown} style={{ outline: "none", display: "flex", flexDirection: "column", minHeight: 0 }}>
        <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "10px 14px", borderBottom: "1px solid var(--hair)" }}>
          <Icon name="clock" size="sm" style={{ color: "var(--accent)" }} />
          <span className="up" style={{ fontSize: 9.5, color: "var(--ink-3)" }}>Recent files</span>
          <span className="tnum" style={{ marginLeft: "auto", fontSize: 9.5, color: "var(--ink-4)" }}>{items.length}</span>
        </div>
        <div className="scroll-y" style={{ padding: "5px 0", maxHeight: "50vh" }}>
          {items.length === 0 && <div style={{ padding: "16px 14px", fontSize: 11.5, color: "var(--ink-4)" }}>no files opened yet</div>}
          {items.map((it, i) => (
            <RowShell key={it.path + i} active={i === sel} onMouseEnter={() => setSel(i)} onClick={() => pick(i)}>
              <Icon name="file" size="sm" style={{ color: i === sel ? "var(--accent)" : "var(--ink-3)" }} />
              <span style={{ fontSize: 12.5 }}>{fileName(it.path)}</span>
              <span style={{ fontSize: 10, color: "var(--ink-4)", flex: 1, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{fileDir(it.path)}</span>
              {it.agentName && <span className="chip" style={{ fontSize: 9, padding: "1px 6px" }}>{it.agentName}</span>}
            </RowShell>
          ))}
        </div>
        <OverlayFooter hints={[["↑↓", "navigate"], ["⏎", "open"], ["esc", "close"]]} />
      </div>
    </OverlayShell>
  );
}

// ---------------------------------------------------------------- go to line
function GoToLine({ path, max, onGo, onClose }) {
  const [v, setV] = useStateC("");
  const parsed = (() => { const m = v.match(/^(\d+)(?::(\d+))?$/); return m ? { line: Math.min(max, Math.max(1, +m[1])), col: m[2] ? +m[2] : 1 } : null; })();
  const submit = () => { if (parsed) { onClose(); setTimeout(() => onGo(parsed.line, parsed.col), 0); } };
  return (
    <OverlayShell width={380} top="20vh" onClose={onClose} label="Go to line">
      <OverlayInput value={v} onChange={(s) => setV(s.replace(/[^\d:]/g, ""))} icon="enter" placeholder={"[Line] or [Line:Column] · 1–" + max}
        onKeyDown={(e) => { if (e.key === "Enter") { e.preventDefault(); submit(); } else if (e.key === "Escape") onClose(); }}
        right={<span className="chip" style={{ fontSize: 9 }}>{fileName(path || "")}</span>} />
      <div style={{ display: "flex", alignItems: "center", gap: 10, padding: "9px 14px", fontSize: 10.5, color: "var(--ink-4)" }}>
        {parsed ? <span style={{ color: "var(--accent-2)" }}>→ line {parsed.line}{parsed.col > 1 ? ", column " + parsed.col : ""}</span> : <span>enter a line number</span>}
        <span style={{ marginLeft: "auto" }} className="tnum">{max} lines</span>
      </div>
    </OverlayShell>
  );
}

// -------------------------------------------------------------- keymap picker
const KEYMAP_PRESETS = [
  { k: "intellij", label: "IntelliJ IDEA", sub: "JetBrains defaults — ⇧⇧, ⌘⇧A, ⌘1/2, ⌘[ / ⌘]" },
  { k: "vscode", label: "Visual Studio Code", sub: "VS Code defaults — ⌘P, ⌘⇧P, ⌘⇧F, ⌥← / ⌥→" },
];

function KeymapModal({ keymap, commands, onPick, onClose }) {
  return (
    <OverlayShell width={640} top="10vh" onClose={onClose} label="Keymap">
      <div style={{ display: "flex", alignItems: "center", gap: 9, padding: "12px 16px", borderBottom: "1px solid var(--hair)" }}>
        <Icon name="lock" style={{ color: "var(--accent)" }} />
        <span className="disp" style={{ fontSize: 14, fontWeight: 600 }}>Keymap</span>
        <span className="chip" style={{ marginLeft: "auto", fontSize: 9.5 }}>{commands.length} registered commands</span>
      </div>
      <div style={{ display: "flex", gap: 8, padding: 14, flex: "none" }}>
        {KEYMAP_PRESETS.map((p) => {
          const on = keymap === p.k;
          return (
            <button key={p.k} className="btn" onClick={() => onPick(p.k)}
              style={{ flex: 1, flexDirection: "column", alignItems: "flex-start", gap: 4, padding: "10px 12px", borderRadius: "var(--r-md)", textAlign: "left",
                border: "1px solid " + (on ? "color-mix(in oklch, var(--accent), transparent 45%)" : "var(--hair)"),
                background: on ? "color-mix(in oklch, var(--accent), transparent 88%)" : "var(--panel-2)" }}>
              <span style={{ display: "flex", alignItems: "center", gap: 7, fontSize: 12.5, color: "var(--ink)" }}>
                {on && <Icon name="check" size="sm" style={{ color: "var(--accent)" }} />}{p.label}
              </span>
              <span style={{ fontSize: 9.5, color: "var(--ink-4)", whiteSpace: "normal" }}>{p.sub}</span>
            </button>
          );
        })}
      </div>
      <div className="scroll-y" style={{ flex: 1, borderTop: "1px solid var(--hair)" }}>
        {commands.filter((c) => c.kbd).map((c) => (
          <div key={c.id} style={{ display: "flex", alignItems: "center", gap: 10, padding: "6px 16px", fontSize: 11.5 }}>
            <Icon name={c.icon} size="sm" style={{ color: "var(--ink-3)" }} />
            <span style={{ flex: 1, color: "var(--ink-2)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{c.label}</span>
            <span style={{ fontSize: 9, color: "var(--ink-4)" }}>{c.group}</span>
            <span className="kbd" style={{ minWidth: 54, textAlign: "center" }}>{kbdLabel(c.kbd)}</span>
          </div>
        ))}
      </div>
      <OverlayFooter hints={[["esc", "close"]]} />
    </OverlayShell>
  );
}

// ------------------------------------------------------- nav stack (B2.6) UI
function NavArrows({ canBack, canForward, onBack, onForward, keymap }) {
  const back = keymap === "intellij" ? "Mod+[" : "Mod+alt+arrowleft";
  const fwd = keymap === "intellij" ? "Mod+]" : "Mod+alt+arrowright";
  const btn = (on, click, rot, title) => (
    <button className="pill-seg" onClick={click} disabled={!on} title={title}
      style={{ opacity: on ? 1 : 0.35, cursor: on ? "pointer" : "default", minWidth: 26, padding: "0 6px" }}>
      <Icon name="chevron" size="sm" style={{ transform: rot }} />
    </button>
  );
  return (
    <div className="action-pill" style={{ height: 26 }}>
      {btn(canBack, onBack, "rotate(180deg)", "Back · " + kbdLabel(back))}
      <span className="pill-div" />
      {btn(canForward, onForward, "none", "Forward · " + kbdLabel(fwd))}
    </div>
  );
}

// --------------------------------------------------------------- key handling
// One global listener drives every registered binding + double-shift.
function CommandCenter({ ctx, overlay, setOverlay }) {
  const commands = useMemoC(() => buildCommandRegistry(ctx), [ctx]);
  const shiftRef = useRefC({ t: 0, armed: false });
  const cmdRef = useRefC(commands);
  cmdRef.current = commands;
  const ctxRef = useRefC(ctx);
  ctxRef.current = ctx;

  useEffectC(() => { window.ORRERY_COMMANDS = commands; }, [commands]);

  useEffectC(() => {
    const onKey = (e) => {
      const tag = (e.target && e.target.tagName) || "";
      const typing = tag === "INPUT" || tag === "TEXTAREA" || (e.target && e.target.isContentEditable);
      if (e.key === "Shift") {
        const now = Date.now();
        if (!typing && shiftRef.current.armed && now - shiftRef.current.t < 380) {
          shiftRef.current.armed = false;
          e.preventDefault();
          setOverlay({ kind: "search" });
          return;
        }
        shiftRef.current = { t: now, armed: true };
        return;
      }
      if (e.key !== "Shift") shiftRef.current.armed = false;
      if (e.key === "Escape") { if (overlay) { setOverlay(null); } return; }
      for (const c of cmdRef.current) {
        if (!c.kbd || c.kbd === "Shift Shift") continue;
        if (matchBinding(e, c.kbd)) {
          if (typing && !(e.metaKey || e.ctrlKey)) return;
          e.preventDefault();
          if (c.enabled) c.run(); else ctxRef.current.flash && ctxRef.current.flash(c.label + " — not available here");
          return;
        }
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [overlay, setOverlay]);

  const close = () => setOverlay(null);
  if (!overlay) return null;
  if (overlay.kind === "palette") return <CommandPalette commands={commands} onClose={close} />;
  if (overlay.kind === "search") return <SearchEverywhere ctx={ctx} commands={commands} initialTab={overlay.tab} onClose={close} />;
  if (overlay.kind === "recent") return <RecentFiles items={ctx.recentFiles || []} onPick={(it) => ctx.openFileAt(it.path)} onClose={close} />;
  if (overlay.kind === "goto") return <GoToLine path={ctx.openFile && ctx.openFile.path} max={ctx.openFileLines || 1} onGo={ctx.goToLine} onClose={close} />;
  if (overlay.kind === "keymap") return <KeymapModal keymap={ctx.keymap} commands={commands} onPick={(k) => { ctx.setKeymap(k); }} onClose={close} />;
  return null;
}

Object.assign(window, {
  buildCommandRegistry, CommandCenter, CommandPalette, SearchEverywhere, RecentFiles, GoToLine, KeymapModal,
  NavArrows, kbdLabel, matchBinding, fzMatch, FzText, symbolsOf, allSymbols, repoFilePaths, SYM_ICON, KEYMAP_PRESETS, IS_MAC,
});
