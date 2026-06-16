/* global React, Icon, fileName, fileDir, langOf, highlight */
// Orrery — shared git diff-view primitives (reused by every git surface).
// The hunk renderer matches the existing workspace diff exactly: same line
// layout, gutter widths, +/- inks and hunk-meta band.

const { useState: useStateD } = React;
const GA = () => window.GIT.authors;

// ---- author avatar (initials chip, colored per author) ----
function AuthorAvatar({ id, size = 18 }) {
  const a = GA()[id] || { short: "?", color: "var(--ink-3)" };
  return (
    <span title={a.name} style={{
      width: size, height: size, flex: "none", borderRadius: a.kind === "agent" ? 5 : "50%",
      display: "grid", placeItems: "center", fontSize: size * 0.46, fontWeight: 700, lineHeight: 1,
      color: a.color, background: "color-mix(in oklch, " + a.color + ", transparent 84%)",
      border: "1px solid color-mix(in oklch, " + a.color + ", transparent 60%)",
    }}>{a.short}</span>
  );
}
function authorName(id) { return (GA()[id] || {}).name || id; }

// ---- sha chip ----
function Sha({ sha, onClick, dim }) {
  return (
    <span className="chip tnum" onClick={onClick}
      style={{ fontSize: 9.5, padding: "0 6px", letterSpacing: "0.02em", cursor: onClick ? "pointer" : "default",
        color: dim ? "var(--ink-3)" : "var(--ink-2)" }}
      onMouseEnter={onClick ? (e) => { e.currentTarget.style.color = "var(--accent-2)"; e.currentTarget.style.borderColor = "color-mix(in oklch, var(--accent-2), transparent 55%)"; } : undefined}
      onMouseLeave={onClick ? (e) => { e.currentTarget.style.color = dim ? "var(--ink-3)" : "var(--ink-2)"; e.currentTarget.style.borderColor = "var(--hair)"; } : undefined}>
      {sha}
    </span>
  );
}

// ---- per-file state badge (A/M/D/R) ----
function RStateBadge({ state }) {
  const c = state === "A" ? "var(--code-add-ink)" : state === "D" ? "var(--code-del-ink)" : state === "R" ? "var(--st-waiting)" : "var(--accent-2)";
  const bg = state === "A" ? "var(--code-add-bg)" : state === "D" ? "var(--code-del-bg)" : state === "R" ? "color-mix(in oklch, var(--st-waiting), transparent 86%)" : "color-mix(in oklch, var(--accent-2), transparent 86%)";
  return (
    <span style={{ flex: "none", width: 15, height: 15, borderRadius: 3, display: "grid", placeItems: "center",
      fontSize: 9, fontWeight: 700, color: c, background: bg }}>{state}</span>
  );
}

function AddDel({ add, del, gap = 4 }) {
  return (
    <span className="tnum" style={{ display: "flex", gap, fontSize: 9.5, flex: "none" }}>
      {add > 0 && <span style={{ color: "var(--code-add-ink)" }}>+{add}</span>}
      {del > 0 && <span style={{ color: "var(--code-del-ink)" }}>−{del}</span>}
    </span>
  );
}

// =========================================================================
// HunkRows — THE diff renderer. Optional per-line render-prefix (gutter
// checkboxes for partial-commit) and per-line click.
// =========================================================================
function HunkRows({ hunks, lang, lead, onLineClick, lineStyle }) {
  return hunks.map((h, hi) => (
    <div key={hi}>
      <div style={{ display: "flex", alignItems: "center", padding: "5px 14px", fontSize: 11, color: "var(--accent-2)",
        background: "color-mix(in oklch, var(--accent-2), transparent 93%)", borderTop: hi ? "1px solid var(--hair)" : "none" }}>
        {lead && lead.hunkHead ? lead.hunkHead(h, hi) : null}
        <span className="tnum" style={{ opacity: 0.95 }}>{h.meta}</span>
      </div>
      {h.lines.map((ln, li) => {
        const k = ln.k;
        const bg = lineStyle ? lineStyle(h, ln, li) : (k === "+" ? "var(--code-add-bg)" : k === "-" ? "var(--code-del-bg)" : "transparent");
        return (
          <div key={li} onClick={onLineClick ? () => onLineClick(h, ln, li) : undefined}
            style={{ display: "flex", fontSize: 12, lineHeight: 1.7, background: bg, cursor: onLineClick ? "pointer" : "default" }}>
            {lead && lead.lineHead ? lead.lineHead(h, ln, li) : null}
            <span className="tnum" style={{ width: 44, flex: "none", textAlign: "right", padding: "0 10px 0 0", color: "var(--ink-4)", userSelect: "none" }}>{ln.n}</span>
            <span style={{ width: 16, flex: "none", textAlign: "center", userSelect: "none",
              color: k === "+" ? "var(--code-add-ink)" : k === "-" ? "var(--code-del-ink)" : "var(--ink-4)" }}>{k}</span>
            <span style={{ flex: 1, whiteSpace: "pre-wrap", wordBreak: "break-word", paddingRight: 14,
              color: k === "+" ? "var(--code-add-ink)" : k === "-" ? "var(--code-del-ink)" : "var(--ink-2)" }}>{ln.s || " "}</span>
          </div>
        );
      })}
    </div>
  ));
}

