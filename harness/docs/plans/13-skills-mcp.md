# 13 · Skills and MCP — adopt both as specified, add only the governance

**Goal.** Two extension mechanisms that already have industry-standard shapes. Take `SKILL.md` unchanged so the agentskills.io ecosystem works, and put MCP behind the same policy check as everything else with no second namespacing scheme. Add exactly one thing to each: a skill's `scripts/` run under a capability grant, and an MCP server is an extension id. When this is done, an existing `SKILL.md` and an existing MCP server both work unmodified.

**Covers.** §4.11 in full.

**Crates.** `core/crates/orrery-skills` · `core/crates/orrery-mcp`.

**Depends on.** [`01`](01-proto-shared-types.md), [`04`](04-tool-registry.md), [`06`](06-extension-host.md) (`orrery-jsonrpc`), [`07`](07-policy-broker-audit.md), [`10`](10-config-layers.md) (discovery).

---

## Constraints

From [`00-overview.md`](00-overview.md):

- **Adopt, do not extend.** A skill that works in Claude Code or Codex works here unchanged. If we need a field they do not have, that is a reason to think again.
- MCP registers as extension id `mcp.<server>` — **no second namespacing scheme**.
- Discovery and connection are different things: a server is *discovered* at session start, always, and *connected* when first needed.
- A tool set that grows after the manifest was approved is exactly what the manifest exists to prevent.

---

## Architecture

### Skills

Adopt the `SKILL.md` format unchanged — the agentskills.io spec already works across Pi, Claude Code, Cursor and Codex.

```rust
pub struct SkillRef {
    pub name: String,
    pub source: SkillSource,          // Builtin | User | Workspace | Extension | Registry
    pub path: PathBuf,
    // The only additions over the bare spec:
    pub scope: Vec<AgentScope>,       // which agents may load it
    pub grant: Option<GrantSpec>,     // scripts/ runs under this, not the user's shell
}
```

**The one addition that matters.** Elsewhere a skill's bundled `scripts/` run with the user's full privileges, which makes a skill a **better attack vector than an extension because it looks like documentation**. Here they run under a declared grant, brokered, audited, budgeted — the same path a tool takes.

Parsing is YAML front matter (`serde_yaml_ng`) plus a Markdown body. Nothing clever: unknown front-matter keys are preserved and ignored, because the spec will grow and we do not own it.

### MCP

Client and server, both brokered.

```rust
#[async_trait]
pub trait McpBroker: Send + Sync {
    async fn connect(&self, spec: &McpServerSpec, grant: Grant) -> Result<McpSession, McpError>;
    /// Tools land in the registry namespaced: mcp.<server> + <tool>
    fn register(&self, s: &McpSession) -> Vec<ToolRef>;
    /// Orrery as an MCP server.
    fn expose(&self, scope: &AgentScope) -> McpServerHandle;
}
```

Three decisions, straight from §4.11:

1. **Namespaced like everything else.** A server registers as extension id `mcp.<server>`, so its tools are ordinary `ToolRef`s — `mcp.jira` + `create_issue` — inheriting namespacing, precedence, policy and audit unchanged. A server whose tool collides with an extension's is a **resolution event, not a failure** (plan 04).
2. **Behind the same policy.** An MCP server is remote code with network access: it gets a grant, its calls are audited, and managed config can pin the allowlist — matching what Codex already does.
3. **Connection lifecycle is the core's.** Discovered at session start, always. Connected when first needed. Health-checked, and a dead one **degrades its tools rather than stalling turns**.

**`list_changed` does not silently extend the session.** The new tools are re-resolved against policy and either admitted and recorded in the ledger, or refused. This is the rule that keeps an approved manifest meaningful.

