/* global React */
// ORCHESTRA shared components & icons

const STATUS_META = {
  running: { label: "running", color: "var(--st-running)" },
  blocked: { label: "blocked", color: "var(--st-blocked)" },
  waiting: { label: "waiting", color: "var(--st-waiting)" },
  done: { label: "done", color: "var(--st-done)" },
  idle: { label: "idle", color: "var(--st-idle)" },
  queued: { label: "queued", color: "var(--st-idle)" },
};

// ---- icon set (stroke, currentColor) ----
const P = (d, extra) => ({ d, ...extra });
const ICONS = {
  agent: "M12 3l7 4v5c0 4.4-3 7.6-7 9-4-1.4-7-4.6-7-9V7l7-4z",
  branch: "M6 4v9M6 13a3 3 0 003 3h3a3 3 0 003-3V9M6 4a2 2 0 100-.01M15 9a2 2 0 100-.01M9 19a2 2 0 100-.01",
  terminal: "M5 6l5 4-5 4M12 16h7",
  diff: "M9 4v12M9 8H4m5 4H4M15 20V8m0 8h5m-5-4h5",
  chat: "M4 5h16v10H9l-4 4V5z",
  git: "M4 12h6m4 0h6M10 12a2 2 0 104 0 2 2 0 00-4 0z",
  commit: "M4 12h5m6 0h5M12 9a3 3 0 100 6 3 3 0 000-6z",
  bell: "M6 9a6 6 0 1112 0c0 5 2 6 2 6H4s2-1 2-6zM10 20a2 2 0 004 0",
  play: "M7 5l11 7-11 7V5z",
  pause: "M8 5v14M16 5v14",
  plus: "M12 5v14M5 12h14",
  check: "M5 12l5 5L20 6",
  x: "M6 6l12 12M18 6L6 18",
  chevron: "M9 6l6 6-6 6",
  chevronD: "M6 9l6 6 6-6",
  folder: "M4 7a2 2 0 012-2h4l2 2h6a2 2 0 012 2v8a2 2 0 01-2 2H6a2 2 0 01-2-2V7z",
  file: "M7 3h7l5 5v13H7V3z M14 3v5h5",
  clock: "M12 7v5l4 2M12 3a9 9 0 100 18 9 9 0 000-18z",
  bolt: "M13 3L5 14h6l-1 7 8-11h-6l1-7z",
  search: "M11 4a7 7 0 105 12l4 4M11 4a7 7 0 010 14",
  sun: "M12 4V2m0 20v-2m8-8h2M2 12h2m13.7-5.7l1.4-1.4M4.9 19.1l1.4-1.4m0-11.4L4.9 4.9m14.2 14.2l-1.4-1.4M12 8a4 4 0 100 8 4 4 0 000-8z",
  moon: "M20 13A8 8 0 119 3a6 6 0 0011 10z",
  merge: "M7 4v8a4 4 0 004 4h6M7 4a2 2 0 100-.01M17 16a2 2 0 100 .01M7 12a2 2 0 100 .01",
  layers: "M12 3l9 5-9 5-9-5 9-5zM3 13l9 5 9-5",
  grid: "M4 4h7v7H4zM13 4h7v7h-7zM4 13h7v7H4zM13 13h7v7h-7z",
  columns: "M4 4h4v16H4zM10 4h4v16h-4zM16 4h4v16h-4z",
  timeline: "M4 7h16M4 12h10M4 17h13M19 10v4",
  graph: "M6 6a2 2 0 100-.01M18 6a2 2 0 100-.01M12 18a2 2 0 100-.01M7.5 7.5l3 8M16.5 7.5l-3 8",
  dots: "M5 12a1 1 0 100-.01M12 12a1 1 0 100-.01M19 12a1 1 0 100-.01",
  stop: "M7 7h10v10H7z",
  refresh: "M4 11a8 8 0 0114-5l2 2M20 13a8 8 0 01-14 5l-2-2M18 4v4h-4M6 20v-4h4",
  cpu: "M9 3v2m6-2v2M9 19v2m6-2v2M3 9h2m-2 6h2m14-6h2m-2 6h2M7 7h10v10H7z",
  link: "M9 15l6-6M10 7l1-1a4 4 0 016 6l-1 1M14 17l-1 1a4 4 0 01-6-6l1-1",
  flag: "M5 21V4m0 0h11l-2 4 2 4H5",
  spark: "M12 3v4m0 10v4m9-9h-4M7 12H3m12.5-5.5l-2 2m-5 5l-2 2m9 0l-2-2m-5-5l-2-2",
  box: "M3.3 7.5L12 3l8.7 4.5v9L12 21l-8.7-4.5v-9zM3.3 7.5L12 12m0 0l8.7-4.5M12 12v9",
  globe: "M12 3a9 9 0 100 18 9 9 0 000-18zM3 12h18M12 3c2.5 2.5 3.5 6 3.5 9S14.5 18.5 12 21M12 3c-2.5 2.5-3.5 6-3.5 9S9.5 18.5 12 21",
  server: "M4 5h16v5H4zM4 14h16v5H4zM7.5 7.5h.01M7.5 16.5h.01",
  database: "M12 5c4.4 0 8 1.1 8 2.5S16.4 10 12 10 4 8.9 4 7.5 7.6 5 12 5zM4 7.5v9c0 1.4 3.6 2.5 8 2.5s8-1.1 8-2.5v-9M4 12c0 1.4 3.6 2.5 8 2.5s8-1.1 8-2.5",
  cube: "M3.3 7.5L12 3l8.7 4.5v9L12 21l-8.7-4.5v-9zM3.3 7.5L12 12m0 0l8.7-4.5M12 12v9",
  rocket: "M5 15c-1 1-1.5 4-1.5 4s3-.5 4-1.5M9 11a8 8 0 015-7c3 0 4 1 4 4a8 8 0 01-7 5l-2.5.5L8.5 14 9 11zM14.5 9.5h.01",
  archive: "M4 7h16v3H4zM5 10v9h14v-9M10 14h4",
  trash: "M5 7h14M9 7V5h6v2m-8 0v12h10V7M10 11v5m4-5v5",
  rename: "M4 20h16M4 16l9-9 3 3-9 9H4v-3zM13 7l3 3",
  dup: "M9 9h10v10H9zM5 15V5h10",
  ext: "M14 5h5v5M19 5l-7 7M11 5H6a1 1 0 00-1 1v12a1 1 0 001 1h12a1 1 0 001-1v-5",
  push: "M12 19V7m0 0l-5 5m5-5l5 5M5 4h14",
  pr: "M7 5v10m0 4a2 2 0 100 .01M7 5a2 2 0 100 .01M17 9v6m0 4a2 2 0 100 .01M17 9a2 2 0 100-4h-3l2-2m0 4l-2-2",
  stage: "M12 4v10m0 0l4-4m-4 4l-4-4M5 18h14",
  discard: "M4 11a8 8 0 0114-5l2 2M6 20l-2-2 2-2M20 13a8 8 0 01-14 5",
  dotsV: "M12 5a1 1 0 100-.01M12 12a1 1 0 100-.01M12 19a1 1 0 100-.01",
  folderOpen: "M4 7a2 2 0 012-2h4l2 2h6a2 2 0 012 2H4V7zM3 9h18l-2 9a1 1 0 01-1 .8H6a1 1 0 01-1-.8L3 9z",
  enter: "M9 10l-4 4 4 4M5 14h11a4 4 0 004-4V5",
  splitCol: "M4 5h16v14H4zM12 5v14",
  splitRow: "M4 5h16v14H4zM4 12h16",
  panelLeft: "M4 5h16v14H4zM9.5 5v14",
  maximize: "M5 9V5h4M19 9V5h-4M5 15v4h4M19 15v4h-4",
  swap: "M7 8h11l-3-3M17 16H6l3 3",
  settings: "M12 9a3 3 0 100 6 3 3 0 000-6zM19.4 15a1.65 1.65 0 00.33 1.82l.05.05a2 2 0 11-2.83 2.83l-.05-.05a1.65 1.65 0 00-1.82-.33 1.65 1.65 0 00-1 1.51V21a2 2 0 01-4 0v-.08a1.65 1.65 0 00-1-1.51 1.65 1.65 0 00-1.82.33l-.05.05a2 2 0 11-2.83-2.83l.05-.05a1.65 1.65 0 00.33-1.82 1.65 1.65 0 00-1.51-1H3a2 2 0 010-4h.08a1.65 1.65 0 001.51-1 1.65 1.65 0 00-.33-1.82l-.05-.05a2 2 0 112.83-2.83l.05.05a1.65 1.65 0 001.82.33H9a1.65 1.65 0 001-1.51V3a2 2 0 014 0v.08a1.65 1.65 0 001 1.51 1.65 1.65 0 001.82-.33l.05-.05a2 2 0 112.83 2.83l-.05.05a1.65 1.65 0 00-.33 1.82V9a1.65 1.65 0 001.51 1H21a2 2 0 010 4h-.08a1.65 1.65 0 00-1.51 1z",
  lock: "M7 11V8a5 5 0 0110 0v3M5 11h14v9H5zM12 15v2",
  shield: "M12 3l7 3v5c0 4.2-3 7.6-7 9-4-1.4-7-4.8-7-9V6l7-3z",
  volume: "M11 5L6 9H3v6h3l5 4V5zM15.5 8.5a5 5 0 010 7M18.5 5.5a9 9 0 010 13",
};

