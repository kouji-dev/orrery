// ORCHESTRA domain models

export type AgentStatus =
  | "running"
  | "blocked"
  | "waiting"
  | "done"
  | "idle"
  | "queued";

export type LogKind = "cmd" | "out" | "ok" | "warn" | "err" | "sys";
export interface LogLine {
  t: LogKind;
  s: string;
}

export type PendingKind = "permission" | "decision" | "review";
export interface PendingItem {
  id: string;
  kind: PendingKind;
  title: string;
  cmd: string;
  when: string;
}

export interface AgentFile {
  path: string;
  add: number;
  del: number;
  state: "A" | "M" | "D";
}

export interface AgentTool {
  id: "claude" | "codex" | "cursor" | "gemini";
  name: string;
  short: string;
  accent: string;
  models: string[];
  effort: false | string[];
}

export interface Project {
  id: string;
  name: string;
  org: string;
  path: string;
  repo?: string;
  branch: string;
  head: string;
  color: string;
  icon: string;
  hasGit: boolean;
  branches: string[];
  files: string[];
}

export interface Agent {
  id: string;
  projectId: string;
  tool: AgentTool["id"];
  model: string;
  effort?: string | null;
  name: string;
  task: string;
  status: AgentStatus;
  branch: string;
  worktree: string;
  base: string;
  commits: number;
  elapsed: number;
  progress: number;
  blockReason?: string;
  waitReason?: string;
  files: AgentFile[];
  pending: PendingItem[];
}

export interface ChatMessage {
  role: "user" | "agent" | "sys";
  s: string;
  time: string;
  decision?: boolean;
}

export interface DiffLine {
  k: "+" | "-" | " ";
  n: number;
  s: string;
}
export interface DiffHunk {
  meta: string;
  lines: DiffLine[];
}
export interface Diff {
  file: string;
  lang: string;
  hunks: DiffHunk[];
}

export interface Commit {
  agent: string;
  projectId: string | null;
  sha: string;
  msg: string;
  when: string;
  files: number;
}

export interface Tab {
  id: string;
}

export interface Tweaks {
  theme: "dark" | "light";
  palette: [string, string];
  density: "compact" | "regular" | "comfy";
  defaultViz: VizMode;
  rightPanel: boolean;
  motion: boolean;
}

export type VizMode = "grid" | "kanban" | "graph" | "timeline";

export interface MenuItem {
  label?: string;
  icon?: string;
  onClick?: () => void;
  disabled?: boolean;
  danger?: boolean;
  accent?: string;
  kbd?: string;
  sep?: boolean;
}

export interface ContextMenuState {
  x: number;
  y: number;
  items: MenuItem[];
}
