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

- [x] **Failing test first.** `merge::closest_layer_wins_for_values` — the same key at user and project; project wins, provenance says so.
- [x] `merge::deny_is_a_union` — denies from three layers all survive the merge.
- [x] `merge::managed_deny_cannot_be_relaxed` — a user `allow` for something managed denies; the deny stands and the attempt is logged.
- [x] `merge::nested_project_closest_wins` — `.orrery/` at two depths; the nearer one wins.
- [x] Implement over `toml_edit` with spans preserved.

### Task 2 · Trust gating

Files: `src/trust.rs`, `tests/trust.rs`

- [x] **Failing test first, and it is the security-relevant one.** `trust::untrusted_project_loads_no_project_config` — a workspace with a `.orrery/config.toml` declaring an extension; untrusted; assert the extension is **not** discovered, the session runs with user-level config, and the ledger says why.
- [x] `trust::project_cannot_grant_itself_trust` — a project config containing `trust = true` has no effect.
- [x] `trust::decision_is_stored_per_path`.
- [x] Implement, with the stored answers in the user layer.

### Task 3 · Startup order

Files: `src/lib.rs`

- [x] **Failing test first.** `startup::order_is_fixed` — instrument each step and assert the sequence 1→5 on every run, including when trust fails at step 2 (steps 3–5 run with the reduced layer set).
- [x] Implement `resolve`.

### Task 4 · Discovery

Files: `src/discover.rs`

- [x] **Failing test first.** `discover::one_pass` — extensions, skills, prompts and MCP servers are found in a single traversal; assert the filesystem is walked once (instrument the walker).
- [x] `discover::manifest_matches_the_ledger` — what discovery produces is what the load ledger reports. §4.11: "if it is in the model's visible set, it is in the manifest."
- [x] Implement.

### Task 5 · Validation with file and line

Files: `src/validate.rs`

- [x] **Failing test first.** `validate::unknown_param_names_the_file_and_line` — an agent binding with a typo'd parameter fails at load with `config.toml:14`, not a generic serde error.
- [x] `validate::missing_role_binding_fails_at_session_start`.
- [x] `validate::singleton_conflict_is_reported` — two `router` claims; the winner by precedence, the loser named.
- [x] Implement, using the retained spans.

### Task 6 · Profiles

Files: `src/profile.rs`, `tests/profile.rs`

- [x] **Failing test first, and it is the phase-5 criterion.** `profile::two_profiles_differ_measurably` — build a kernel from `review` and from `ci`; assert different visible tool sets, different models, and that `review` cannot write while `ci` never prompts.
- [x] `profile::consent_never_denies_instead_of_prompting`.
- [x] Implement `Profile`, agent/role binding tables, budget parsing.

### Task 7 · `config explain`

Files: `src/explain.rs`

- [x] **Failing test first.** `explain::prints_value_layer_and_file` — for a key set in two layers, the explanation names the winner and the shadowed one.
- [x] Implement. The CLI surface is plan 17, and it **landed**: `orrery config explain <key>` prints the winner and the shadowed values, with `--json`.
- [x] **Amended, round 5.** `explain` ignored the profile overlay: with `model`
  set under `[profile.review]`, `orrery config explain model --profile review`
  printed `model: not set in any layer`, and only the raw dotted path resolved.
  `explain_in` now reads `profile.<name>.<key>` first and falls through to the
  bare key, which stays in the answer as shadowed. `Explanation::resolved_from`
  names the key that was actually read, in the text and in the JSON —
  `explain::a_profile_overlay_wins_and_names_the_key_it_read`, and through the
  binary in `orrery-cli/tests/explain.rs`.

### Task 8 · `init` and `import`

Files: `src/import.rs`, `tests/import.rs`

- [x] **Failing test first.** `import::claude_code_permissions` — a real-shaped Claude Code settings file maps to equivalent `deny`/`ask`/`allow` rules; a round-trip test over a fixture.
- [x] `import::codex_mcp_servers` — a `config.toml` with `mcp_servers` maps to our MCP config.
- [x] `import::is_explicit_and_one_way` — nothing reads a foreign config at runtime.
- [x] Implement `init` (write a workspace config from a chosen profile) and `import`.

---

## Done when

- `cargo test -p orrery-config` green. **True** — 30 tests, 0 failures.
- An untrusted project's config and extensions demonstrably do not load. **True**
  — `trust::untrusted_project_loads_no_project_config`.