// ---- file header band (sticky) used above a diff ----
function DiffFileHeader({ path, lang, right, left }) {
  return (
    <div style={{ position: "sticky", top: 0, display: "flex", alignItems: "center", gap: 8, padding: "8px 14px",
      background: "var(--panel)", borderBottom: "1px solid var(--hair)", fontSize: 11.5, zIndex: 2, flex: "none" }}>
      {left}
      <Icon name="file" size="sm" style={{ color: "var(--ink-3)" }} />
      <span style={{ color: "var(--ink-4)" }}>{fileDir(path)}</span>
      <span style={{ marginLeft: -6 }}>{fileName(path)}</span>
      {right}
      <span className="chip" style={{ marginLeft: "auto", fontSize: 9.5 }}>{lang || langOf(path)}</span>
    </div>
  );
}

// ---- a complete scrollable diff for one file ----
function FileDiff({ path, diff, headerRight }) {
  if (!diff) return <div style={{ display: "grid", placeItems: "center", color: "var(--ink-4)", fontSize: 12, padding: 30 }}>no textual diff</div>;
  return (
    <div className="scroll-y" style={{ flex: 1, background: "var(--bg)", minHeight: 0 }}>
      <DiffFileHeader path={path} lang={diff.lang} right={headerRight} />
      <HunkRows hunks={diff.hunks} lang={diff.lang} />
    </div>
  );
}

// =========================================================================
// fallback diff synthesizer for (sha,path) pairs without an explicit diff.
// keeps the long tail of history clickable without authoring every hunk.
// =========================================================================
function synthDiff(file) {
  const lang = langOf(file.path);
  const base = file.path.split("/").pop().replace(/\.[^.]+$/, "");
  const cap = base.replace(/(^|[-_])(\w)/g, (_, __, c) => c.toUpperCase());
  if (file.state === "A") {
    const body = lang === "json"
      ? ['{', '  "name": "' + base + '",', '  "version": "1.0.0"', '}']
      : lang === "sql"
      ? ["CREATE TABLE " + base + " (", "  id uuid PRIMARY KEY DEFAULT gen_random_uuid(),", "  created_at timestamptz NOT NULL DEFAULT now()", ");"]
      : lang === "hcl"
      ? ['resource "aws_' + base + '" "this" {', "  name = var.name", '  tags = { env = var.env }', "}"]
      : ["import { logger } from '../lib/log';", "", "export function " + cap.replace(/[^a-zA-Z0-9]/g, "") + "() {", "  logger.debug('" + base + "');", "}"];
    const show = body.slice(0, Math.max(2, Math.min(body.length, file.add)));
    return { lang, hunks: [{ meta: "@@ -0,0 +1," + file.add + " @@ new file", lines: show.map((s, i) => ({ k: "+", n: i + 1, s })) }] };
  }
  if (file.state === "D") {
    return { lang, hunks: [{ meta: "@@ -1," + file.del + " +0,0 @@ deleted", lines: [
      { k: "-", n: 1, s: "// " + file.path + " removed" },
      { k: "-", n: 2, s: "export {};" },
    ] }] };
  }
  // modified — a believable small edit
  return { lang, hunks: [{ meta: "@@ -8,6 +8,7 @@ " + base, lines: [
    { k: " ", n: 8, s: "export function " + cap.replace(/[^a-zA-Z0-9]/g, "") + "() {" },
    { k: "-", n: 9, s: "  return legacy();" },
    { k: "+", n: 9, s: "  return next();   // " + file.add + " insertions, " + file.del + " deletions" },
    { k: " ", n: 10, s: "}" },
  ] }] };
}

function diffFor(sha, file) {
  const explicit = window.GIT.commitDiffs[sha + "/" + file.path];
  return explicit || synthDiff(file);
}

// =========================================================================
// ChangedFiles — selectable file list (flat + tree), with ± and state badge.
// =========================================================================
function rBuildTree(files) {
  const root = {};
  files.forEach((f) => {
    const parts = f.path.split("/");
    let cur = root;
    parts.forEach((p, i) => {
      const leaf = i === parts.length - 1;
      cur[p] = cur[p] || { name: p, leaf, path: leaf ? f.path : parts.slice(0, i + 1).join("/"), file: leaf ? f : null, children: {} };
      cur = cur[p].children;
    });
  });
  const walk = (obj) => Object.values(obj).sort((a, b) => (a.leaf === b.leaf) ? a.name.localeCompare(b.name) : a.leaf ? 1 : -1)
    .map((n) => n.leaf ? n : { ...n, dir: true, children: walk(n.children) });
  return walk(root);
}

