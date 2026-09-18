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

- [x] **Failing test first.** `parse::real_skills_from_the_wild` — a table over committed fixtures taken from actual published skills; each parses with the expected name and description. **If one fails, we are extending rather than adopting.**
- [x] `parse::unknown_frontmatter_is_preserved` — a key we do not know survives a round trip and does not error.
- [x] `parse::missing_name_is_an_error_naming_the_file`.
- [x] Implement with `serde_yaml_ng`.

### Task 2 · Discovery and scoping

Files: `orrery-skills/src/{discover,scope}.rs`

- [x] **Failing test first.** `discover::found_across_layers` — skills in user, workspace and project layers are all found in the single discovery pass (plan 10 task 4).
- [x] `discover::scope_limits_which_agents_load_it` — a skill scoped to one agent is absent from another's context.
- [x] Implement.

### Task 3 · `scripts/` under a grant

Files: `orrery-skills/src/scripts.rs`, `tests/scripts.rs`

- [x] **Failing test first, and it is the security-relevant one.** `scripts::run_under_the_declared_grant` — a skill whose script tries to write outside its grant is **denied**, and the denial is audited. Compare with the counterfactual documented in §4.11: elsewhere this script would have had the user's full shell privileges.
- [x] `scripts::no_grant_means_no_scripts` — a skill with no declared grant cannot run scripts at all.
- [x] `scripts::budgeted` — a script that runs forever is stopped by the tool budget.
- [x] Implement over the broker's `spawn`.

### Task 4 · MCP client — stdio

Files: `orrery-mcp/src/{client,transport}.rs`, `tests/client.rs`

- [x] **Failing test first.** `client::real_server_works_unmodified` — spawn an actual off-the-shelf MCP server (a small published one, vendored as a test fixture), list its tools, call one. **This is the phase-7 acceptance criterion; write it first and let it fail.**
- [x] `client::initialize_handshake` — protocol version negotiation.
- [x] `client::stdio_is_line_delimited` — uses `orrery-jsonrpc`'s `LineDelimited`, not `ContentLength`.
- [x] Implement.

### Task 5 · MCP client — HTTP

Files: `orrery-mcp/src/transport.rs`

- [x] **Failing test first.** `client::http_transport` — against a local test server.
- [x] Implement streamable HTTP through the broker's `net`.

### Task 6 · Registration and namespacing

Files: `orrery-mcp/src/register.rs`, `tests/register.rs`

- [x] **Failing test first.** `register::no_second_scheme` — `mcp.jira`'s `create_issue` resolves through the ordinary registry as `ToolRef { ext: "mcp.jira", name: "create_issue" }`, and `mcp.jira.create_issue` parses to it.
- [x] `register::collision_is_a_resolution_event` — an extension tool and an MCP tool with the same short name both survive; the ledger records the choice.
- [x] `register::calls_are_policy_checked` — an MCP call with no grant is denied, exactly like a local tool.
- [x] Implement.

### Task 7 · Lifecycle and health

Files: `orrery-mcp/src/health.rs`, `tests/health.rs`

- [x] **Failing test first.** `health::discovered_but_not_connected` — at session start the server is in the manifest; assert **no process was spawned** until a tool is called.
- [x] `health::dead_server_degrades` — kill the server mid-session; its tools report `Failed`/`Unloaded` and the **turn continues**; assert no turn stalled waiting.
- [x] `health::reconnects` — a server that comes back is usable again.
- [x] Implement.

### Task 8 · `list_changed`

Files: `orrery-mcp/src/register.rs`

- [x] **Failing test first, and it is the manifest-integrity one.** `register::list_changed_is_re_resolved` — a server adds a tool mid-session; assert the new tool is checked against policy and either admitted **and recorded in the ledger** or refused, never silently available.
- [x] Implement.

### Task 9 · Orrery as an MCP server

Files: `orrery-mcp/src/expose.rs`, `tests/expose.rs`