- ~~`orrery config explain <key>` names the layer and the file.~~ **Amended:**
  `orrery_config::explain` names the layer, the file, the line *and* the
  shadowed value, and `ResolvedConfig::explain` answers for a resolved session.
  ~~There is no `orrery config` **command**: the CLI surface is plan 17's and
  `orrery-cli` is still a stub, so nothing here can type that line at a shell.~~
  **Stale, corrected 2026-09-18: `orrery config explain <key>` exists**, prints
  the winning layer, the file, the line and what it shadowed, and takes
  `--json`. `orrery init` and `orrery import` are wired to task 8 as well.
  **Amended 2026-09-19: it answers for a table too.** `config explain
  permissions` printed "not set in any layer" under `--profile fast`
  (`read = true`) *and* under `--profile careful` (`read = false`), while
  `permissions explain` and the turn itself both enforced the shorthand — an
  explanation contradicting the rule set in force. `permissions` is never a leaf:
  a layer writes `permissions.deny` and a profile writes
  `profile.<name>.permissions.read`, and the explainer only looked for a leaf. A
  key that names a table now answers with its leaves, each contribution carrying
  the dotted key it was read from (`Contribution::from`, `"from"` in `--json`).
- ~~Two profiles produce measurably different agents from one binary.~~
  **Amended:** `review` and `ci` produce measurably different *assembled
  agents* — different model, different visible tool set, `review` denied a
  write that `ci` is allowed, `ci` refusing a spawn that `review` asks about —
  asserted in `profile::two_profiles_differ_measurably`. They are not fed to a
  kernel. **The caveat holds; its reason was wrong and is corrected here:**
  `orrery-kernel` is **plan 05**'s, not plan 03's, and it is not an empty stub —
  it landed in wave 3 with a working loop. Nothing here feeds a profile to it,
  because wiring config to the kernel is the CLI's job. **Half of that is now
  stale: `orrery-cli` is not a stub** — it runs turns, serves and attaches, and
  it resolves these layers for `config explain`, `permissions explain` and
  `init`. ~~What it does *not* yet do is build the kernel from a profile: it
  still assembles one from flags, so the provider is the fixture one.~~
  **Stale as of round 5, and this was the root cause of five `TODO(plan-10)`
  markers:** `orrery-harness` now depends on `orrery-config`, and
  `cmd::setup` folds the resolved layers into the `KernelConfig` the loop runs
  on — budget, retry policy, price table, model and output ceilings, each read
  through the profile overlay first. The consequence that made it worth doing is
  that `maxUsd` was **inert in every build** before it, and
  `orrery-cli/tests/budget.rs` now stops a real turn from a config file, through
  the binary, exit 3.

  **Round 7: `[permissions]` was inert for the same reason and is not any
  more.** The layers reached `KernelConfig`, but not the policy engine:
  `orrery_harness::ResolvedConfig::policy_toml` had no writer in the tree, so
  the kernel dispatched through the hardcoded `DEFAULT_RULES` while
  `permissions explain` answered from the layers — a permission system that
  reported a denial it did not enforce. Resolution now owns the fallback
  (`merge::DEFAULT_PERMISSIONS`, used only when **no** layer declares a rule),
  `ResolvedConfig::policy` and `rules_in_force` hand the compiled set out, and
  `cmd::layers::rules` gives that same set to the engine a turn dispatches
  through. Layer precedence is unchanged — still `PolicyBuilder`'s, deny a union
  and a managed deny final — which is exactly why the rules travel as
  `ResolvedRules` and not as a re-parsed TOML fragment that would collapse into
  one layer. `orrery-cli/tests/permissions_enforced.rs` drives the binary and
  asserts explain and run agree, per layer and per profile.

  `ProviderChoice` had only `Fixture` and `Custom`, so no configuration could
  name a real model. It has an `Anthropic` variant now, selected by
  `[provider] kind = "anthropic"`, with the feature that links it still off in
  CI and the transport injectable —
  `orrery-harness/tests/anthropic.rs` drives a turn through an in-process
  loopback server and a hand-written SSE fixture.
- A Claude Code settings file imports into working rules. **True** —
  `import::claude_code_permissions` round-trips a real-shaped fixture through a
  real `PolicyEngine` and checks the verdicts.

## State

**Landed 2026-09-18**, branch `feat/harness_claude-0917`, all eight tasks, in
task order, failing test first each time.

