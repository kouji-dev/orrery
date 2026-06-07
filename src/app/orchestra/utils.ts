// ORCHESTRA shared helpers, icon paths, status metadata
import { AGENT_TOOLS } from "./data";
import { AgentStatus, LogKind } from "./models";

// ---- icon set (stroke, currentColor) ----
export const ICONS: Record<string, string> = {
  agent: "M12 3l7 4v5c0 4.4-3 7.6-7 9-4-1.4-7-4.6-7-9V7l7-4z",
  sparkles: "M12 3l1.6 4.4 4.4 1.6-4.4 1.6L12 15l-1.6-4.4L6 9l4.4-1.6L12 3zM18.5 14l.6 1.7 1.7.6-1.7.6-.6 1.7-.6-1.7-1.7-.6 1.7-.6.6-1.7z",
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
  // question/help: a circle with a "?" stem + dot — for question-type notifications
  question: "M12 3a9 9 0 100 18 9 9 0 000-18zM9.2 9.2a2.8 2.8 0 015.5.8c0 1.9-2.8 2.5-2.8 4M12 17h.01",
  // shield: a guarded action — for command/permission notifications
  shield: "M12 3l7 3v5c0 4.4-3 7.6-7 9-4-1.4-7-4.6-7-9V6l7-3z",
  // split-right: frame with a vertical divider (new pane to the right)
  splitCol: "M4 5h16v14H4zM12 5v14",
  // split-down: frame with a horizontal divider (new pane below)
  splitRow: "M4 5h16v14H4zM4 12h16",
  // left panel: frame with a narrow left column highlighted (sidebar toggle)
  panelLeft: "M4 5h16v14H4zM9 5v14",
  // swap: two arrows trading places (swap pane content)
  swap: "M7 4L4 7l3 3M4 7h13M17 20l3-3-3-3M20 17H7",
};

export interface StatusMeta {
  label: string;
  color: string;
}
export const STATUS_META: Record<AgentStatus, StatusMeta> = {
  running: { label: "running", color: "var(--st-running)" },
  blocked: { label: "blocked", color: "var(--st-blocked)" },
  waiting: { label: "waiting", color: "var(--st-waiting)" },
  done: { label: "done", color: "var(--st-done)" },
  idle: { label: "idle", color: "var(--st-idle)" },
  queued: { label: "queued", color: "var(--st-idle)" },
};

export function fmtDur(sec: number): string {
  if (!sec) return "0s";
  if (sec < 60) return sec + "s";
  const m = Math.floor(sec / 60),
    s = sec % 60;
  if (m < 60) return m + "m" + (s ? " " + s + "s" : "");
  const h = Math.floor(m / 60);
  return h + "h " + (m % 60) + "m";
}

export function fileName(p: string): string {
  const parts = p.replace(/\/$/, "").split("/");
  return parts[parts.length - 1] + (p.endsWith("/") ? "/" : "");
}
export function fileDir(p: string): string {
  const parts = p.split("/");
  parts.pop();
  return parts.length ? parts.join("/") + "/" : "";
}

export function logColor(t: LogKind | string): string {
  return (
    (
      {
        cmd: "var(--ink)",
        out: "var(--ink-2)",
        ok: "var(--st-done)",
        warn: "#f5c451",
        err: "var(--st-blocked)",
        sys: "var(--accent-2)",
      } as Record<string, string>
    )[t] || "var(--ink-2)"
  );
}
export function logPrefix(t: LogKind | string): string {
  return (
    ({ cmd: "$", out: " ", ok: "✓", warn: "!", err: "✗", sys: "›" } as Record<string, string>)[t] || " "
  );
}

// Matches CSI (\x1b[ … final), OSC (\x1b] … BEL/ST) and 2-char escape sequences.
const ANSI_RE = /\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[@-Z\\-_]/g;

/** Strip ANSI / escape sequences from a PTY chunk for plain-text display. */
export function stripAnsi(s: string): string {
  return s.replace(ANSI_RE, "");
}

