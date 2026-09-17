# 10 · Configuration — five layers, one effective config, and proof of where every value came from

**Goal.** Resolve managed, organisation, user, workspace and project into one effective configuration; refuse to load project-supplied code until the project is trusted; make "configure your own harness" concrete through profiles; and answer "where did this value come from" for any key. When this is done, two profiles produce measurably different agents from one binary.

**Covers.** §4.9 in full · the `[profile.*]` half of §4.6 · §7's "set in" column.

**Crate.** `core/crates/orrery-config`.

**Depends on.** [`01`](01-proto-shared-types.md), [`07`](07-policy-broker-audit.md) (rules are config values).

---

## Constraints

From [`00-overview.md`](00-overview.md):

- **Deny is a union across layers; a managed deny cannot be relaxed.** Allow and ask resolve by layer precedence.
- Names resolve closest-layer-first; permissions resolve managed-wins. Opposite directions, on purpose.
- Trust gating: **project-local config, extensions and interceptors do not load until the project is trusted.** Cloning a repository must not be equivalent to running its code.
- The startup order is fixed and one-directional, so nothing a project could supply gets a vote on whether the project is trusted.

---

## Architecture

### The layers

| Layer | Location | Who writes it | Overridable |
|---|---|---|---|
| Managed | OS-managed path | IT / security | No |
| Organisation | Signed, fetched from registry | Platform team | Only where marked |
| User | `~/.orrery/config.toml` | The developer | Yes |
| Workspace | `.orrery/config.toml` | The team, committed | Yes |
| Project | Nested `.orrery/`, closest wins | The developer | Yes |

OS-managed paths: `%ProgramData%\Orrery\managed.toml` (Windows), `/Library/Application Support/Orrery/managed.toml` (macOS), `/etc/orrery/managed.toml` (Linux).

### The startup order — trust and discovery depend on each other

Fixed, one-directional, and the whole point:

1. Read the **managed, organisation and user** layers. These need no trust decision — the developer or the administrator wrote them.
2. Resolve **trust** for the workspace from those layers alone, plus the stored answer for this path. An untrusted workspace **stops here** and the session runs with user-level config only.
3. **Discover** — one pass over extensions, skills, prompts and MCP servers across the layers now in force.
4. **Load and validate**: agent parameter schemas, role bindings, singleton conflicts. A missing binding or an unknown parameter fails here, **naming the file and line**.
5. Fire `session.start` — interceptors first for verdicts on the resolved manifest, then lifecycle handlers for their I/O.

An extension therefore cannot influence the trust decision that governs whether it loads. Codex does the same; so should we.

```rust
pub fn resolve(ctx: &StartupCtx) -> Result<ResolvedConfig, ConfigError>;

pub struct ResolvedConfig {
    pub profile: Profile,
    pub values: Provenanced,          // every value carries its layer + file + line
    pub trust: TrustState,
    pub manifest: DiscoveryManifest,  // what the ledger reports
}
```

### Provenance is not an add-on

Every value keeps where it came from, or `config explain` is a lie:

```rust
pub struct Provenanced { /* key → (Value, Origin) */ }
pub struct Origin { pub layer: Layer, pub file: PathBuf, pub line: u32 }
```

That means the merge cannot use `serde` deserialize-into-one-struct. It merges `toml_edit` documents, keeping spans, then deserializes once at the end.

### Profiles

A profile is a named composition of everything the runtime assembles:

```toml
[profile.review]
model = "claude-sonnet-5"
extensions = ["git", "lsp", "buildgraph"]
interceptors = ["no-write-outside-diff"]
subagents = ["critic"]
skills = ["review-checklist"]
permissions = { write = false }

[profile.ci]
model = "local/qwen-coder"
extensions = ["git", "test-runner"]
consent = "never"                     # no human present; deny instead of prompt
audit = { sink = "otlp://collector.internal" }
```

Plus, from §4.6, the things that make token cost a configuration question rather than prompt engineering:

```toml
[agents.planner]
use    = "myteam.architect"
model  = "claude-sonnet-5"
params = { maxIterations = 2, depth = 3 }

[agents.reviewer]
prompt = "Review the diff for correctness and test coverage. Do not edit."
model  = "local/qwen-coder"
tools  = ["git.*", "lsp.*"]
budget = { maxTurns = 4, maxTokens = 60000, wallClockMs = 120000 }
```

Role binding validation happens at step 4 — **a role bound to an agent that does not exist fails at `session.start`, not mid-turn.**

### Import

`orrery import` reads an existing Claude Code or Codex setup — permissions, MCP servers, skills — into the equivalent layers. We already parse both formats in the ADE (`ade/src-tauri/src/agents/adapters/{claude,codex}.rs` knows their config shapes), so this is mapping, not reverse engineering.

It is a **one-way, explicit** command that writes a config file for the user to review. It never reads a foreign config at runtime.

---

## File structure

**Create**