- `src/{error,layer,merge,provenance,trust,discover,validate,profile,explain,import}.rs`
  and `src/lib.rs`'s `resolve`.
- `tests/{merge,trust,profile,import}.rs` plus unit tests in `discover`,
  `validate`, `explain` and `lib` (the startup order). 30 tests, all green.
- Rules are **not** re-implemented here: every layer is handed to
  `orrery_policy::PolicyBuilder`, which is what makes deny a union and a
  managed deny final. `orrery-config` adds the value merge, the provenance, the
  trust gate, discovery, validation, profiles and the import.
- Task 3's `resolve` landed with task 2, because trust gating is not observable
  without it; the startup-order test is task 3's own commit.
- `Cargo.lock` deliberately left dirty — siblings are editing it.

## Open questions

1. **Organisation layer fetch.** It is "signed, fetched from registry" — which makes it depend on plan 15. Phase 5 can ship with the layer present but only loadable from a local file, and the fetch arrives with the registry. Confirm that ordering.

   **Decided: yes, that ordering.** `ConfigPaths::org` is an `Option<PathBuf>`
   pointing at a local file (`~/.orrery/org.toml` by default), and the layer is
   merged with `Layer::Org` precedence exactly as it will be once it is fetched.
   Nothing else about the layer changes when the fetch lands: plan 15 fills that
   field from a verified download instead of from disk, and **signature
   verification is deferred with it** — a local org file is trusted because the
   user or their IT put it there, the same as the managed one. Plan 15 does not
   exist yet, so shipping the fetch now would mean inventing a registry to fetch
   from.
2. **Trust prompt UX.** The decision is made before any client is fully attached (step 2, before discovery). What renders it? Probably a minimal bootstrap surface on the control channel. Needs a concrete answer before this ships.

   **Decided: a minimal bootstrap surface on the control channel, and `resolve`
   never prompts.** `resolve` is handed an answer or it is not
   (`StartupCtx::answer`); it cannot block on a human, because the fixed first
   thing a session does must not be able to hang. With no answer and nothing
   stored, the session starts **untrusted** and usable — user-level config, no
   project extensions — and the client may ask, store the answer with
   `TrustStore::record`, and resolve again. That also means a headless or CI
   session is correct by construction rather than deadlocked: no human, no
   prompt, no project code. The surface itself is a client concern; this crate's
   contract is the `answer` in and the `TrustDecision` (state, source, why) out.
3. **Where is `trust` stored?** User layer is the obvious place, but it is a security-relevant store that a hostile project must not edit. Confirm it lives outside any project-writable path.

   **Decided: a file of its own in the user directory, `~/.orrery/trust.toml`,
   not in `config.toml`.** Separate because `config.toml` is a file people
   share, template, commit to a dotfiles repo and edit with tooling, and none of
   that should be able to move a trust answer. `TrustStore::open` **refuses** —
   `ConfigError::TrustStoreInsideProject` — if the user directory resolves to
   the workspace root or anything under it, so the "outside any project-writable
   path" property is enforced rather than assumed, and
   `the_trust_store_never_lives_inside_the_workspace` holds it. Answers are
   keyed by canonical path, so a symlinked or differently-cased path cannot
   borrow another's answer.
4. **`permissions = { write = false }`** in a profile is a shorthand that does not obviously map to the rule grammar. Define the shorthands explicitly, or drop them in favour of real rules? Leaning: define a small, documented set, since §4.9's example uses one.

   **Decided: a small, closed, documented set — exactly five keys.**

   | Shorthand | `false` | `true` |
   |---|---|---|
   | `read`  | `deny = ["read(./**)"]`     | `allow = ["read(./**)"]` |
   | `write` | `deny = ["write(./**)"]`    | `allow = ["write(./**)"]` |
   | `net`   | `deny = ["net(domain: *)"]` | `allow = ["net(domain: *)"]` |
   | `spawn` | `deny = ["spawn(*)"]`       | `allow = ["spawn(*)"]` |
   | `creds` | `deny = ["creds(*)"]`       | `allow = ["creds(*)"]` |

   A sixth key is a **load error naming the file and the line**, not a silent
   no-op — the failure mode of an open-ended shorthand set is a permission
   somebody believes is in force. Each one expands to a real rule, so nothing
   here is a second grammar: a shorthand `true` is an ordinary `allow` and
   cannot carve an exception out of any deny, and `orrery init` writes the
   expansion rather than the shorthand, so the file a person reviews says what
   it does.
