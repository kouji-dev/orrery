# Agent hooks — reliable status + remote permission approval

Status: **implemented** — commits d728dd3 (`AgentAdapter` trait + detection) and
975baff (loopback bridge + permission round-trip), runtime follow-up 55bebef.

**Deviation:** the "tiny no-deps `kat-hook` binary" shipped instead as a proper
CLI subcommand of the main app binary — `katrix hook --event <EVENT>`
(`src-tauri/src/cli/`, clap; transport std-only). Same logic, but always
co-located with the app, so no sidecar to bundle. `hook_binary()` returns
`current_exe()`; the adapter hook configs invoke `"<exe>" hook --event <EVENT>`,
with connection/identity via the `KATRIX_*` env stamped on the agent process and
the payload via stdin (the agents' own hook protocol).

## Goal

Replace the unreliable frontend PTY-parsing heuristic for "needs permission /
question / done" with the agents' **native hook systems**. The same blocking
pre-tool hook gives us BOTH reliable detection AND the reverse path (approve a
permission from the notification, forwarded back into the agent — no fake `y\r`
keystroke). PTY parsing stays only as a fallback floor for un-hooked tools.

All three target agents support **synchronous, blocking** pre-tool hooks that
return an allow/deny decision:

| Tool | Hook (blocking) | Allow output | Deny output | Config location |
|---|---|---|---|---|
| Claude | `PreToolUse` (timeout 600s) | `{"hookSpecificOutput":{"hookEventName":"PreToolUse","permissionDecision":"allow"}}` | `…"deny"` | worktree `.claude/settings.json` |
| Codex | `PreToolUse` / `PermissionRequest` | same `hookSpecificOutput` schema | `…"deny"` or `{"decision":"block"}` | managed `CODEX_HOME` (env) |
| Cursor | `beforeShellExecution` / `beforeMCPExecution` | `{"permission":"allow"}` | `{"permission":"deny"}` / exit 2 | worktree `.cursor/hooks.json` |

Other hooks give status: `Stop` = done, `UserPromptSubmit`/`PreToolUse` = working.

## Requirements (from the user)

1. **Use the hooks logic** (blocking-hook bridge below), not PTY parsing, for the
   permission/question/done signals.
2. **An abstraction trait implemented per agent** — one Rust trait (e.g.
   `AgentAdapter`) with a concrete impl per tool (claude/codex/cursor/gemini…).
   All per-tool differences (launch command, hook install, decision JSON format,
   config strategy) live behind it.
3. **Initial detection of which agents are installed** on the user's machine
   (formalize/extend the current `which`-based `detect_tools`), surfaced through
   the trait so only-installed tools are offered/hooked.
4. Keep **PTY parsing as the fallback** for tools without usable hooks.

## The trait

```rust
pub trait AgentAdapter: Send + Sync {
    fn id(&self) -> &str;                       // "claude" | "codex" | "cursor" | "gemini"
    fn binary(&self) -> &str;                   // executable name for `which` + spawn
    fn is_installed(&self) -> bool;             // detection (which on PATH)
    fn build_command(&self, task: Option<&str>) -> CommandBuilder; // launch (prompt once)
    /// Install the blocking pre-tool hook so the agent calls our `kat-hook`.
    /// `env` carries KATRIX_AGENT_ID/ENDPOINT/TOKEN/TOOL.
    fn install_hooks(&self, worktree: &Path, env: &HookEnv) -> std::io::Result<()>;
    /// Per-tool stdout JSON for an allow/deny decision (used by kat-hook).
    fn format_decision(&self, allow: bool, reason: &str) -> String;
}
```
A registry returns the adapters; `detect_tools` becomes "list adapters where
`is_installed()`".

## The permission round-trip (blocking-hook bridge)

1. Agent fires its pre-tool hook synchronously and **blocks**.
2. The hook command is our tiny no-deps **`kat-hook`** binary (installed by the
   adapter, env-stamped at spawn). It reads the tool request on stdin, POSTs
   `{agentId, tool, input, requestId}` to the loopback server, and **waits**.
3. Backend registers `requestId → oneshot`, emits `agent://permission
   {requestId, agentId, tool, detail}`.
4. UI notification shows the real command + Accept / Reject / Open terminal.
5. `agent_permission_decide(requestId, allow|deny)` resolves the held request →
   the HTTP response returns the decision to `kat-hook`.
6. `kat-hook` prints the adapter's `format_decision(...)` JSON, exits 0.
7. The agent allows/denies accordingly.
8. Timeout (< Claude's 600s, ~9 min): `kat-hook` returns `ask`/`defer` so the
   agent falls back to its own native prompt — nothing hangs.

## Pieces to build

- **Loopback HTTP server** (re-add; the legit, hook-driven rebirth of the removed
  `kat`/`ipc`): token-auth, holds requests pending on a `requestId → oneshot`
  registry with a timeout.
- **`kat-hook` binary** (`src-tauri/src/bin/kat_hook.rs`, no deps): stdin → POST →
  wait → print per-`KATRIX_TOOL` decision JSON.
- **`AgentAdapter` trait + impls** (`src-tauri/src/agents/adapters/`): claude,
  codex, cursor, gemini. Hook install scoped to the worktree (claude/cursor) or a
  managed home (codex); env-stamp `KATRIX_AGENT_ID/ENDPOINT/TOKEN/TOOL`.
- **Backend**: `agent://permission` event; `agent_permission_decide` command;
  status events from `Stop`/`UserPromptSubmit` hooks → update agent status.
- **Frontend**: notification carries `requestId`; Accept/Reject call
  `agent_permission_decide` (replace the fake `y\r`); a `permission`-kind
  notification renders the structured tool/command detail.
- **Fallback**: keep the PTY title/output parsing as the floor for un-hooked
  tools; remove it as the *primary* permission signal.

## Non-goals (for now)

- Editing the user's real **global** agent config — everything is worktree-scoped
  or managed-home, never the user's `~/.claude` etc.
