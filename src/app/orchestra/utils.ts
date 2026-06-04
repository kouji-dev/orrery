// ORCHESTRA shared helpers, icon paths, status metadata
import { AGENT_TOOLS } from "./data";
import { AgentStatus, LogKind } from "./models";

// ---- icon set (stroke, currentColor) ----
export const ICONS: Record<string, string> = {
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