function Icon({ name, size, cls, style }) {
  const d = ICONS[name] || ICONS.dots;
  const w = size === "sm" ? 13 : size === "lg" ? 18 : 15;
  return (
    <svg className={"icon " + (cls || "")} width={w} height={w} viewBox="0 0 24 24"
      fill="none" stroke="currentColor" strokeWidth="1.7"
      strokeLinecap="round" strokeLinejoin="round" style={style} aria-hidden="true">
      <path d={d} />
    </svg>
  );
}

function StatusDot({ status }) {
  return <span className={"dot " + status} />;
}

function StatusPill({ status, filled }) {
  const m = STATUS_META[status] || STATUS_META.idle;
  return (
    <span className="chip" style={filled ? {
      color: m.color, borderColor: "color-mix(in oklch, " + m.color + ", transparent 60%)",
      background: "color-mix(in oklch, " + m.color + ", transparent 88%)",
    } : { color: m.color }}>
      <StatusDot status={status} />
      <span className="up" style={{ fontSize: "9.5px", letterSpacing: "0.1em" }}>{m.label}</span>
    </span>
  );
}

function fmtDur(sec) {
  if (!sec) return "0s";
  if (sec < 60) return sec + "s";
  const m = Math.floor(sec / 60), s = sec % 60;
  if (m < 60) return m + "m" + (s ? " " + s + "s" : "");
  const h = Math.floor(m / 60);
  return h + "h " + (m % 60) + "m";
}