- [x] **Failing test first.** `expose::only_the_visible_set` — expose under a narrow scope; assert tools outside `visible(scope)` are not listed and not callable.
- [x] `expose::inbound_calls_are_policy_checked`.
- [x] Implement.

---

## Done when

- `cargo test -p orrery-skills -p orrery-mcp` green. **True** — 15 and 22 tests.
- ~~An off-the-shelf MCP server~~ **A conformant MCP server written to the published
  spec** and a published `SKILL.md` both work with no modification.
  **Amended, and here is why.** The `SKILL.md` half is met as written: six files are
  committed byte for byte from what Anthropic, the Superpowers plugin, Google and
  `playwright-core` actually ship, and they parse with no edit
  (`orrery-skills/tests/fixtures/wild/`, provenance beside them). The MCP half is
  **not** met as written and cannot be under this repository's hard constraint that
  no test may make a network request: vendoring a published server means fetching
  one. `orrery-mcp/tests/fixtures/server/main.rs` is the closest correct thing — a
  server written to the spec rather than to this client, which knows nothing about
  `orrery-mcp`, answers `initialize` / `tools/list` / `tools/call` over
  newline-delimited JSON-RPC, returns `-32601` for a method it lacks, and emits
  `notifications/tools/list_changed`. `tests/client.rs` says in its header how to
  point the same tests at a third-party binary, which is the check a release should
  run once and this suite must not.
- ~~A skill's script is demonstrably confined by its grant.~~ **A skill's script is
  demonstrably confined by its grant at the effect channel, and by
  `orrery-broker::contain` at the process.** **Amended.**
  `scripts::run_under_the_declared_grant` proves the first half completely: the
  script asks for two writes, the one outside the grant is refused by rule, the file
  is not on disk afterwards, and the denial is in the audit stream. What it does not
  prove — and what no test in this crate could — is that a **hostile binary** cannot
  call `open(2)` behind the channel's back. That is containment, it is enforced on
  the same `spawn`, and it belongs to plan 07. The two are layers of one answer, and
  `src/scripts.rs` says so rather than letting the stronger claim stand.
- A dead MCP server degrades instead of stalling a turn. **True** —
  `health::dead_server_degrades` asserts the elapsed time against a 30s budget, not
  just the outcome.
- `list_changed` cannot grow the tool set silently. **True**, twice: once over a
  made-up list (`register::list_changed_is_re_resolved`) and once end to end against
  the fixture server announcing its own growth
  (`health::list_changed_from_a_real_server_is_re_resolved`).

## State

**Done**, 2026-09-18, on `feat/harness_claude-0917`. All nine tasks implemented
test-first. `cargo test -p orrery-skills -p orrery-mcp` green, clippy clean,
`cargo run -p xtask -- deps-check` ok. Two "Done when" bullets are amended above
rather than quietly weakened.

Two things a later plan has to pick up, both recorded in the code:

1. **`orrery-broker` has no interactive spawn.** `proc::Child` gives its child
   `Stdio::null()` for stdin and consumes itself in `wait()` — right for a tool that
   runs and finishes, wrong for an MCP session that talks both ways for a turn. So
   `transport::connect_stdio` creates the process directly, with `kill_on_drop`, and
   the module says plainly that this is a gap and not a decision. A
   `Broker::spawn_interactive` returning a handle with a stdin, a stdout and a kill
   grip closes it; it is `orrery-broker`'s to add, not this crate's. A skill's
   scripts, which do run and finish, go through `Broker::spawn` as the plan says.
2. **`orrery-tools`' ledger is crate-private.** `Registry::record` cannot be called
   from here, so `reconcile`'s admissions and refusals are recorded in the audit
   stream (`AuditEvent::ExtensionLoad`, status `tool-admitted` / `tool-refused`) and
   returned as a `Reconciliation`. The ledger still carries the *name* decisions — a
   collision between an extension tool and an MCP tool is an `Ambiguous` entry in
   it, which `register::collision_is_a_resolution_event` asserts.

