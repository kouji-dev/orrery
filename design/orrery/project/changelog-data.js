/* global window */
// Orrery release history — single source of truth for the in-app "What's new"
// digest and the full marketing changelog page. Modelled on `git tag` + a
// conventional-commit `git log`; the real build wires this to the repo.
// Newest first. channel: "beta" (shipped pre-release) | "dev" (development).

const RELEASES = [
  {
    tag: "v0.9.4", channel: "beta", date: "June 18, 2026", ref: "e7c41a0",
    summary: "A what’s-new digest after every update, one-click log access, and a steadier auto-updater.",
    commits: [
      { type: "feat", hash: "e7c41a0", scope: "updater", msg: "show a “what’s new” digest the first time you open a new build", by: "@kj" },
      { type: "feat", hash: "b9d3f12", scope: "diagnostics", msg: "open the full log file straight from Settings → Updates" },
      { type: "feat", hash: "1f8c2a9", scope: "orchestrator", msg: "stream each agent’s terminal output onto its overview card" },
      { type: "fix",  hash: "4cf0a2b", scope: "updater", msg: "clamp install progress so the bar never moves backward" },
      { type: "perf", hash: "c12f9b8", scope: "sidebar", msg: "render the agent tree 40% faster past 50 agents" },
    ],
  },
  {
    tag: "v0.9.3", channel: "beta", date: "June 3, 2026", ref: "c0a5d18",
    summary: "Inline review comments and clearer blocked-agent cards.",
    commits: [
      { type: "feat",  hash: "c0a5d18", scope: "review", msg: "send inline diff comments back to a running agent" },
      { type: "feat",  hash: "2af6b91", scope: "agents", msg: "blocked agents surface the pending decision inline on the card" },
      { type: "fix",   hash: "7e0d1a4", scope: "tabs",   msg: "detaching an agent from a tiled tab keeps its terminal scrollback" },
      { type: "chore", hash: "44b1c2e", scope: "deps",   msg: "bump the React renderer and tighten the content-security-policy" },
    ],
  },
  {
    tag: "v0.9.2", channel: "beta", date: "May 22, 2026", ref: "7d10b4e",
    summary: "Graph and Timeline views, plus tab tiling.",
    commits: [
      { type: "feat",  hash: "b3a7c10", scope: "viz",  msg: "add Graph and Timeline visualizations to the Orchestrator" },
      { type: "feat",  hash: "e5519f2", scope: "tabs", msg: "drag a tab onto another to tile two agent terminals" },
      { type: "fix",   hash: "a8810dd", scope: "tabs", msg: "tab reorder no longer drops the wrong worktree on merge" },
      { type: "chore", hash: "02ce915", scope: "deps", msg: "bump Tauri to 2.1 and move to a Node 22 baseline" },
    ],
  },
  {
    tag: "v0.9.0", channel: "dev", date: "May 4, 2026", ref: "f02ce91",
    summary: "Per-agent model and effort selection.",
    commits: [
      { type: "feat", hash: "77a0e3c", scope: "agents", msg: "pick tool + model per task — Claude, Codex, Cursor, Gemini" },
      { type: "feat", hash: "5d2b9af", scope: "codex",  msg: "expose Codex effort levels (low / medium / high)" },
      { type: "fix",  hash: "3c4471e", scope: "ci",     msg: "refresh-token migration agent no longer stalls on queued CI" },
    ],
  },
  {
    tag: "v0.8.5", channel: "dev", date: "April 18, 2026", ref: "9c0b215",
    summary: "Multi-project orchestration and isolated worktrees.",
    commits: [
      { type: "feat", hash: "10ad77b", scope: "core",   msg: "run agents across many git repos from one console" },
      { type: "feat", hash: "2bf8e90", scope: "git",    msg: "give every agent its own isolated git worktree + branch" },
      { type: "feat", hash: "6e1c004", scope: "viz",    msg: "Kanban board grouped by agent status" },
      { type: "fix",  hash: "d90f7a3", scope: "status", msg: "status dots distinguish queued from waiting correctly" },
    ],
  },
  {
    tag: "v0.8.0", channel: "dev", date: "March 30, 2026", ref: "4471ee0",
    summary: "First public preview of the Orchestrator.",
    commits: [
      { type: "feat", hash: "0a12f6d", scope: "orchestrator", msg: "live agent cards with progress rings and mini terminals" },
      { type: "feat", hash: "88c3b21", scope: "agents",       msg: "spawn an agent into a project with a single click" },
    ],
  },
];

Object.assign(window, { RELEASES });