function fileName(p) {
  const parts = p.replace(/\/$/, "").split("/");
  return parts[parts.length - 1] + (p.endsWith("/") ? "/" : "");
}
function fileDir(p) {
  const parts = p.split("/");
  parts.pop();
  return parts.length ? parts.join("/") + "/" : "";
}

// tiny sparkline (svg)
function Spark({ data, color, w = 60, h = 18 }) {
  const max = Math.max(...data, 1);
  const step = w / (data.length - 1);
  const pts = data.map((v, i) => `${i * step},${h - (v / max) * (h - 2) - 1}`).join(" ");
  return (
    <svg width={w} height={h} style={{ display: "block" }}>
      <polyline points={pts} fill="none" stroke={color || "var(--accent-2)"} strokeWidth="1.5"
        strokeLinejoin="round" strokeLinecap="round" opacity="0.9" />
    </svg>
  );
}

// circular progress ring
function Ring({ value, size = 30, stroke = 3, color }) {
  const r = (size - stroke) / 2;
  const c = 2 * Math.PI * r;
  return (
    <svg width={size} height={size} style={{ transform: "rotate(-90deg)" }}>
      <circle cx={size / 2} cy={size / 2} r={r} fill="none" stroke="var(--hair)" strokeWidth={stroke} />
      <circle cx={size / 2} cy={size / 2} r={r} fill="none"
        stroke={color || "var(--accent)"} strokeWidth={stroke} strokeLinecap="round"
        strokeDasharray={c} strokeDashoffset={c * (1 - value)}
        style={{ transition: "stroke-dashoffset 0.6s ease" }} />
    </svg>
  );
}

