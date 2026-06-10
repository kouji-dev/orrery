// Orrery domain models

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
  state: "A" | "M" | "D" | "R"; // R = renamed/moved
  oldPath?: string; // R only: the pre-move path
}

// Old (HEAD) vs new (working tree) content of a file, for the diff view.
export interface FileDiff {
  old: string;
  new: string;
  lang: string;
}

// A node in an agent's worktree file tree. children === null → not loaded yet (lazy).
export interface FileNode {
  name: string;
  path: string;
  isDir: boolean;
  ignored: boolean;
  children: FileNode[] | null;
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
  // --- persisted ---
  id: string;
  name: string;
  path: string;
  icon: string;
  color: string;
  // --- transient (computed by the backend at read time, never stored) ---
  folderExists: boolean;
  hasGit: boolean;
  branch?: string;
  head?: string;
  // --- ui-only extras (mock/demo data) ---
  org?: string;
  repo?: string;
  branches?: string[];
  files?: string[];
}

/** Async per-entity sub-resource: `idle` = never requested (unknown, NOT empty). */
export interface Loadable<T> {
  status: "idle" | "loading" | "ready" | "error";
  data: T;
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
  /** True once launched at least once — drives Start (first run) vs Resume. */
  started?: boolean;
  /** The tool's CLI session id (captured from a hook), for `--resume <id>`. */
  sessionId?: string;
  commits: number;
  /** Persisted placeholder (the backend sends 0). LIVE elapsed is derived in
   *  the UI from AgentRuntimeService.elapsedFor() — never patched in here, so
   *  the clock can tick without churning agent record identities. */
  elapsed: number;
  progress: number;
  /** Live: process is producing output right now (recent PTY activity / title spinner). */
  working?: boolean;
  /** Live: the agent's terminal title signals it is waiting on the user (permission). */
  needsInput?: boolean;
  blockReason?: string;
  waitReason?: string;
  pending: PendingItem[];
  // (worktree-scoped transients — file tree / git status / branch commits — live
  // in AgentWorkStore as per-agent Loadable maps, NOT on the Agent record)
}

// ---- agent notifications ----
// What an agent surfaced that wants the user: a question, a permission request,
// or finished work. Detail text is scraped from the agent's terminal output.
export type NotificationKind = "question" | "permission" | "done";
export type NotificationStatus = "pending" | "accepted" | "rejected" | "dismissed";

// The classification carried by agent://activity, used to colorize each preview
// row in the overview mini-term: who/what produced the line.
export type ActivityKind =
  | "user"
  | "agent"
  | "tool"
  | "success"
  | "error"
  | "question"
  | "info";

// A settings-rule suggestion attached to a permission request. Only Claude
// emits these; codex/cursor/gemini send []. Display-only for now — the action
// (persist the rule / return a decision) is deferred to the remote-approval phase.
export interface PermissionSuggestion {
  behavior: "allow" | "deny";
  rule: string; // a settings rule string, e.g. `Bash(rm *)`
  description: string;
}

// One option offered for an AskUserQuestion-style question: a short `label` (the
// choice) and an optional longer `description` (revealed on hover in the card).
export interface PermissionOption {
  label: string;
  description?: string;
}

// One question in an AskUserQuestion-style permission prompt. `header` is a short
// label/category for the question; `options` are the CONCRETE choices Claude
// offered (each label + optional description). `multiSelect` is whether more than
// one option may be picked (Claude's flag, default false → single-select).
//
// NOTE: Claude's TUI auto-appends an "Other" free-text choice AFTER these
// concrete `options` (its number = options.length + 1). That choice is NOT in the
// payload — the multi-step question UI synthesizes it so the user can type a
// custom answer.
export interface PermissionQuestion {
  question: string;
  header?: string;
  options?: PermissionOption[];
  multiSelect?: boolean;
}

export interface AgentNotification {
  id: string;
  agentId: string;
  agentName: string;
  kind: NotificationKind;
  title: string;
  detail: string; // scraped prompt / context, or a concise permission summary
  createdAt: number;
  status: NotificationStatus;
  decision?: string; // human-readable record of what the user chose
  // ---- structured permission detail (kind === "permission"; all optional) ----
  tool?: string; // the tool being invoked, e.g. "Bash", "Edit"
  command?: string; // the concrete command / args, when present
  description?: string; // a human description of the requested action
  filePath?: string; // the file the action targets, when present
  mode?: string; // the agent's permission mode, when present
  suggestions?: PermissionSuggestion[]; // suggested settings rules (Claude only)
  summary?: string; // a human headline for the prompt (e.g. AskUserQuestion summary)
  questions?: PermissionQuestion[]; // AskUserQuestion-style questions + options (display-only)
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
  ts?: number; // unix seconds, for cross-project ordering (absent on simulated commits)
}

export interface Tab {
  id: string;
  // "orchestrator" = the fixed overview tab; "agent" = a workspace tab holding a
  // pane tree (one agent, or several tiled together). Defaults to "agent".
  kind?: "orchestrator" | "agent";
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

// ---- system metrics (status-bar cpu/memory monitor) ----
// One subtree's roll-up: the app's own tree ("app"/"orrery") or an agent's
// (uuid string / agent name). cpu is a percent (may exceed 100 on multi-core);
// memBytes is resident bytes.
export interface ProcMetric {
  id: string;
  label: string;
  cpu: number;
  memBytes: number;
}

// A whole snapshot pushed on `system://metrics` every 3s. Totals are the SUM of
// the rows — cpu%/memory used by orrery + its agents ONLY (not machine-wide).
// totalCpu is machine-relative (a share of all logical cores, like Task Manager).
export interface SystemMetrics {
  totalCpu: number;
  totalMemBytes: number;
  procs: ProcMetric[];
}

// A cost snapshot pushed on `system://cost` (~every 5 minutes). `available` is false
// when ccusage could not run — the status bar then hides the readout.
export interface CostSnapshot {
  totalCost: number;
  currency: string;
  available: boolean;
}