Transports: stdio (on `orrery-jsonrpc`'s `LineDelimited` framing — the reason that variant exists) and streamable HTTP. Both brokered: stdio spawns through the broker's `spawn`, HTTP goes through `net`.

### Orrery as an MCP server

`expose(scope)` publishes the **visible set for that scope** (plan 04's `visible()`), so exposing Orrery to another tool cannot leak tools the scope could not call itself. The same policy check runs on inbound calls.

---

## File structure

**Create**

- `harness/core/crates/orrery-skills/src/{lib,parse,discover,scripts,scope}.rs`
- `harness/core/crates/orrery-skills/tests/{parse,scripts}.rs`
- `harness/core/crates/orrery-skills/tests/fixtures/` — real `SKILL.md` files from the wild
- `harness/core/crates/orrery-mcp/src/{lib,client,server,transport,register,health,expose}.rs`
- `harness/core/crates/orrery-mcp/tests/{client,register,health,expose}.rs`

---

## Tasks

### Task 1 · SKILL.md parsing

Files: `orrery-skills/src/parse.rs`, `tests/parse.rs`

- [ ] **Failing test first.** `parse::real_skills_from_the_wild` — a table over committed fixtures taken from actual published skills; each parses with the expected name and description. **If one fails, we are extending rather than adopting.**
- [ ] `parse::unknown_frontmatter_is_preserved` — a key we do not know survives a round trip and does not error.
- [ ] `parse::missing_name_is_an_error_naming_the_file`.
- [ ] Implement with `serde_yaml_ng`.

### Task 2 · Discovery and scoping

Files: `orrery-skills/src/{discover,scope}.rs`

- [ ] **Failing test first.** `discover::found_across_layers` — skills in user, workspace and project layers are all found in the single discovery pass (plan 10 task 4).
- [ ] `discover::scope_limits_which_agents_load_it` — a skill scoped to one agent is absent from another's context.
- [ ] Implement.

### Task 3 · `scripts/` under a grant

Files: `orrery-skills/src/scripts.rs`, `tests/scripts.rs`

- [ ] **Failing test first, and it is the security-relevant one.** `scripts::run_under_the_declared_grant` — a skill whose script tries to write outside its grant is **denied**, and the denial is audited. Compare with the counterfactual documented in §4.11: elsewhere this script would have had the user's full shell privileges.
- [ ] `scripts::no_grant_means_no_scripts` — a skill with no declared grant cannot run scripts at all.
- [ ] `scripts::budgeted` — a script that runs forever is stopped by the tool budget.
- [ ] Implement over the broker's `spawn`.

### Task 4 · MCP client — stdio

Files: `orrery-mcp/src/{client,transport}.rs`, `tests/client.rs`

- [ ] **Failing test first.** `client::real_server_works_unmodified` — spawn an actual off-the-shelf MCP server (a small published one, vendored as a test fixture), list its tools, call one. **This is the phase-7 acceptance criterion; write it first and let it fail.**
- [ ] `client::initialize_handshake` — protocol version negotiation.
- [ ] `client::stdio_is_line_delimited` — uses `orrery-jsonrpc`'s `LineDelimited`, not `ContentLength`.
- [ ] Implement.

### Task 5 · MCP client — HTTP

Files: `orrery-mcp/src/transport.rs`

- [ ] **Failing test first.** `client::http_transport` — against a local test server.
- [ ] Implement streamable HTTP through the broker's `net`.

### Task 6 · Registration and namespacing

Files: `orrery-mcp/src/register.rs`, `tests/register.rs`

- [ ] **Failing test first.** `register::no_second_scheme` — `mcp.jira`'s `create_issue` resolves through the ordinary registry as `ToolRef { ext: "mcp.jira", name: "create_issue" }`, and `mcp.jira.create_issue` parses to it.
- [ ] `register::collision_is_a_resolution_event` — an extension tool and an MCP tool with the same short name both survive; the ledger records the choice.
- [ ] `register::calls_are_policy_checked` — an MCP call with no grant is denied, exactly like a local tool.
- [ ] Implement.

### Task 7 · Lifecycle and health

Files: `orrery-mcp/src/health.rs`, `tests/health.rs`

- [ ] **Failing test first.** `health::discovered_but_not_connected` — at session start the server is in the manifest; assert **no process was spawned** until a tool is called.
- [ ] `health::dead_server_degrades` — kill the server mid-session; its tools report `Failed`/`Unloaded` and the **turn continues**; assert no turn stalled waiting.
- [ ] `health::reconnects` — a server that comes back is usable again.
- [ ] Implement.

### Task 8 · `list_changed`

Files: `orrery-mcp/src/register.rs`

- [ ] **Failing test first, and it is the manifest-integrity one.** `register::list_changed_is_re_resolved` — a server adds a tool mid-session; assert the new tool is checked against policy and either admitted **and recorded in the ledger** or refused, never silently available.
- [ ] Implement.

### Task 9 · Orrery as an MCP server

Files: `orrery-mcp/src/expose.rs`, `tests/expose.rs`

- [ ] **Failing test first.** `expose::only_the_visible_set` — expose under a narrow scope; assert tools outside `visible(scope)` are not listed and not callable.
- [ ] `expose::inbound_calls_are_policy_checked`.
- [ ] Implement.

---

## Done when

- `cargo test -p orrery-skills -p orrery-mcp` green.
- An off-the-shelf MCP server and a published `SKILL.md` both work with no modification.
- A skill's script is demonstrably confined by its grant.
- A dead MCP server degrades instead of stalling a turn.
- `list_changed` cannot grow the tool set silently.

## Open questions

1. **Which real MCP server to vendor as a fixture.** It needs to be small, stable and permissively licensed. Pick one and pin it; the test is worth more than its maintenance cost.
2. **Skill `scripts/` grant declaration syntax.** The bare spec has no place for it. Options: a sidecar `skill.toml`, a reserved front-matter key, or config-side scoping only. Config-side keeps `SKILL.md` untouched, which is the adoption promise — **lean that way** and record it.
3. **MCP protocol version drift.** The spec moves. Pin a version, test against it, and add a drift check like the AG-UI one (plan 08 task 2)?
4. **Exposing Orrery over MCP — authentication.** Same gap as plan 08's HTTP listener. Do not solve it twice; share the answer.