function logColor(t) {
  return {
    cmd: "var(--ink)", out: "var(--ink-2)", ok: "var(--st-done)",
    warn: "#f5c451", err: "var(--st-blocked)", sys: "var(--accent-2)",
  }[t] || "var(--ink-2)";
}
function logPrefix(t) {
  return { cmd: "$", out: " ", ok: "✓", warn: "!", err: "✗", sys: "›" }[t] || " ";
}

function toolMeta(id) {
  return (window.AGENT_TOOLS || []).find((t) => t.id === id) || { id, name: id, short: id, accent: "var(--ink-3)" };
}

// little square tool glyph (monogram)
function ToolBadge({ tool, size = 16 }) {
  const m = toolMeta(tool);
  const letter = { claude: "✳", codex: "◆", cursor: "▸", gemini: "✦" }[tool] || m.short[0].toUpperCase();
  return (
    <span title={m.name} style={{
      width: size, height: size, flex: "none", borderRadius: 4,
      display: "grid", placeItems: "center", fontSize: size * 0.62, lineHeight: 1,
      color: m.accent, background: "color-mix(in oklch, " + m.accent + ", transparent 84%)",
      border: "1px solid color-mix(in oklch, " + m.accent + ", transparent 64%)",
    }}>{letter}</span>
  );
}

function projectOf(id) {
  return (window.PROJECTS || []).find((p) => p.id === id);
}

function langOf(path) {
  const ext = (path.split(".").pop() || "").toLowerCase();
  return { ts: "typescript", tsx: "tsx", js: "javascript", jsx: "jsx", json: "json",
    css: "css", tf: "hcl", hcl: "hcl", md: "markdown", sql: "sql", sh: "bash", yml: "yaml", yaml: "yaml" }[ext] || ext || "text";
}

// resolve a file's lines for the viewer:
// explicit content → diff-reconstructed → believable stub
function getFileLines(path, agent) {
  const FC = window.FILE_CONTENTS || {};
  if (FC[path] != null) return FC[path].split("\n").map((s, i) => ({ n: i + 1, s }));
  const d = (window.DIFFS || {})[agent && agent.id];
  if (d && d.file === path) {
    let n = 0;
    return d.hunks.flatMap((h) => h.lines.filter((l) => l.k !== "-").map((l) => { n = l.n; return { n: l.n, s: l.s, add: l.k === "+" }; }));
  }
  return stubLines(path);
}

