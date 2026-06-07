# Agent hooks — reliable status + remote permission approval

Status: **implemented, then simplified to fire-and-forget** (commit a6221bb).
After checking how stablyai/orca does it (its hook server acks fire-and-forget,
HTTP 204 — it does NOT forward decisions; the user approves in the agent's TUI),
we matched that: the loopback bridge stays for **status + needs-input detection**
but no longer **holds** the agent. The blocking allow/deny round-trip described
below is **deferred** — kept here as the design for if/when we add remote approve.

What's live now (a6221bb + the `AgentEvent` taxonomy refactor):
- Bridge acks every hook immediately (204, no body); the agent never blocks.
- Hook events are status/needs-input/activity only.

### The `AgentEvent` taxonomy (current)

The raw `(hook name, payload)` an agent fires is parsed in
`hooks/protocol.rs::parse()` into ONE generalized, structured cross-agent
`AgentEvent` variant. This **supersedes** the old squeezed
`Working/NeedsInput/Done/Other` four-way classify — each variant now carries the
fields we care about instead of collapsing everything to a status word.

| Variant | Carries | Notes |
|---|---|---|
| `SessionStart { source }` | source | Claude/Cursor `SessionStart`, Gemini `BeforeAgent` |
| `UserPrompt { text }` | prompt text | turn start |
| `AgentMessage { text }` | assistant text | Claude `MessageDisplay` (`delta`), Cursor `afterAgentResponse`/`afterAgentThought` (💭), Gemini `AfterModel`, Codex `SubagentStop` |
| `ToolStart { tool, input }` | **`ToolInput`** | tool about to run |
| `ToolEnd { tool, status, result }` | **`ToolStatus`** + hint | status re-derived from payload (success/exit_code/error → Ok/Failed/Denied) |
| `PermissionRequest { tool, input, mode, suggestions }` | **`ToolInput` + `Vec<Suggestion>`** | the FULL permission detail — replaces the old squeezed `NeedsInput` |
| `Notification { kind, message }` | kind + message | Claude/Gemini `Notification`; a Claude `permission_prompt` is promoted to `PermissionRequest` |
| `TurnEnd` | — | Stop / Gemini `AfterAgent` → idle |
| `SessionEnd { reason }` | reason | → idle |
| `Compact { phase }` | phase | PreCompact/PostCompact — acknowledged, no emit |
| `Error { kind, message }` | message | feed line, status unchanged |
| `Unknown` | — | unmapped hook name; raw payload still reachable via the envelope |

Structured payload types: `ToolInput { command, description, file_path, raw }`
(the most-telling field is lifted; `raw` keeps the agent's full tool input so
nothing is lost) and `Suggestion { behavior, rule, description }` (one flattened
Claude `permission_suggestions` entry). `AgentEvent::activity_detail()` renders
the one-line preview per variant (e.g. `▸ Bash: npm test`, `Edit ✓`,
`✋ Bash: git push`).

### Installed hook set per tool (current)

| Tool | Config | Installed events |
|---|---|---|
| Claude | `~/.claude/settings.json` (merge) | `Notification`, `PermissionRequest`, `UserPromptSubmit`, `PreToolUse`, `PostToolUse`, `PostToolUseFailure`, `MessageDisplay`, `SessionStart`, `SessionEnd`, `Stop` |
| Codex | `~/.codex/config.toml` (flat keys, `toml_edit` merge) | `pre_tool_use`, `post_tool_use`, `permission_request`, `session_start`, `stop` |
| Cursor | `~/.cursor/hooks.json` (merge) | `beforeShellExecution`, `beforeMCPExecution`, `afterShellExecution`, `afterFileEdit`, `postToolUseFailure`, `afterAgentResponse`, `afterAgentThought`, `beforeSubmitPrompt`, `sessionStart`, `sessionEnd`, `stop` |
| Gemini | `~/.gemini/settings.json` (merge, Claude-shaped groups) | `BeforeTool`, `AfterTool`, `BeforeAgent`, `AfterAgent`, `AfterModel`, `Notification`, `SessionStart`, `SessionEnd` |

- **Gemini IS now hooked** (this reverses the old "no usable hook surface" note —
  see the superseded global-install section). Its group shape matches Claude's, so
  it reuses `merge_json_hooks`. **Gap:** Gemini has NO permission "ask" event, so
  katrix raises NO permission card for it — a tool denial only surfaces as a
  result-derived `ToolEnd(Denied)` ("✗") via `AfterTool`; approval stays in
  gemini's own TUI.
- **Cursor** has NO dedicated permission EVENT either — its allow/deny is returned
  inline from the `before*` hooks — so katrix surfaces no `PermissionRequest` for
  cursor (PTY fallback).
- **Codex** keeps the established flat `[hooks] pre_tool_use/post_tool_use/stop`
  keys (the newer docs show an `[[hooks.EventName]]` array-table schema mirroring
  Claude). `permission_request` (its documented dedicated ask) + `session_start`
  were added under the same flat convention; codex offers no suggestions, so its
  `PermissionRequest.suggestions` is always empty. See `adapters/codex.rs`.
- **Only `PermissionRequest` (and a Claude `permission_prompt` Notification) ever
  raise a permission card.** Every lifecycle / post-tool / message hook is
  activity/status-only and never raises permission — `handle()` branches on the
  `AgentEvent` variant, not the raw hook name.

### The `agent://permission` payload contract (current)

`handle()` → `emit_permission()` emits the FULL structured detail:

```jsonc
{
  "agentId": "a1",            // which agent is asking
  "tool":    "Bash",          // tool_name, or the agent label
  "mode":    "default",       // permission_mode, or null
  "command":     "git push",  // tool_input.command, or null
  "description": "…",         // tool_input.description, or null
  "filePath":    "src/foo.rs",// tool_input.file_path, or null
  "suggestions": [            // flattened permission_suggestions (empty when absent)
    { "behavior": "allow", "rule": "Bash(git push:*)", "description": "Always allow git push" }
  ]
}
```

`permission_suggestions` are **Claude-only**: captured by `parse_suggestions()`
and displayed as "always allow/deny" rule chips. The act-on-suggestion /
decision round-trip is **still DEFERRED** — emitting a permission is
fire-and-forget; the agent is never held and you still approve in its TUI.

### Activity / preview pipeline (current)

- `handle()` PREFERS the REAL latest message content scraped from the agent's
  transcript (`transcript::latest_content`, Claude's `transcript_path`) so the
  card mirrors the live terminal (assistant prose / thinking / tool use); it
  falls back to the variant's structured `activity_detail()` for agents/events
  with no transcript (gemini, cursor's own shapes, …). `PermissionRequest` is
  special — its ✋ line always reflects the REQUEST, ignoring transcript content.
- `AgentMessage` (Claude `MessageDisplay` / Cursor `afterAgentResponse` / Gemini
  `AfterModel`) feeds the card preview directly.
- **Backend dedup + non-empty gate:** `agent://activity` is emitted ONLY when the
  detail is non-empty AND differs from the last detail for that agent (the
  transcript re-read returns the same latest message across many hooks, so this
  collapses 1000+ identical msgs to one emit). Payload now also carries the
  precise hook `event` as metadata:
  `agent://activity { agentId, tool, event, detail }`.
- **Frontend keeps the last 10** activity entries per agent (rolling, consecutive
  dupes skipped), driving the overview mini-term preview —
  `agent-runtime.service.ts`.
- `agent://status { id, state }` is the working/idle ping (working on
  message/tool/prompt/session-start; idle on TurnEnd/SessionEnd).
- Frontend raises a "needs your input" notification; Accept/Reject are
  best-effort keystrokes, **Open terminal** is the reliable path.
- Removed: pending registry, oneshot hold, decide timeout, `decide`,
  `agent_permission_decide`, the `format_decision`/`Decision` adapter contract.

History — commits d728dd3 (`AgentAdapter` trait + detection), 975baff (the
blocking bridge), 55bebef (runtime follow-up). The blocking design below lived in
975baff; restore from there if we implement remote approve.

---


**Deviation:** the "tiny no-deps `kat-hook` binary" shipped instead as a proper
CLI subcommand of the main app binary — `katrix hook --event <EVENT>`
(`src-tauri/src/cli/`, clap; transport std-only). Same logic, but always
co-located with the app, so no sidecar to bundle. `hook_binary()` returns
`current_exe()`; the adapter hook configs invoke `"<exe>" hook --event <EVENT>`,
with connection/identity via the `KATRIX_*` env stamped on the agent process and
the payload via stdin (the agents' own hook protocol).

> **SUPERSEDED (historical) from here down.** Everything below is the original
> BLOCKING design (held requests, `format_decision`, the `requestId → oneshot`
> round-trip, worktree-scoped/managed-home install). It is kept for reference if we
> ever add remote approve. What actually shipped is described in "What's live now"
> at the top: fire-and-forget hooks, the `AgentEvent` taxonomy, global merge-install
> (incl. Gemini), and the `agent://permission` contract. The trait below also drops
> `format_decision` and uses `argv`/`allow_keys`/`deny_keys` + `install_hooks(home,
> hook_bin)` in the real code.

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

## Global hook install (reversed from the original non-goal)

The original design avoided the user's real global config (worktree-scoped /
managed-home only). That is now **reversed**: katrix installs its status +
needs-input hooks **globally**, merged **non-destructively** into the user's real
config on app startup (`install_global_hooks`):

- Claude → `~/.claude/settings.json` (merge via `merge_json_hooks`)
- Cursor → `~/.cursor/hooks.json` (merge; home-dir hooks apply across all
  projects per Cursor's docs)
- Codex → `~/.codex/config.toml` (merge via `toml_edit`, preserving other
  keys/comments; no more managed `CODEX_HOME`)
- ~~Gemini → no-op (no hook surface yet)~~ **SUPERSEDED:** Gemini IS now hooked —
  `~/.gemini/settings.json` (merge via `merge_json_hooks`, Claude-shaped groups);
  see the "Installed hook set per tool" table above. The only remaining gap is
  that Gemini has no permission "ask" event.

The merge preserves every existing key + the user's own hooks, and is idempotent.

**The env-presence check is the registration gate.** A globally-installed hook is
harmless for runs katrix didn't launch: `katrix hook` only brokers when the
`KATRIX_*` env (endpoint + token + agent-id) is present — see
`cli::hook::should_broker`. katrix stamps that env only on agents it launches, so
a user's own plain CLI session in an unregistered project is a silent no-op.

### Non-goals (for now)

- **Marking unregistered projects as "candidates"** when a global hook fires
  outside a katrix run (e.g. surfacing "hook up this project to katrix?") —
  future work. Today, absent the `KATRIX_*` env, the hook simply no-ops.
