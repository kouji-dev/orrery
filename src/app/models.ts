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

/** One entry of a tool's curated `--model` list. */
export interface ModelOption {
  /** The exact value forwarded as `--model`. An alias (`opus`, `fable`…)
   *  resolves to the tool's latest of that family; a full name
   *  (`claude-opus-4-6`) pins a version. */
  id: string;
  /** Picker label — the human name, versioned when pinned ("Opus 4.6"). */
  label: string;
  /** Picker group heading; absent = ungrouped. */
  group?: string;
  /** Effort levels THIS model accepts when narrower than the tool's
   *  (`false` = none: the effort field hides). Absent = the tool's list. */
  effort?: false | string[];
  /** Level pre-selected for this model; absent = `high` when offered, else the first. */
  defaultEffort?: string;
}

export interface AgentTool {
  id: "claude" | "codex" | "cursor" | "gemini";
  name: string;
  short: string;
  accent: string;
  /** Curated models; the first entry is the spawn default. */
  models: ModelOption[];
  /** Effort levels the tool's CLI flag accepts (`false` = no knob at all).
   *  Also the fallback for a custom model id typed in Settings. */
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
  /** repo default branch (origin/HEAD → main/master → HEAD); pre-selected base
   *  when spawning. Undefined → fall back to the first entry in `branches`. */
  defaultBranch?: string;
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

export type TicketStatus = "todo" | "inprogress" | "done";

export interface Ticket {
  id: string;
  title: string;
  notes: string;
  status: TicketStatus;
  projectId: string | null;
  agentId: string | null;
  /** snake_case labels (mirrors the Rust `tags` column). */
  tags: string[];
  createdAt: number;
  updatedAt: number;
}

export type CommentRole = "user" | "agent";

export interface Comment {
  id: string;
  ticketId: string;
  author: string;
  role: CommentRole;
  tool: string | null;
  body: string;
  createdAt: number;
}

export interface Agent {
  id: string;
  projectId: string;
  ticketId?: string;
  /** "shell" is the v2 project pseudo-agent: a project tab's plain shell in
   *  the main worktree (never a stored agent — synthesized at render time). */
  tool: AgentTool["id"] | "shell";
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
  /** Unix ms of the last launch/resume (set at spawn too, so a never-run agent
   *  still sorts). Absent on rows written before the column existed. */
  lastRunAt?: number;
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
  /** UI-only: the row is a placeholder for an agent being created, or a live
   *  agent whose removal is in flight. Never sent by the backend. */
  transition?: "creating" | "removing";
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
  // pane tree (one agent, or several tiled together); "project" (v2) = the same
  // pane tree rooted at a project's MAIN worktree — an agent is just a worktree
  // with a process attached, and a project tab is that worktree with a plain
  // shell. Defaults to "agent".
  kind?: "orchestrator" | "agent" | "backlog" | "ticket" | "project";
  ticketId?: string;
  projectId?: string;
}

export interface Tweaks {
  theme: "dark" | "light";
  density: "compact" | "regular" | "comfy";
  defaultViz: VizMode;
  motion: boolean;
}

export type VizMode = "grid" | "kanban" | "graph" | "timeline";

/** Recency bucket of the orchestrator grid, keyed off {@link Agent.lastRunAt}.
 *  A long-lived workspace accumulates dozens of finished agents; the grid shows
 *  one bucket at a time so the recent ones are not buried. */
export type GridRange = "week" | "month" | "older";

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

// ---- system metrics (status-bar gauge + dev-panel Resources tab) ----
// One subtree's roll-up: the app's own tree ("app"/"Orrery") or an agent's
// (uuid string / agent name). cpu is machine-relative percent (already divided
// by the core count, like Task Manager); memBytes is resident bytes.
export interface ProcMetric {
  id: string;
  label: string;
  cpu: number;
  memBytes: number;
}

// A whole snapshot pushed on `system://metrics` (5s while agents run, 20s idle).
// Totals are the SUM of the rows — cpu%/memory used by orrery + its agents ONLY
// (not machine-wide). sysMemBytes/cores are the machine denominators for gauges.
export interface SystemMetrics {
  totalCpu: number;
  totalMemBytes: number;
  sysMemBytes: number;
  cores: number;
  procs: ProcMetric[];
}

// ---- A7.7 process tree (`process_tree` command; polled while the perf panel is open) ----

/** One node in the recursive process tree. `privBytes` (private, no shared-page
 *  double count) is the headline; `rssBytes` is secondary and NEVER rolled up —
 *  subtree totals sum private bytes only. */
export interface ProcessNode {
  pid: number;
  name: string;
  /** Self-explaining kind annotation ("webview2 · not our code", …) or null. */
  note: string | null;
  /** Machine-relative % (already ÷ cores, like Task Manager). */
  cpu: number;
  privBytes: number;
  rssBytes: number;
  subtreeCpu: number;
  subtreePrivBytes: number;
  subtreeProcs: number;
  /** In the Job Object but unreachable via the parent-pid walk (surfaced so
   *  nothing we spawned can hide). */
  detached: boolean;
  /** External browser Orrery merely launched (link handoff) — rendered but
   *  excluded from every subtree rollup. */
  excluded: boolean;
  children: ProcessNode[];
}

/** One tree root: the app ("app"/"Orrery") or an agent (uuid / display name). */
export interface ProcessTreeRoot {
  id: string;
  label: string;
  node: ProcessNode;
}

export interface ProcessTreeSnapshot {
  roots: ProcessTreeRoot[];
  tsMs: number;
}

// ---- A0.7 emit telemetry (`telemetry_emits` / `telemetry_trace_state`) ----

/** One event name's aggregate row (session-cumulative; calls10s is a rolling rate). */
export interface EmitAggRow {
  name: string;
  count: number;
  totalBytes: number;
  maxBytes: number;
  p50Bytes: number;
  p95Bytes: number;
  calls10s: number;
  peakPerSec: number;
}

/** Raw emit-trace indicator state (also pushed on `telemetry://trace` changes). */
export interface TelemetryTraceState {
  active: boolean;
  startedMs: number;
  bytes: number;
}

// A cost snapshot pushed on `system://cost` (~every 5 minutes). `available` is false
// when ccusage could not run — the status bar then hides the readout.
export interface CostSnapshot {
  totalCost: number;
  currency: string;
  available: boolean;
}

// ---- app settings ----
// ONE JSON document persisted by the backend (`settings_get` / `settings_set`),
// serde camelCase with defaults on every field — this mirrors the Rust
// `Settings` struct exactly (src-tauri/src/settings/model.rs).
export type UpdateChannel = "stable" | "beta";
export type UpdatePolicy = "auto" | "notify" | "manual";
export type AutoApprovePolicy = "off" | "allowlist" | "everything";

/** Per-tool runtime detection (backend `detect_tools` / `verify_tool_path`).
 *  `ok` = resolved + `--version` ran; `error` = found but couldn't launch
 *  (`reason` explains); `missing` = nothing on PATH. */
export interface ToolDetection {
  id: string;
  status: "ok" | "error" | "missing";
  /** Mirror of `status === "ok"` (back-compat with the old boolean detection). */
  available: boolean;
  /** Resolved executable path (PATH or a manual override); null when missing. */
  path: string | null;
  /** Version parsed from `<bin> --version`, when it ran. */
  version: string | null;
  /** Where `path` came from: "path" (auto) | "manual" (user override). */
  source: "path" | "manual" | null;
  /** Why an `error` tool can't run — shown in the locate-binary editor. */
  reason: string | null;
  /** True when the install is a script shim (npm `.cmd`/`.ps1`, Windows) rather
   *  than a native executable — the tool tile shows a one-line hint recommending
   *  the native installer (a shim can cost a shell wrapper process per agent). */
  shim: boolean;
}

/** Which notification kinds are raised at all (off = the alert is not raised). */
export interface SettingsEvents {
  finished: boolean;
  question: boolean;
  permission: boolean;
  error: boolean;
}

export interface Settings {
  /** Update channel: stable | beta. */
  channel: UpdateChannel;
  /** Startup update behavior: auto (install) | notify (check only) | manual (no check). */
  updatePolicy: UpdatePolicy;
  /** Spawn-modal prefill tool id"" = none saved (spawn keeps its hardcoded default). */
  defaultTool: string;
  /** Per-tool model prefill (tool id → model id). Absent key = the curated default. */
  toolModel: Record<string, string>;
  /** Per-tool effort prefill (tool id → effort). Absent key = the tool default. */
  toolEffort: Record<string, string>;
  /** Per-tool manual executable path override (tool id → absolute path). Absent
   *  key = auto-detect on PATH. A set path wins for both detection and launch. */
  toolPath: Record<string, string>;
  /** Branch name template; tokens: {name} (worktree slug), {tool}, {date} (MMDD). */
  branchTemplate: string;
  /** Absolute dir new worktrees are created under"" = the built-in app-data root. */
  worktreeRoot: string;
  /** Absolute dir the add-project folder picker opens in"" = the OS default. */
  projectsRoot: string;
  /** Relaunch agents that were running when the app last quit/crashed. */
  autoResume: boolean;
  /** Write dirty editor buffers automatically shortly after typing stops. */
  autosave: boolean;
  /** User keybinding overrides: command id → binding ("Ctrl+Shift+p").
   *  Absent id = the command's built-in default binding. */
  keymap: Record<string, string>;
  /** Per-command "fires inside a focused terminal" override (the steal list).
   *  Absent id = the default: Ctrl/Mod+Shift chords and Search Everywhere
   *  steal; every other chord flows to the PTY untouched. */
  keymapTerminal: Record<string, boolean>;
  /** Per-tool permission policy. Absent key = "off" (the tool's own flow). */
  autoApprove: Record<string, AutoApprovePolicy>;
  /** Permission prompts raise a native toast even when the app is unfocused. */
  remoteApproval: boolean;
  /** Master toggle: also fire native OS toasts (off = in-app only). */
  osNotifications: boolean;
  events: SettingsEvents;
  /** Play a sound cue when a notification fires. */
  sound: boolean;
  /** "Ping" | "Chime" | "Pop" | "Glass" | "Submarine". */
  soundName: string;
  /** 0–100. */
  volume: number;
  /** Budget cap (USD) for AI git actions; 0 = no cap. At the cap, AI variants
   *  disable (native stays fully usable). */
  budgetCapUsd: number;
  /** AI actions estimated above this USD amount need a confirming second
   *  click; 0 = never confirm. */
  confirmAboveUsd: number;
  /** User-editable provider rate table (model id → $/Mtok). Absent model =
   *  the built-in default rates in EstimateService. */
  costRates: Record<string, CostRate>;
  /** Opt-in raw emit trace (A0.7): NDJSON `ts·name·key·bytes` lines in
   *  app-data/telemetry (NEVER payload contents). Auto-disables after
   *  30min/200MB — the backend then writes this back to false. */
  telemetryRawTrace: boolean;
  /** Minutes without a request before a language server is stopped (it
   *  restarts on the next request). */
  lspIdleMinutes: number;
  /** Manual language-server executable paths (pack id → absolute path).
   *  Absent id = auto-detect on PATH. */
  lspPaths: Record<string, string>;
  /** Fall back to a language server found on PATH / a known dir when no pack
   *  is installed. Off = a server without its pack is "not installed" even
   *  when a system copy exists (self-contained packs are the norm). */
  lspUseSystemServers: boolean;
  /** Override of the extension registry index URL; null = the built-in one. */
  extRegistryUrl: string | null;
}

// ---- extensions: grammar packs + language servers (M1) ----

/** `runtime` = a dependency pack a server ships with (Node, Java): auto-
 *  managed, no enable toggle, pulled in by `ext_install` of the server. */
export type ExtKind = "grammar" | "server" | "runtime";
/** Backend-owned lifecycle state of one pack. `installed` + `enabled` on the
 *  pack say whether it is on disk / loaded; this is the transition it is in. */
export type ExtState = "available" | "downloading" | "installed" | "pendingRestart" | "error" | "incompatible";
/** Where a language server's executable was found (server packs only).
 *  `bundled` = the server pack is installed; `path` is its launch binary. */
export interface ExtDetection {
  status: "found" | "missing" | "configured" | "bundled";
  path: string | null;
  /** Prerequisite hint ("needs JDK 17+ — JAVA_HOME not set"). */
  hint: string | null;
}
export interface ExtPack {
  id: string;
  kind: ExtKind;
  name: string;
  description: string;
  /** Registry (latest) version. */
  version: string;
  /** Version on disk; null when not installed. */
  installedVersion: string | null;
  languages: string[];
  sizeBytes: number;
  /** Pack ids this one needs (a server's runtime packs); `[]` otherwise. */
  requires: string[];
  /** Servers: own size + the not-yet-installed packs in `requires` — what an
   *  Install actually downloads. Other kinds: `sizeBytes`. */
  bundledSizeBytes: number;
  installed: boolean;
  enabled: boolean;
  /** Registry gate: the pack's ABI / minimum app version fits this build. */
  compatible: boolean;
  /** A build exists for this OS/arch. */
  availableForTarget: boolean;
  state: ExtState;
  error: string | null;
  detection: ExtDetection | null;
  /** Minimum Orrery version the pack needs (surfaced on incompatible rows). */
  minAppVersion?: string | null;
}
export interface ExtRegistryView {
  registryUrl: string;
  /** Unix ms of the last successful index fetch; null = never. */
  fetchedAt: number | null;
  /** The last fetch failed — `items` is the cached list. */
  offline: boolean;
  items: ExtPack[];
}
export interface ExtProgressPayload {
  /** The REQUESTED pack — a server's dependency downloads stream under the
   *  server's id, naming the dependency in `dependency`. */
  id: string;
  downloaded: number;
  total: number | null;
  phase: "download" | "verify" | "unpack" | "activate";
  /** The pack being fetched right now when it is a dependency of `id`. */
  dependency: string | null;
  /** 1-based step of `stepCount` (deps first, the requested pack last). */
  stepIndex: number;
  stepCount: number;
}

// ---- symbols: tree-sitter index + Monaco navigation (M2) ----
// Every line/col below is 0-BASED (the backend's convention); the Monaco
// adapters in workspace/nav-providers.ts add the 1 on the way in and out.

export type IndexState = "idle" | "indexing" | "ready" | "error";
/** `symbols://index` payload + `symbols_index_status` reply — one per root. */
export interface IndexStatus {
  /** The root's id: an agent uuid, or the project id for its main checkout. */
  root: string;
  state: IndexState;
  /** Files in the index once ready. */
  files: number;
  /** Files parsed so far / to parse in the current run. */
  done: number;
  total: number;
  elapsedMs: number;
  error?: string | null;
}

/** One node of a file's outline (`symbols_document`), hierarchical. */
export interface DocSymbol {
  name: string;
  /** Grammar-level kind ("class", "fn", "method", "field", "enum", …). */
  kind: string;
  line: number;
  col: number;
  endLine: number;
  endCol: number;
  /** Where the NAME sits inside the range (the outline's click target). */
  selLine: number;
  selCol: number;
  container?: string | null;
  children: DocSymbol[];
}

/** Which roots a `symbols_search` covers. */
export interface SymbolScope {
  kind: "worktree" | "project" | "all";
  agentId?: string | null;
  projectId?: string | null;
}

/** One `symbols_search` hit (backend-ranked). `agentId` null = the project
 *  checkout; `root` is the human label of that root (agent name). */
export interface SymbolHit {
  name: string;
  kind: string;
  path: string;
  line: number;
  col: number;
  container?: string | null;
  agentId: string | null;
  root: string | null;
}

/** One navigation target. `uri` is the Monaco model uri (`orrery://<id>/<path>`
 *  or a virtual read-only scheme); `id` + `path` are its parsed halves. */
export interface NavLocation {
  uri: string;
  /** Both null for a library hit (M4): the uri is `orrery-lib://…` and
   *  `preview` carries the fully-qualified name ("java.util.ArrayList"). */
  id: string | null;
  path: string | null;
  line: number;
  col: number;
  endLine: number;
  endCol: number;
  kind?: string | null;
  container?: string | null;
  preview?: string | null;
}

export type NavSource = "index" | "lsp";
/** M3: what the language server was doing when the router answered. */
export type LspState = null | "starting" | "timeout" | "unavailable";

export interface NavResult {
  source: NavSource;
  locations: NavLocation[];
  lspState: LspState;
  /** M4: why an EMPTY answer may be empty, when the library index can tell
   *  — no JDK on this machine, or the pom names jars without a sources jar. */
  libHint?: LibHint | null;
}

export type LibHint = "jdk-missing" | "sources-missing";

/** `nav_hover` reply: `contents` is markdown (signature block + meta lines). */
export interface NavHover {
  source: NavSource;
  contents: string;
  range?: { line: number; col: number; endLine: number; endCol: number } | null;
  /** M3: set when the server for this language was still starting. */
  lspState?: LspState;
}

// ---- language servers (M3) ----

/** One running (or lately running) server process: one per language pack per
 *  project — every worktree of a project shares it. */
export type LspServerState = "starting" | "ready" | "idle" | "crashed" | "stopped" | "missing";

export interface LspServer {
  /** `<extId>:<projectId>`. */
  id: string;
  extId: string;
  /** Short binary/pack label ("jdtls", "rust-analyzer"). */
  label: string;
  language: string;
  /** Project root the server was started in. */
  root: string;
  projectId: string;
  projectName: string;
  pid: number | null;
  state: LspServerState;
  memBytes: number;
  /** Machine-relative cpu %, one decimal. */
  cpu: number;
  restarts: number;
  /** Unix ms; null until the process is up. */
  startedAt: number | null;
  lastError: string | null;
}

export interface LspStatus {
  servers: LspServer[];
}

/** `nav_virtual_read` reply: a read-only document behind a non-`orrery://`
 *  uri (a jdtls class-file source, a library entry in M4). */
export interface VirtualDoc {
  uri: string;
  /** App language tag (`langId` vocabulary); "" = plain text. */
  language: string;
  text: string;
  /** Human title ("java.util.ArrayList", "List.java"). */
  title: string;
}

// ---- library sources (M4) ----

export type LibSourceKind = "jdk" | "maven" | "cargo";
export type LibSourceState = "idle" | "pending" | "indexing" | "done" | "error" | "cancelled" | "removed";

/** One library source the symbol index covers (`libsrc_sources`): a JDK
 *  (`jdk:<hash>`), or — per registered project — the crates its `Cargo.lock`
 *  pins (`cargo:<projectId>`) / the jars its `pom.xml` names (`maven:<projectId>`). */
export interface LibSource {
  id: string;
  kind: LibSourceKind;
  /** The src.zip / the lockfile ("C:/jdk-21/lib/src.zip", "C:/w/orrery/src-tauri/Cargo.lock"). */
  path: string;
  /** Human label ("JDK 21 (openjdk21)", "Cargo · orrery", "Maven · shop"). */
  label: string;
  state: LibSourceState;
  /** Files / declarations in the index once done. */
  files: number;
  decls: number;
  /** Unix ms of the last completed run; null = never. */
  indexedAt: number | null;
  sizeBytes: number;
  /** The owning project (null for a JDK). */
  projectId: string | null;
  projectName: string | null;
  /** Crates / jars the lockfile names (1 for a JDK); not on disk; over a perf guard. */
  artifacts: number;
  missing: number;
  skipped: number;
  error?: string | null;
  /** Progress of the current run — merged in from `libsrc://status`, never
   *  part of the backend's `libsrc_sources` reply. */
  done?: number;
  total?: number;
}

/** `libsrc://status` payload: one source's run, every ~500 files. */
export interface LibSrcStatus {
  sourceId: string;
  label: string;
  kind: LibSourceKind;
  state: "indexing" | "done" | "error" | "cancelled";
  done: number;
  total: number;
  decls: number;
  error?: string | null;
}

/** An available app update as resolved by `update_check` (date/notes optional —
 *  older backends returned a bare version string). */
export interface UpdateInfo {
  version: string;
  date?: string | null;
  notes?: string | null;
}

// ---- git-inspection models (serde camelCase from backend) ----

/** One file entry in a commit's changed-file list. */
export interface CommitFile {
  path: string;
  state: string;
  add: number;
  del: number;
}

/** One line of blame output for a file (hydrated view — see hydrateBlame). */
export interface BlameLine {
  n: number;
  sha: string;
  author: string;
  when: number;
  summary: string;
  line: string;
}

/** One commit in an interned blame's commit table (A0.6 blame interning). */
export interface BlameCommit {
  sha: string;
  author: string;
  when: number;
  summary: string;
}

/** The wire shape of `agent_blame` / `agent_working_blame`: commit metadata
 *  interned ONCE in `commits`, per-line entries carrying only an index `c`. */
export interface BlameIntern {
  commits: BlameCommit[];
  lines: { n: number; c: number; line: string }[];
}

/** Expand an interned blame into flat per-line rows for the view components.
 *  The commit strings are SHARED references (one JS string per commit, not per
 *  line), so a 50k-line file costs row objects + pointers — not megabytes of
 *  duplicated author/summary strings. */
export function hydrateBlame(b: BlameIntern | null | undefined): BlameLine[] {
  if (!b?.lines?.length) return [];
  const orphan: BlameCommit = {
    sha: "0000000",
    author: "Uncommitted",
    when: 0,
    summary: "Uncommitted changes",
  };
  return b.lines.map((l) => {
    const c = b.commits[l.c] ?? orphan;
    return { n: l.n, sha: c.sha, author: c.author, when: c.when, summary: c.summary, line: l.line };
  });
}

/** One commit entry in a file's history. */
export interface FileHistoryEntry {
  sha: string;
  author: string;
  email: string;
  when: number;
  summary: string;
  add: number;
  del: number;
}

/** Changed files across a commit range (from..to). */
export interface RangeFiles {
  files: CommitFile[];
  from: string;
  to: string;
}

/** One file touched by a SET of selected commits (`agent_commits_files`): the
 *  change SUMMED over every selected commit that touched it, plus the span of
 *  those commits. `firstSha`/`lastSha` key the per-file diff, so a file click
 *  costs two tree diffs instead of one per selected commit. */
export interface CommitsFile extends CommitFile {
  oldPath?: string;
  firstSha: string;
  lastSha: string;
  commits: number;
}

/** Discriminated union describing which git surface is currently displayed. */
export type GitView =
  | { kind: 'commit'; sha: string; path?: string }
  | { kind: 'range'; shas: string[] }
  /** The UNION of what the selected commits themselves changed — unlike
   *  'range', commits sitting between two selections contribute nothing. */
  | { kind: 'commits'; shas: string[] }
  | { kind: 'filehistory'; path: string }
  | { kind: 'conflict' };

// ---- native merge + conflict session (serde camelCase from backend) ----

/** One conflicted file in a merge session: full stage-1/2/3 contents plus the
 *  working-tree text with diff3 conflict markers (parsed into segments by the
 *  3-way view). `resolved` is false in backend listings; the store flips it
 *  after `conflict_resolve`. */
export interface ConflictFile {
  path: string;
  ours: string;
  theirs: string;
  base: string;
  merged: string;
  resolved: boolean;
  lang: string;
}

/** Result of a native merge. Empty `conflicts` = merged clean (or FF/up to
 *  date); otherwise a session is in progress (continue or abort). */
export interface MergeSession {
  ours: string;
  theirs: string;
  conflicts: ConflictFile[];
}

/** Whether a merge / rebase / cherry-pick is in progress in a worktree. */
export interface GitSessionState {
  state: "none" | "merge" | "rebase" | "cherrypick" | "revert" | "other";
  conflicts: number;
  ours: string;
}

// ---- AI cost estimation (A4.3) ----

/** $ per million tokens for one model (user-editable in settings). */
export interface CostRate {
  in: number;
  out: number;
}

/** A cost envelope for an AI git action, shown on the dropdown row BEFORE the
 *  action runs. `confidence` is "heuristic" until calibration lands (A6). */
export interface CostEstimate {
  op: string;
  model: string;
  tokensLow: number;
  tokensHigh: number;
  usdLow: number;
  usdHigh: number;
  confidence: string;
}