function stubLines(path) {
  const name = fileName(path).replace(/\.[^.]+$/, "");
  const ext = (path.split(".").pop() || "").toLowerCase();
  const cap = name.replace(/(^|[-_])(\w)/g, (_, __, c) => c.toUpperCase());
  let body;
  if (ext === "json") body = `{\n  "name": "${name}",\n  "version": "1.0.0"\n}`;
  else if (ext === "css") body = `.${name} {\n  display: flex;\n  gap: var(--space-2);\n}`;
  else if (ext === "tf") body = `resource "aws_${name}" "this" {\n  name = var.name\n  tags = { env = var.env }\n}`;
  else if (ext === "md") body = `# ${cap}\n\nDocumentation for ${name}.`;
  else if (ext === "sql") body = `-- ${name}\nCREATE TABLE ${name} (\n  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),\n  created_at timestamptz NOT NULL DEFAULT now()\n);`;
  else if (ext === "tsx" || ext === "jsx") body = `import React from 'react';\n\nexport function ${cap}() {\n  return <div className="${name}" />;\n}`;
  else body = `// ${path}\nimport { logger } from '../lib/log';\n\nexport function ${name.replace(/[^a-zA-Z0-9]/g, "")}() {\n  logger.debug('${name}');\n  return null;\n}`;
  return body.split("\n").map((s, i) => ({ n: i + 1, s }));
}

const KW = new RegExp("\\b(" + ["import","from","export","default","const","let","var","function","return","async","await","if","else","for","while","class","extends","new","try","catch","throw","type","interface","public","private","this","void","null","undefined","true","false","resource","module","variable","required_providers","describe","it","expect"].join("|") + ")\\b", "g");

// very light tokenizer → array of React spans (no external deps)
function highlight(line, lang) {
  if (line == null) return " ";
  const trimmed = line.trimStart();
  if (trimmed.startsWith("//") || trimmed.startsWith("#") || trimmed.startsWith("--") || trimmed.startsWith("*")) {
    return <span style={{ color: "var(--ink-4)", fontStyle: "italic" }}>{line || " "}</span>;
  }
  const out = [];
  // split by strings first, then keyword-highlight the rest
  const re = /("[^"]*"|'[^']*'|`[^`]*`)/g;
  let last = 0, m, key = 0;
  const pushPlain = (txt) => {
    let li = 0, km;
    KW.lastIndex = 0;
    while ((km = KW.exec(txt))) {
      if (km.index > li) out.push(<span key={key++}>{txt.slice(li, km.index)}</span>);
      out.push(<span key={key++} style={{ color: "var(--accent-2)" }}>{km[0]}</span>);
      li = km.index + km[0].length;
    }
    if (li < txt.length) out.push(<span key={key++}>{txt.slice(li)}</span>);
  };
  while ((m = re.exec(line))) {
    if (m.index > last) pushPlain(line.slice(last, m.index));
    out.push(<span key={key++} style={{ color: "var(--code-add-ink)" }}>{m[0]}</span>);
    last = m.index + m[0].length;
  }
  if (last < line.length) pushPlain(line.slice(last));
  return out.length ? out : " ";
}

// ---------------------------------------------------------------- skeletons
// Shimmer placeholder primitive. `seed` lets us vary widths so a list of
// skeleton rows doesn't look mechanically uniform.
function Skeleton({ w, h = 10, r, style }) {
  return <span className="skel" style={{ width: w == null ? "100%" : w, height: h, borderRadius: r, ...style }} />;
}

// matches a sidebar AgentRow: status dot + name + tool, then branch line
function SkelAgentRow({ wide }) {
  return (
    <div style={{ display: "flex", flexDirection: "column", gap: 5, padding: "7px 10px 8px", margin: "1px 8px 1px 14px" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 7 }}>
        <Skeleton w={8} h={8} r={"50%"} />
        <Skeleton w={wide ? 96 : 74} h={9} r={3} />
        <Skeleton w={14} h={14} r={4} style={{ marginLeft: "auto" }} />
      </div>
      <div style={{ display: "flex", alignItems: "center", gap: 6, paddingLeft: 15 }}>
        <Skeleton w={11} h={11} r={3} />
        <Skeleton w={wide ? 110 : 88} h={8} r={3} />
      </div>
    </div>
  );
}

