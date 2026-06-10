// Orrery shared helpers, status metadata. (The icon set now lives in
// icon.component.ts, backed by Lucide — see `LUCIDE` there.)
import { AGENT_TOOLS } from "./data";
import { AgentStatus, LogKind } from "./models";

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

// A single ANSI/escape token (CSI / OSC / 2-char), a RUN of plain printable
// characters, or one stray control character. We walk the chunk token-by-token
// so the cursor/erase CSI sequences can be acted on (not just deleted) — a
// deleted clear-screen would otherwise leave a TUI's previous frame stacked on
// top of the new one, which is the garbage we're trying to avoid in the
// preview. Printable text (anything >= \x20, which can never start an escape)
// is consumed as one greedy run so a flush costs O(tokens), not O(chars ×
// line length); controls (\n \r \t …) fall through to the single-char arm.
const TOKEN_RE =
  /\x1b\[[0-9;?]*[ -/]*[@-~]|\x1b\][^\x07\x1b]*(?:\x07|\x1b\\)|\x1b[@-Z\\-_]|[^\x00-\x1f]+|[\s\S]/g;
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
  // Splice a whole run of text into the current line at the cursor in one
  // operation (overwrite-in-place, padding with spaces if the cursor is past
  // the line end). Equivalent to writing the run char-by-char, but O(line).
  const writeAt = (text: string) => {
    ensureRow();
    const line = lines[row];
    const head = col <= line.length ? line.slice(0, col) : line.padEnd(col);
    lines[row] = head + text + line.slice(col + text.length);
    col += text.length;
  };

  const tokens = stripUnactionable(chunk).match(TOKEN_RE) ?? [];
  for (const tok of tokens) {
    if (tok.charCodeAt(0) !== 0x1b) {
      // plain text run or a single stray control char (runs never contain
      // controls — the run arm of TOKEN_RE excludes \x00-\x1f)
      if (tok === "\n") {
        row += 1;
        col = 0;
        ensureRow();
      } else if (tok === "\r") {
        col = 0;
      } else if (tok === "\t") {
        writeAt("  ");
      } else if (tok >= " ") {
        writeAt(tok); // whole printable run, one splice
      }
      continue; // drop remaining stray control chars (\x07, \x08, …)
    }
    // ESC-led: a CSI we may need to act on (others stripped upstream; a lone
    // dangling ESC falls through the exec below and is dropped, as before)
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