function FileRowFlat({ f, active, onSelect }) {
  return (
    <div onClick={() => onSelect(f.path)}
      style={{ display: "flex", alignItems: "center", gap: 8, padding: "6px 12px", cursor: "pointer", margin: "1px 6px", borderRadius: "var(--r-sm)", background: active ? "var(--panel-3)" : "transparent" }}
      onMouseEnter={(e) => { if (!active) e.currentTarget.style.background = "var(--panel-2)"; }}
      onMouseLeave={(e) => { if (!active) e.currentTarget.style.background = "transparent"; }}>
      <RStateBadge state={f.state} />
      <div style={{ flex: 1, minWidth: 0 }}>
        <div style={{ fontSize: 11.5, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", color: "var(--ink)" }}>{fileName(f.path)}</div>
        {fileDir(f.path) && <div style={{ fontSize: 9.5, color: "var(--ink-4)", overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap" }}>{fileDir(f.path)}</div>}
      </div>
      <AddDel add={f.add} del={f.del} />
    </div>
  );
}

function DiffTreeRows({ nodes, depth, selPath, onSelect }) {
  return nodes.map((node) => {
    const pad = 8 + depth * 13;
    if (node.dir) {
      return (
        <div key={node.path}>
          <div style={{ display: "flex", alignItems: "center", gap: 6, padding: "4px 8px", paddingLeft: pad }}>
            <Icon name="folderOpen" size="sm" style={{ width: 13, height: 13, color: "var(--ink-4)", flex: "none" }} />
            <span style={{ fontSize: 11, color: "var(--ink-3)" }}>{node.name}</span>
          </div>
          <DiffTreeRows nodes={node.children} depth={depth + 1} selPath={selPath} onSelect={onSelect} />
        </div>
      );
    }
    const f = node.file, active = selPath === node.path;
    return (
      <div key={node.path} onClick={() => onSelect(node.path)}
        style={{ display: "flex", alignItems: "center", gap: 7, padding: "4px 10px", paddingLeft: pad + 4, cursor: "pointer", margin: "1px 6px", borderRadius: "var(--r-sm)", background: active ? "var(--panel-3)" : "transparent" }}
        onMouseEnter={(e) => { if (!active) e.currentTarget.style.background = "var(--panel-2)"; }}
        onMouseLeave={(e) => { if (!active) e.currentTarget.style.background = "transparent"; }}>
        <RStateBadge state={f.state} />
        <span style={{ flex: 1, fontSize: 11.5, overflow: "hidden", textOverflow: "ellipsis", whiteSpace: "nowrap", color: active ? "var(--ink)" : "var(--ink-2)" }}>{node.name}</span>
        <AddDel add={f.add} del={f.del} />
      </div>
    );
  });
}

function DiffFileList({ files, selPath, onSelect, title, defaultTree }) {
  const [tree, setTree] = useStateD(!!defaultTree);
  const tot = files.reduce((s, f) => ({ a: s.a + f.add, d: s.d + f.del }), { a: 0, d: 0 });
  return (
    <div style={{ display: "flex", flexDirection: "column", minHeight: 0, height: "100%" }}>
      <div style={{ display: "flex", alignItems: "center", gap: 8, padding: "8px 10px 8px 14px", borderBottom: "1px solid var(--hair)", flex: "none" }}>
        <span className="up" style={{ fontSize: 9, color: "var(--ink-3)" }}>{title || "Changed"} · {files.length}</span>
        <AddDel add={tot.a} del={tot.d} />
        <div style={{ marginLeft: "auto", display: "flex", gap: 2, padding: 2, background: "var(--panel-2)", border: "1px solid var(--hair)", borderRadius: "var(--r-sm)" }}>
          {[{ k: true, icon: "graph", label: "Tree" }, { k: false, icon: "dots", label: "Flat" }].map((o) => (
            <button key={o.label} className="btn" onClick={() => setTree(o.k)} title={o.label + " view"}
              style={{ padding: "3px 7px", borderRadius: 4, gap: 5, fontSize: 10,
                background: tree === o.k ? "var(--panel-3)" : "transparent", color: tree === o.k ? "var(--ink)" : "var(--ink-3)",
                boxShadow: tree === o.k ? "0 0 0 1px var(--hair-2)" : "none" }}>
              <Icon name={o.icon} size="sm" style={{ width: 12, height: 12, color: tree === o.k ? "var(--accent)" : "inherit" }} />{o.label}
            </button>
          ))}
        </div>
      </div>
      <div className="scroll-y" style={{ flex: 1, padding: "6px 0" }}>
        {tree
          ? <DiffTreeRows nodes={rBuildTree(files)} depth={0} selPath={selPath} onSelect={onSelect} />
          : files.map((f) => <FileRowFlat key={f.path} f={f} active={selPath === f.path} onSelect={onSelect} />)}
      </div>
    </div>
  );
}

Object.assign(window, {
  AuthorAvatar, authorName, Sha, RStateBadge, AddDel,
  HunkRows, DiffFileHeader, FileDiff, DiffFileList, diffFor, synthDiff,
});