// matches a sidebar ProjectGroup header + a few agent rows
function SidebarSkeleton({ groups = 3 }) {
  const counts = [3, 2, 2, 1];
  const widths = [120, 96, 138, 84];
  return (
    <div className="skel-fade" aria-busy="true" aria-label="Loading projects">
      {Array.from({ length: groups }).map((_, g) => (
        <div key={g} style={{ marginBottom: 2 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "7px 10px", margin: "0 6px" }}>
            <Skeleton w={11} h={11} r={3} />
            <Skeleton w={19} h={19} r={5} />
            <Skeleton w={widths[g % widths.length]} h={11} r={4} />
            <Skeleton w={14} h={9} r={3} style={{ marginLeft: "auto" }} />
          </div>
          <div style={{ position: "relative" }}>
            <span style={{ position: "absolute", left: 21, top: 0, bottom: 4, width: 1, background: "var(--hair)" }} />
            {Array.from({ length: counts[g % counts.length] }).map((_, i) => <SkelAgentRow key={i} wide={(g + i) % 3 === 0} />)}
          </div>
        </div>
      ))}
    </div>
  );
}

// matches an Overview AgentCard
function AgentCardSkeleton() {
  return (
    <div className="surface" style={{ padding: 14, display: "flex", flexDirection: "column", gap: 11 }}>
      <div style={{ display: "flex", alignItems: "flex-start", gap: 10 }}>
        <Skeleton w={36} h={36} r={"50%"} />
        <div style={{ flex: 1, minWidth: 0, display: "flex", flexDirection: "column", gap: 7 }}>
          <div style={{ display: "flex", alignItems: "center", gap: 7 }}>
            <Skeleton w={120} h={13} r={4} />
            <Skeleton w={52} h={15} r={999} />
          </div>
          <Skeleton w={150} h={9} r={3} />
        </div>
      </div>
      <div style={{ display: "flex", flexDirection: "column", gap: 6 }}>
        <Skeleton w={"100%"} h={9} r={3} />
        <Skeleton w={"82%"} h={9} r={3} />
      </div>
      <Skeleton w={"100%"} h={52} r={"var(--r-sm)"} />
      <div style={{ display: "flex", alignItems: "center", gap: 10 }}>
        <Skeleton w={34} h={9} r={3} /><Skeleton w={28} h={9} r={3} /><Skeleton w={28} h={9} r={3} />
        <Skeleton w={40} h={9} r={3} style={{ marginLeft: "auto" }} />
      </div>
      <div style={{ display: "flex", gap: 6 }}>
        <Skeleton w={"100%"} h={28} r={"var(--r-sm)"} />
        <Skeleton w={64} h={28} r={"var(--r-sm)"} />
      </div>
    </div>
  );
}

function OverviewSkeleton({ count = 6 }) {
  return (
    <div className="skel-fade" aria-busy="true" aria-label="Loading agents" style={{
      display: "grid", gap: 14, padding: 18,
      gridTemplateColumns: "repeat(auto-fill, minmax(320px, 1fr))", alignContent: "start",
    }}>
      {Array.from({ length: count }).map((_, i) => <AgentCardSkeleton key={i} />)}
    </div>
  );
}

// a skeleton <tr> for the dev-console tables. `cols` = array of pixel widths.
function TableRowSkeleton({ cols, lead }) {
  return (
    <tr aria-hidden="true">
      {cols.map((w, i) => (
        <td key={i} style={{ height: 32 }}>
          {i === 0 && lead
            ? <span style={{ display: "flex", alignItems: "center", gap: 8 }}><Skeleton w={11} h={11} r={3} /><Skeleton w={w} h={10} r={3} /></span>
            : <Skeleton w={w} h={10} r={3} style={{ display: "inline-block", verticalAlign: "middle" }} />}
        </td>
      ))}
    </tr>
  );
}

Object.assign(window, {
  Icon, StatusDot, StatusPill, STATUS_META, fmtDur, fileName, fileDir,
  Spark, Ring, logColor, logPrefix, ToolBadge, toolMeta, projectOf,
  getFileLines, highlight, langOf,
  Skeleton, SidebarSkeleton, AgentCardSkeleton, OverviewSkeleton, TableRowSkeleton,
});