- `harness/core/crates/orrery-config/src/{lib,layer,merge,provenance,trust,profile,discover,validate,explain,import}.rs`
- `harness/core/crates/orrery-config/tests/{merge,trust,profile,import}.rs`

---

## Tasks

### Task 1 · Layer loading and merge

Files: `src/{layer,merge,provenance}.rs`, `tests/merge.rs`

- [ ] **Failing test first.** `merge::closest_layer_wins_for_values` — the same key at user and project; project wins, provenance says so.
- [ ] `merge::deny_is_a_union` — denies from three layers all survive the merge.
- [ ] `merge::managed_deny_cannot_be_relaxed` — a user `allow` for something managed denies; the deny stands and the attempt is logged.
- [ ] `merge::nested_project_closest_wins` — `.orrery/` at two depths; the nearer one wins.
- [ ] Implement over `toml_edit` with spans preserved.

### Task 2 · Trust gating

Files: `src/trust.rs`, `tests/trust.rs`

- [ ] **Failing test first, and it is the security-relevant one.** `trust::untrusted_project_loads_no_project_config` — a workspace with a `.orrery/config.toml` declaring an extension; untrusted; assert the extension is **not** discovered, the session runs with user-level config, and the ledger says why.
- [ ] `trust::project_cannot_grant_itself_trust` — a project config containing `trust = true` has no effect.
- [ ] `trust::decision_is_stored_per_path`.
- [ ] Implement, with the stored answers in the user layer.

### Task 3 · Startup order

Files: `src/lib.rs`

- [ ] **Failing test first.** `startup::order_is_fixed` — instrument each step and assert the sequence 1→5 on every run, including when trust fails at step 2 (steps 3–5 run with the reduced layer set).
- [ ] Implement `resolve`.

### Task 4 · Discovery

Files: `src/discover.rs`

- [ ] **Failing test first.** `discover::one_pass` — extensions, skills, prompts and MCP servers are found in a single traversal; assert the filesystem is walked once (instrument the walker).
- [ ] `discover::manifest_matches_the_ledger` — what discovery produces is what the load ledger reports. §4.11: "if it is in the model's visible set, it is in the manifest."
- [ ] Implement.

### Task 5 · Validation with file and line

Files: `src/validate.rs`

- [ ] **Failing test first.** `validate::unknown_param_names_the_file_and_line` — an agent binding with a typo'd parameter fails at load with `config.toml:14`, not a generic serde error.
- [ ] `validate::missing_role_binding_fails_at_session_start`.
- [ ] `validate::singleton_conflict_is_reported` — two `router` claims; the winner by precedence, the loser named.
- [ ] Implement, using the retained spans.

### Task 6 · Profiles

Files: `src/profile.rs`, `tests/profile.rs`

- [ ] **Failing test first, and it is the phase-5 criterion.** `profile::two_profiles_differ_measurably` — build a kernel from `review` and from `ci`; assert different visible tool sets, different models, and that `review` cannot write while `ci` never prompts.
- [ ] `profile::consent_never_denies_instead_of_prompting`.
- [ ] Implement `Profile`, agent/role binding tables, budget parsing.

### Task 7 · `config explain`

Files: `src/explain.rs`

- [ ] **Failing test first.** `explain::prints_value_layer_and_file` — for a key set in two layers, the explanation names the winner and the shadowed one.
- [ ] Implement. The CLI surface is plan 17.

### Task 8 · `init` and `import`

Files: `src/import.rs`, `tests/import.rs`

- [ ] **Failing test first.** `import::claude_code_permissions` — a real-shaped Claude Code settings file maps to equivalent `deny`/`ask`/`allow` rules; a round-trip test over a fixture.
- [ ] `import::codex_mcp_servers` — a `config.toml` with `mcp_servers` maps to our MCP config.
- [ ] `import::is_explicit_and_one_way` — nothing reads a foreign config at runtime.
- [ ] Implement `init` (write a workspace config from a chosen profile) and `import`.

---

## Done when

- `cargo test -p orrery-config` green.
- An untrusted project's config and extensions demonstrably do not load.
- `orrery config explain <key>` names the layer and the file.
- Two profiles produce measurably different agents from one binary.
- A Claude Code settings file imports into working rules.

## Open questions

1. **Organisation layer fetch.** It is "signed, fetched from registry" — which makes it depend on plan 15. Phase 5 can ship with the layer present but only loadable from a local file, and the fetch arrives with the registry. Confirm that ordering.
2. **Trust prompt UX.** The decision is made before any client is fully attached (step 2, before discovery). What renders it? Probably a minimal bootstrap surface on the control channel. Needs a concrete answer before this ships.
3. **Where is `trust` stored?** User layer is the obvious place, but it is a security-relevant store that a hostile project must not edit. Confirm it lives outside any project-writable path.
4. **`permissions = { write = false }`** in a profile is a shorthand that does not obviously map to the rule grammar. Define the shorthands explicitly, or drop them in favour of real rules? Leaning: define a small, documented set, since §4.9's example uses one.