// A single ANSI/escape token (CSI / OSC / 2-char) OR one plain character. We
// walk the chunk token-by-token so the cursor/erase CSI sequences can be acted
// on (not just deleted) — a deleted clear-screen would otherwise leave a TUI's
// previous frame stacked on top of the new one, which is the garbage we're
// trying to avoid in the preview.
const TOKEN_RE = /\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[@-Z\\-_]|[\s\S]/g;
// A CSI sequence captured into [, params, final] so we can interpret it.
const CSI_RE = /^\x1b\[([0-9;?]*)([@-~])$/;

// Drop OSC titles and 2-char escapes up front; CSI sequences are kept so the
// folder below can interpret cursor/erase moves token-by-token.
function stripUnactionable(s: string): string {
  return s.replace(/\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[@-Z\\-_]/g, "");
}

/**
 * Fold a raw PTY chunk into a rolling plain-text screen of at most `max` lines,
 * interpreting the cursor/erase control sequences a TUI uses to redraw in place
 * rather than just stripping them. This keeps a redraw (claude/codex/gemini
 * full-frame repaint) from leaving stale rows stacked behind the new frame.
 *
 * Lightweight, NOT a full terminal grid — it tracks a current row/column and
 * handles: \n, \r, \t, cursor up/down/home/position (A/B/H/f), horizontal
 * absolute (G), erase-in-line (K) and erase-in-display (J / 1J / 2J / 3J).
 * Anything else (colors, SGR, OSC titles, etc.) is dropped. Used only for the
 * compact overview mini-term — the full xterm view renders the raw stream.
 */
export function appendPtyTail(prev: string[], chunk: string, max = 60): string[] {
  const lines = prev.length ? prev.slice() : [""];
  let row = lines.length - 1; // cursor row (index into `lines`)
  let col = lines[row].length; // cursor column within that row

  const ensureRow = () => {
    if (row < 0) row = 0;
    while (lines.length <= row) lines.push("");
  };
  const writeAt = (text: string) => {
    ensureRow();
    const line = lines[row];
    // overwrite-in-place (pad with spaces if the cursor is past the line end)
    const head = col <= line.length ? line.slice(0, col) : line.padEnd(col);
    lines[row] = head + text + line.slice(col + text.length);
    col += text.length;
  };

  const tokens = stripUnactionable(chunk).match(TOKEN_RE) ?? [];
  for (const tok of tokens) {
    if (tok === "\n") {
      row += 1;
      col = 0;
      ensureRow();
      continue;
    }
    if (tok === "\r") {
      col = 0;
      continue;
    }
    if (tok === "\t") {
      writeAt("  ");
      continue;
    }
    if (tok.length === 1) {
      if (tok >= " ") writeAt(tok);
      continue; // drop remaining stray control chars
    }
    // multi-char: a CSI we may need to act on (others stripped upstream)
    const m = CSI_RE.exec(tok);
    if (!m) continue;
    const params = m[1];
    const final = m[2];
    const n = parseInt(params, 10);
    switch (final) {
      case "A": // cursor up
        row = Math.max(0, row - (isNaN(n) ? 1 : n));
        break;
      case "B": // cursor down
        row += isNaN(n) ? 1 : n;
        ensureRow();
        break;
      case "G": // cursor horizontal absolute (1-based)
        col = isNaN(n) ? 0 : Math.max(0, n - 1);
        break;
      case "H":
      case "f": {
        // cursor position "row;col" (1-based); bare = home (top-left)
        const [r, c] = params.split(";");
        row = r ? Math.max(0, parseInt(r, 10) - 1) : 0;
        col = c ? Math.max(0, parseInt(c, 10) - 1) : 0;
        ensureRow();
        break;
      }
      case "K": // erase in line: 0=to end (default), 1=to start, 2=whole
        ensureRow();
        if (params === "1") lines[row] = " ".repeat(col) + lines[row].slice(col);
        else if (params === "2") lines[row] = "";
        else lines[row] = lines[row].slice(0, col);
        break;
      case "J": // erase in display
        if (params === "2" || params === "3") {
          // clear whole screen → reset the buffer (kills a stale TUI frame)
          lines.length = 0;
          lines.push("");
          row = 0;
          col = 0;
        } else if (params === "1") {
          for (let i = 0; i < row; i++) lines[i] = "";
          ensureRow();
          lines[row] = lines[row].slice(col);
        } else {
          // 0 (default): erase from cursor to end of screen
          ensureRow();
          lines[row] = lines[row].slice(0, col);
          lines.length = row + 1;
        }
        break;
      default:
        break; // SGR (m) and friends: nothing to do
    }
  }
  return lines.length > max ? lines.slice(lines.length - max) : lines;
}

// ---- terminal-title status detection ----
// CLI coding agents encode live state in their OSC window title: an animated
// braille spinner (or tool glyph / keyword) while working, a distinct glyph
// when idle or waiting on the user. Reading the title lets us tell "actively
// working" from "waiting for input" without the agent reporting anything.
export type TitleStatus = "working" | "permission" | "idle";

const BRAILLE_SPINNER_RE = /[⠀-⣿]/; // ⠋⠙⠹… spinner frames
// "working" | "thinking" | "running" | "generating", but not inside another
// word ("reworking") or a path ("~/codex/working").
const WORKING_KEYWORDS_RE = /(?<![\w./\\-])(working|thinking|running|generating)(?![\w-])/i;
const GEMINI_WORKING = "✦"; // ✦
const GEMINI_SILENT_WORKING = "⏲"; // ⏲
const PERMISSION_GLYPH = "✋"; // ✋ (gemini permission prompt)
const CLAUDE_IDLE = "✳"; // ✳ (claude idle prefix)
const GEMINI_IDLE = "◇"; // ◇

/**
 * Classify a terminal title. Returns null when the title carries no recognized
 * signal (e.g. a plain cwd) so the caller can fall back to output activity.
 */
export function detectTitleStatus(title: string): TitleStatus | null {
  if (!title) return null;
  if (title.includes(PERMISSION_GLYPH)) return "permission";
  if (
    BRAILLE_SPINNER_RE.test(title) ||
    title.includes(GEMINI_WORKING) ||
    title.includes(GEMINI_SILENT_WORKING) ||
    WORKING_KEYWORDS_RE.test(title)
  ) {
    return "working";
  }
  if (title.includes(CLAUDE_IDLE) || title.includes(GEMINI_IDLE)) return "idle";
  return null;
}

// ---- prompt classification (notification typing) ----
// Heuristic: does the scraped terminal prompt read like a yes/no permission
// request (→ Accept/Reject actions) vs an open question (→ open the terminal)?
const PERMISSION_RE =
  /\b(y\/n|\[y\/n]|\(y\/n\)|yes\/no|proceed\??|do you want|allow\b|permission|approve|grant\b|confirm\b|continue\?|press y|1\.?\s*yes)\b|❯\s*1\.?\s*yes/i;

export function isPermissionPrompt(text: string): boolean {
  return PERMISSION_RE.test(text);
}

/**
 * Does the scraped terminal tail read like the agent is blocked on the user?
 * Catches y/n permission prompts and trailing questions — the signal CLI agents
 * that don't set a "waiting" terminal title (claude/codex) still leave in output.
 */
export function isAwaitingInput(tail: string): boolean {
  if (!tail) return false;
  if (isPermissionPrompt(tail)) return true;
  const last = tail.split("\n").map((l) => l.trim()).filter(Boolean).pop() ?? "";
  return /\?\s*$/.test(last); // a trailing question
}

export function toolMeta(id: string) {
  return (
    AGENT_TOOLS.find((t) => t.id === id) || {
      id,
      name: id,
      short: id,
      accent: "var(--ink-3)",
      models: [],
      effort: false as const,
    }
  );
}

export const TOOL_GLYPH: Record<string, string> = {
  claude: "✳",
  codex: "◆",
  cursor: "▸",
  gemini: "✦",
};

export function hexRgb(hex: string): string {
  const h = hex.replace("#", "");
  const n = parseInt(h.length === 3 ? h.split("").map((c) => c + c).join("") : h, 16);
  return [(n >> 16) & 255, (n >> 8) & 255, n & 255].join(", ");
}

// color-mix helper to keep templates terse
export function mix(color: string, transparentPct: number): string {
  return `color-mix(in oklch, ${color}, transparent ${transparentPct}%)`;
}

// ---- file language tag (diff header) ----
// Human-readable language label from a file's extension, for the diff view's
// language tag. Common extensions map to friendly names; anything unknown falls
// back to the raw extension uppercased (e.g. "toml" -> "TOML").
const LANG_LABELS: Record<string, string> = {
  ts: "typescript",
  mts: "typescript",
  cts: "typescript",
  tsx: "typescript react",
  js: "javascript",
  mjs: "javascript",
  cjs: "javascript",
  jsx: "javascript react",
  rs: "rust",
  py: "python",
  rb: "ruby",
  go: "go",
  java: "java",
  kt: "kotlin",
  swift: "swift",
  c: "c",
  h: "c",
  cpp: "c++",
  cc: "c++",
  cxx: "c++",
  hpp: "c++",
  cs: "c#",
  php: "php",
  sh: "shell",
  bash: "shell",
  zsh: "shell",
  ps1: "powershell",
  html: "html",
  htm: "html",
  css: "css",
  scss: "scss",
  sass: "sass",
  less: "less",
  json: "json",
  jsonc: "json",
  yaml: "yaml",
  yml: "yaml",
  toml: "toml",
  xml: "xml",
  md: "markdown",
  markdown: "markdown",
  mdx: "markdown",
  sql: "sql",
  tf: "terraform",
  hcl: "hcl",
  dockerfile: "dockerfile",
  vue: "vue",
  svelte: "svelte",
};

export function langTag(path: string): string {
  const name = path.replace(/\/$/, "").split("/").pop() ?? "";
  if (/^dockerfile$/i.test(name)) return "dockerfile";
  const dot = name.lastIndexOf(".");
  if (dot < 0) return ""; // no extension
  const ext = name.slice(dot + 1).toLowerCase();
  return LANG_LABELS[ext] ?? ext.toUpperCase();
}

// extension → the canonical grammar tag understood by code-lang's `loadLangExt`
// (a key in its LANGS map). Distinct from LANG_LABELS, which is display-only.
const LANG_ID: Record<string, string> = {
  ts: "javascript", mts: "javascript", cts: "javascript", tsx: "javascript",
  js: "javascript", mjs: "javascript", cjs: "javascript", jsx: "javascript",
  json: "json", jsonc: "json", json5: "json",
  css: "css", scss: "sass", sass: "sass", less: "less",
  html: "html", htm: "html", xhtml: "html",
  xml: "xml", svg: "xml", xsd: "xml", xsl: "xml", plist: "xml", wsdl: "xml",
  md: "markdown", markdown: "markdown", mdx: "markdown",
  rs: "rust", py: "python", pyw: "python", pyi: "python",
  java: "java", yaml: "yaml", yml: "yaml", toml: "toml", sql: "sql",
  c: "cpp", h: "cpp", cpp: "cpp", cc: "cpp", cxx: "cpp", hpp: "cpp", hh: "cpp", hxx: "cpp",
  php: "php", go: "go", vue: "vue", wat: "wast", wast: "wast",
  rb: "ruby", gemspec: "ruby", rake: "ruby",
  cs: "csharp", kt: "kotlin", kts: "kotlin", scala: "scala", sc: "scala",
  dart: "dart", swift: "swift", lua: "lua", pl: "perl", pm: "perl",
  r: "r", clj: "clojure", cljs: "clojure", cljc: "clojure", edn: "clojure",
  hs: "haskell", jl: "julia", erl: "erlang", hrl: "erlang",
  ps1: "powershell", psm1: "powershell", groovy: "groovy", gradle: "groovy",
  diff: "diff", patch: "diff",
  properties: "properties", ini: "properties", env: "properties", cfg: "properties", conf: "properties",
  sh: "shell", bash: "shell", zsh: "shell",
};

/** Canonical CodeMirror grammar tag for a path (""=no grammar / plain text). */
export function langId(path: string): string {
  const name = path.replace(/\/$/, "").split("/").pop() ?? "";
  if (/^dockerfile$/i.test(name) || /\.dockerfile$/i.test(name)) return "dockerfile";
  const dot = name.lastIndexOf(".");
  if (dot < 0) return "";
  return LANG_ID[name.slice(dot + 1).toLowerCase()] ?? "";
}

// ---- file-status label (diff header) ----
// Maps the git status char from an AgentFile to a human-readable label.
export function fileStateLabel(state: string): string {
  return (
    ({ A: "new file", D: "deleted", M: "modified", R: "renamed" } as Record<string, string>)[
      state
    ] ?? "modified"
  );
}

// ---- hunk header (diff header) ----
// Build a `@@ -a,b +c,d @@` style hunk header from old/new line counts. The
// backend FileDiff carries only old/new content (not the raw patch header), so
// for a whole-file view we synthesize a single hunk spanning both versions.
// A new file → `@@ -0,0 +1,N @@`; a deleted file → `@@ -1,M +0,0 @@`.
export function hunkHeader(oldText: string, newText: string): string {
  const count = (s: string) => (s.length ? s.replace(/\n$/, "").split("\n").length : 0);
  const o = count(oldText);
  const n = count(newText);
  const oldRange = o ? `-1,${o}` : "-0,0";
  const newRange = n ? `+1,${n}` : "+0,0";
  return `@@ ${oldRange} ${newRange} @@`;
}