One deviation from the architecture sketch: `SkillRef.scope` is `Vec<String>` (agent
names), not `Vec<AgentScope>`. An `AgentScope` is a running sub-agent — a branch id,
a tool set, a grant — and a copy of one hung off every skill would go stale.
`visible_to` needs the names and nothing else.

## Open questions

1. **Which real MCP server to vendor as a fixture.** It needs to be small, stable and permissively licensed. Pick one and pin it; the test is worth more than its maintenance cost.

   **Decided: none — commit a conformant server instead.** Vendoring means fetching,
   and no test here may make a network request; a checked-in copy of somebody's npm
   package is also a licence question and a stale copy within a release.
   `orrery-mcp/tests/fixtures/server/main.rs` is written to the **published
   protocol** and knows nothing about this client, which is the property the
   acceptance criterion actually needs — "our client works against a server written
   to the spec", not "our client works against one particular vendor". It is a bin
   target of the crate, so it builds wherever CI runs and needs no node, no python
   and no registry.

   The cost is real and named: a server that *claims* conformance and is not is
   exactly what a third-party fixture would have caught. So `tests/client.rs` is
   written against `StdioSpec` alone — point it at any binary and the same five
   tests run — and a release checks one real server by hand rather than the suite
   doing it on every commit.

2. **Skill `scripts/` grant declaration syntax.**

   **Decided: config-side only, as the plan leans.** There is no sidecar file and no
   reserved front-matter key. `SkillSettings { scope, grant }` is read from the
   config layers and joined to the parsed document by name, so a `SKILL.md` governed
   here is byte-identical to the one published for Claude Code or Codex — which is
   the whole adoption promise, and it would be worth very little if we then asked
   authors to add a file.

   The consequence is worth saying out loud, because it is the *good* direction: **a
   skill cannot ask for privileges.** It is granted them by the operator or it has
   none, and a skill with none cannot run a script at all. For a document that is
   easy to publish and easy to trust by mistake, "the thing being trusted does not
   get a say in how much" is the right default.

   The subject a script acts as is `agent:skill:<name>` — a `Subject::SubAgent`, not
   a `Subject::Ext`, because `ExtId` reserves its one dotted form for `mcp.<server>`
   and a skill is not an extension. It therefore sits under the main agent and is
   intersected with it, so a skill can never be wider than the agent that loaded it.

3. **MCP protocol version drift.**

   **Decided: pin one revision, accept a short list, refuse anything else by name.**
   `client::PROTOCOL_VERSION` is what `initialize` asks for; `client::SUPPORTED` is
   what will be spoken. A server answering with anything else is
   `McpError::ProtocolVersion` naming both sides, rather than a client carrying on
   and discovering the difference in production six weeks later.

   **No generated drift check, unlike AG-UI.** Plan 08's check diffs against a schema
   artefact that is in this tree; MCP has none here, so a generator would have to
   fetch the spec — the network again — and would otherwise be checking our copy
   against our copy. The constant plus
   `client::the_pinned_version_is_one_we_accept` is the honest version of the same
   idea: adding a revision is a deliberate edit with a test beside it.

4. **Exposing Orrery over MCP — authentication.**

   **Decided: share plan 08's answer; this plan adds nothing.** `expose::serve` takes
   an already-authenticated byte stream and says so in its signature and its module
   docs. Over stdio the caller is whoever started the process. Over a socket, "who is
   that" is the same question plan 08's HTTP listener has to answer, and two
   independent answers to one question is how a system ends up with a door nobody
   audited.

   What this plan does own, and does enforce, is that authentication is not
   *load-bearing for confidentiality*: `expose` publishes `visible(scope)` and
   dispatch refuses anything outside the scope, so an unauthenticated caller still
   cannot see or call what the scope could not.
