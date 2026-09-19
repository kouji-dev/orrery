# 07 · Policy, broker, audit — the boundary extensions sit behind

**Goal.** Every decision about whether a call may run, and every handle that would let it. A rule grammar an operator can read at a glance, per-subject rule sets, capability tokens that cannot be forged or serialised, a broker that enforces budgets where the resource actually is, and one append-only audit stream that answers "which rule allowed this". When this is done, an extension denied `spawn` degrades instead of failing, and every decision is logged.

**Covers.** §4.8 in full · §4.12 (observability).

**Crates.** `core/crates/orrery-policy` · `core/crates/orrery-broker` · `core/crates/orrery-audit`.

**Depends on.** [`01`](01-proto-shared-types.md). Consumed by [`04`](04-tool-registry.md) and [`06`](06-extension-host.md), which stub it with allow-all until this lands.

---

## Constraints

From [`00-overview.md`](00-overview.md):

- `PolicyEngine::check` is a **sync, pure `fn`**. Not "fast enough" — non-async makes "policy performs no I/O" a compile-time property.
- `CapabilityToken` is private-field, not `Clone`, not `Serialize`, taken **by value**.
- Budgets are enforced where the resource is, not by the string that matched.
- Denial is a value.
- A `PermissionHandler` that panics fails **closed**.
- Paths normalise before they match — symlinks, `..`, case-folding, UNC, drive-relative **and Windows extended-length (`\\?\`) forms**.

This plan owns translation **#3** (`Subject`).

---

## Architecture

### The rule grammar

Borrow [Claude Code's ergonomics](https://code.claude.com/docs/en/permissions): `Tool` or `Tool(specifier)`, three lists — `deny`, `ask`, `allow` — evaluated in that order, **first match wins, specificity deliberately irrelevant**, and an allow can never carve an exception out of a deny. Add what a multi-principal system needs.

| Aspect | Example | Selector matches |
|---|---|---|
| `tool` | `tool(ripgrep.search)`, `tool(shell.exec: npm run *)` | namespaced id, then the tool's own specifier |
| `mcp` | `mcp(github.*)`, `mcp(github.get_*)` | server, then tool name |
| `skill` | `skill(review-*)` | skill name |
| `ext` | `ext(buildgraph)` | extension id, checked at load |
| `mode` | `mode(plan)`, `mode(execute)` | which mode the subject may enter |
| `read` `write` `spawn` `net` `creds` | `write(./src/**)`, `net(domain: *.corp.internal)` | broker resources — paths gitignore-style, domains, command text |
| `mem.read` `mem.write` | `mem.write(global)` | scoped by `MemScope`, not by path |

Operator set, small on purpose: `*` any text, `**` any depth in a path, trailing `prefix:*`, and `param:value` to match one named input. A `re:` escape hatch exists but is **off by default and warned about at load** — a permission rule nobody can read at a glance is a governance problem rather than a feature.

### Per subject

The part a single-principal harness has no equivalent for.

```toml
[permissions]                       # the agent itself
allow = ["tool(git.*)", "read(./**)"]
ask   = ["write(./**)"]
deny  = ["net(domain: *)"]

[permissions."ext:buildgraph"]      # one extension
allow = ["spawn(bazel *)", "read(./**)"]
deny  = ["creds(*)"]

[permissions."agent:critic"]        # one sub-agent
allow = ["tool(lsp.*)"]
```

A subject's effective set is its own rules **intersected with its parent's**: a rule file narrows a sub-agent, it never widens one. Across config layers, **deny is a union and the managed layer's deny cannot be relaxed**; allow and ask resolve by layer precedence.

### The engine

```rust
pub struct PolicyEngine { rules: ArcSwap<ResolvedRules>, minter: TokenMinter, audit: Arc<AuditSink> }

impl PolicyEngine {
    /// Sync. Pure. No I/O is possible from here.
    pub fn check(&self, call: &PendingCall, subject: &Subject, scope: &AgentScope) -> Decision;
    pub fn consent(&self, d: Decision, answer: ConsentAnswer) -> Decision;   // consumes d
    pub fn explain(&self, call: &PendingCall, subject: &Subject) -> Explanation;
}

#[non_exhaustive]
pub enum Decision {
    Allow { token: CapabilityToken, rule: RuleId },   // single-use, this call only
    Ask   { prompt: ConsentPrompt, rule: RuleId, fallback: Box<Decision> },
    Deny  { rule: RuleId, reason: String },
}
```

`ArcSwap` so a config reload does not block every `check`.

**Vocabulary, because both words appear throughout: `ask` is the verdict, consent is the interaction.** A rule in the `ask` list produces `Decision::Ask`, which the kernel turns into a `consent.request` frame carrying a deadline; the answer comes back as `consent.answer`; a profile with `consent = "never"` resolves every `ask` to its fallback without prompting.

### PermissionHandler — narrowing only

```rust
#[async_trait]
pub trait PermissionHandler: Send + Sync {
    async fn review(&self, call: &PendingCall, subject: &Subject, proposed: Decision)
        -> Result<Decision, HandlerError>;
}
```

It may narrow — `Allow → Ask | Deny`, `Ask → Deny` — never widen. A wider verdict is **dropped and logged**. It never reaches the managed layer. A panic fails **closed**: the call is denied.

Enforcement is mechanical: `review` returns a `Decision`, and the engine runs `narrowed = min(proposed, returned)` under an explicit ordering with `Deny` lowest. A proptest asserts the result is never greater than `proposed`.

### CapabilityToken

```rust
pub struct CapabilityToken(Inner);        // private; no Clone/Copy/Default/Serialize/From
struct Inner { call: CallId, aspect: Aspect, scope: ResolvedScope,
               deadline: Instant, nonce: u64, rule: RuleId }

pub struct TokenMinter { ledger: Arc<TokenLedger> }   // constructible only at boot
impl TokenMinter { pub(crate) fn mint(&self, ..) -> CapabilityToken; }

pub struct TokenLedger { live: DashSet<u64> }
impl TokenLedger {
    pub fn redeem(&self, nonce: u64) -> Result<(), BrokerError>;   // check-and-remove
    pub fn revoke_call(&self, call: CallId);                       // cancellation
}
```

Three properties, each testable:

1. **Unforgeable** — no public constructor; a `trybuild` compile-fail test proves an external crate cannot make one.
2. **Single-use** — the broker takes it by value, so the move consumes it; the nonce redemption catches any clone attempt through unsafe.
3. **Revocable** — cancelling a turn drops its nonces, so an in-flight tool's next broker call fails `Revoked`.

And the load-bearing one: **not `Serialize`**, so §4.8's "no extension ever holds a handle" is a type error rather than a code review.

### The broker

```rust
#[async_trait]
pub trait Broker: Send + Sync {
    async fn read(&self, t: CapabilityToken, path: &Path, budget: &ToolBudget)
        -> Result<LimitedReader, BrokerError>;
    async fn write(&self, t: CapabilityToken, path: &Path, atomic: bool)
        -> Result<WriteHandle, BrokerError>;
    async fn spawn(&self, t: CapabilityToken, cmd: SpawnSpec, budget: &ToolBudget)
        -> Result<Child, BrokerError>;
    async fn net(&self, t: CapabilityToken, req: NetRequest, budget: &ToolBudget)
        -> Result<NetResponse, BrokerError>;
    /// Resolves a NAME at the point of use and never returns the value to the caller.
    async fn creds(&self, t: CapabilityToken, name: &str, use_it: CredUse)
        -> Result<(), BrokerError>;
}
```

`creds` is the shape that matters: a provider hands the broker a request to *use* a credential (sign this header), not a request to *read* one. Rotation rewrites the name and nothing holding a reference changes.

**Say plainly which layer enforces.** Claude Code's docs are candid that a `Bash(curl *)` deny stops `curl https://x` but not `/usr/bin/curl https://x` or `sh -c 'curl https://x'`. Patterns describe intent; they do not enforce. So: **rules decide whether to ask; the broker and the token decide what can be touched.** A `spawn` grant is enforced where the process is created, not by the string that matched. Put this paragraph in the crate docs.

### Audit

One append-only structured stream: extension loads, capability decisions **with the rule that produced them**, tool calls with input hashes, model requests with token counts, consent answers, sub-agent spawns, routing decisions with their signal values.

**Redaction is in the schema, not the deployment.** Tool inputs are hashed (`blake3`). Credential values never appear — the broker resolves references at the point of use, so there is nothing to log. Memory entries, surface payloads and prompt bodies are recorded by reference (id, scope, length, hash), with content retrievable only from the session store.

Three streams as three `tracing` layers (§4.12): load ledger, audit, telemetry. File sink always; OTLP behind a feature.

---

## File structure

**Create**

- `harness/core/crates/orrery-audit/src/{lib,event,sink,layer,redact}.rs`
- `harness/core/crates/orrery-policy/src/{lib,rule,parse,match,engine,token,handler,explain,error,call}.rs`
- `harness/core/crates/orrery-policy/tests/{parse,match_props,layers,narrow,token,explain}.rs`
- `harness/core/crates/orrery-broker/src/{lib,fs,proc,net,creds,limit,contain,error,gate}.rs`
- `harness/core/crates/orrery-broker/tests/{limits,contain,atomic,creds,degrade}.rs`

---

## Tasks

### Task 1 · Audit first

Files: `orrery-audit/src/*`

Everything else writes to it, so it comes first.

- [x] **Failing test first.** `audit::inputs_are_hashed` — record a tool call with a secret in its input; assert the raw value appears nowhere in the sink and the hash is stable.
- [x] `audit::append_only` — no API mutates or deletes.
- [x] Implement the event enum, the file sink (JSONL), the three tracing layers, `redact`.
- [x] **(2026-09-19)** `orrery-tools`'s dispatch actually *emits* `AuditEvent::ToolCall`. It had carried a `TODO(plan-07)` reading "until that crate exists as a dependency, tracing carries it" long after `orrery-audit` became a dependency of `orrery-tools` — a stale blocker, which is a bug and not a note. `dispatch::a_settled_call_reaches_the_audit_stream` and `a_denied_call_is_audited_as_denied` pin both halves: every exit is audited, including the six early refusals, and the input is hashed before an interceptor can rewrite it.

### Task 2 · Rule parsing

Files: `orrery-policy/src/{rule,parse}.rs`

- [x] **Failing test first.** `parse::every_aspect_form` — a table over the grammar table above; each string parses to the expected `Rule`.
- [x] `parse::re_escape_hatch_warns` — `re:` parses but emits a load warning and is rejected when disabled.
- [x] `parse::bad_rule_names_the_file_and_line`.
- [x] Implement.

### Task 3 · The matcher

Files: `orrery-policy/src/match.rs`, `tests/match_props.rs`

- [x] **Failing test first.** `match::deny_beats_allow` — `deny(write(./**))` plus `allow(write(./src/**))` denies, because allow never carves an exception out of deny.
- [x] `match::first_match_wins_within_a_list`.
- [x] `match::paths_normalise` — a symlink out of the workspace, a `..` traversal, a UNC path and a drive-relative path all resolve before matching. This is the test that stops the rule being defeated by a link.
- [x] `match::case_folding_where_the_fs_is_insensitive` — Windows and macOS only.
- [x] `long_paths::a_file_past_max_path_is_still_inside_the_workspace` — **added 2026-09-19.** A target past `MAX_PATH` canonicalises to the `\\?\` form and `dunce::simplified` will not reduce it *because* it is long, which is right for a path about to be opened and wrong for a key about to be matched: the rules had one shape and the target another, so nothing matched and a filesystem limit came back out of the engine as "no rule allows `read(...)`". `lexical` now reduces the verbatim prefix itself. A length is not a permission decision.
- [x] Proptest: `match::never_panics` on arbitrary selectors and inputs.
- [x] Implement with `globset` and `dunce`.

### Task 4 · Layers and subjects

Files: `orrery-policy/src/engine.rs`

- [x] **Failing test first.** `layer::managed_deny_is_final` — a user allow cannot relax a managed deny.
- [x] `layer::deny_is_a_union` — denies from three layers all apply.
- [x] `subject::child_is_intersected` — a sub-agent's allow list is narrowed by its parent's, never widened.
- [x] `subject::string_forms` — cross-check plan 01's `Subject` serialisation round-trips from TOML keys (translation #3).
- [x] Implement layer resolution, `ArcSwap` hot-swap.

### Task 5 · Tokens

Files: `orrery-policy/src/token.rs`, `tests/token.rs`

- [x] **Failing test first (compile-fail).** `token::cannot_be_constructed_externally` — proving an out-of-crate `CapabilityToken(..)` does not compile.
- [x] `token::is_not_serializable` — a compile-fail test on the `Serialize` bound.

  **Built with `compile_fail` doctests, not `trybuild`.** A doctest is compiled
  as a separate crate against the real rlib, which is exactly the out-of-crate
  vantage point the property is about, and it needs no dependency this
  environment can fetch. Both live on `CapabilityToken` in `token.rs`, with a
  third, *passing* doctest naming the type, so neither failure can be a typo
  passing for a proof.
- [x] `token::single_use` — redeeming the same nonce twice fails.
- [x] `token::revoked_on_cancel` — revoke the call, then redeem; fails `Revoked`.
- [x] `token::expires` — past the deadline, redemption fails even with a live nonce.
- [x] Implement `CapabilityToken`, `TokenMinter`, `TokenLedger`.

### Task 6 · PermissionHandler

Files: `orrery-policy/src/handler.rs`, `tests/narrow.rs`

- [x] **Failing test first.** `narrow::widening_is_dropped` (proptest) — for arbitrary proposed and returned decisions, the result is never more permissive than proposed, and a widening attempt is logged.
- [x] `narrow::panic_fails_closed` — a handler that panics ⇒ the call is denied, the session survives.
- [x] `narrow::never_reaches_managed` — a handler cannot affect a managed-layer decision.
- [x] Implement with `catch_unwind` at the boundary.

### Task 7 · Broker — filesystem and limits

Files: `orrery-broker/src/{fs,limit}.rs`, `tests/{limits,atomic}.rs`

- [x] **Failing test first.** `limits::read_is_bounded_while_reading` — a 100 MB file, a 4 KB ceiling; assert via an instrumented reader that total bytes pulled never exceeded the ceiling plus one buffer. Not "the result was truncated" — **peak** matters.
- [x] `atomic::write_reverts_on_cancel` — cancel mid-write; original intact, no temp file left.
- [x] `limits::no_token_no_call` — every broker method rejects without a valid token.
- [x] Implement `LimitedReader`, `take_bytes`, `WriteHandle` with temp-then-rename.

### Task 8 · Broker — processes

Files: `orrery-broker/src/{proc,contain}.rs`, `tests/contain.rs`

- [x] **Failing test first.** `contain::grandchildren_die` — spawn a child that spawns a grandchild; kill the call; assert both are gone. Windows via Job Object (port `ade/src-tauri/src/runtime/jobobj.rs`), unix via setsid + process-group kill.
- [x] `contain::wall_clock_watchdog` — a child that ignores SIGTERM is SIGKILLed after the grace window.
- [x] `contain::memory_ceiling` — Windows and Linux assert enforcement; **macOS asserts best-effort sampling and the test is marked as such.**
- [x] `contain::stdout_backpressure` — a child writing faster than we read gets EPIPE rather than growing our heap.
- [x] Implement.

### Task 9 · Credentials

Files: `orrery-broker/src/creds.rs`

- [x] **Failing test first.** `creds::value_never_returned` — the API has no method returning a secret; a doc test shows the `use_it` shape.
- [x] `creds::rotation_is_transparent` — rewrite the stored value; a held reference keeps working.
- [x] **(2026-09-19) The extension-facing half.** `orrery_ext_api::creds` is one `CredStore` for every provider extension, where there had been a copy per vendor, and `BrokerCredStore` goes through the `creds` grant: policy-checked, in the ledger, refused with a rule id. The facade gained `store_credential`, `forget_credential` and `has_credential`, each with a default body — a non-breaking addition under `orrery-ext/1`. `MockBroker` implements them, so an extension author tests a login against the same ledger a session shows.

> **Where this is still short, stated plainly (2026-09-19).** A provider can
> *store* a credential through the grant and *ask whether one exists* through
> the grant. It cannot have the broker **use** one, because using it means the
> broker owns the outgoing request and the broker installs no HTTP transport yet
> — `Broker::fetch` answers "no transport is installed", and that is plan 14's.
> So a provider still holds its own key, and reads it through
> `LayeredCredStore`: the grant first, then `EnvCredStore`. That fallback is
> **documented, not a TODO** — `EnvCredStore`'s doc comment says what it is for
> (a contributor's exported key before any session exists; a downstream
> embedder's CI) and that it refuses to write, because a login that wrote to the
> process environment would be lost at exit. A *denial* from the grant never
> falls through to it: reaching around a refusal via an environment variable is
> exactly the hole the grant exists to close.
>
> **What closes this:** plan 14's transport, after which `fetch` signs the
> request through `CredUse::Header` and the fallback is deleted. Nothing else in
> a provider changes, which is why the seam is a type rather than an `if` in
> each of them.
- [x] Implement: OS keychain where available, `0600` file otherwise.

### Task 10 · `permissions explain`

Files: `orrery-policy/src/explain.rs`

- [x] **Failing test first.** `explain::names_rule_layer_and_file` — dry-run a call; the explanation carries the rule id, the layer, the source file and line, and the verdict.
- [x] Implement. The CLI surface is plan 17.

### Task 11 · Degrade, end to end

Files: `orrery-broker/tests/`

- [x] **Failing test first, and it is the phase-3 criterion.** `degrade::denied_spawn_degrades` — install an extension requesting `spawn`; deny it; assert the install succeeds, the spawn-needing tool is disabled, its other tools work, the ledger says `degraded`, and the audit holds the decision with its rule.

---

## Done when

- `cargo test -p orrery-audit -p orrery-policy -p orrery-broker` green, including every compile-fail test.
- An extension denied `spawn` degrades rather than failing.
- Every decision appears in the audit with the rule that produced it.
- **The rules a turn is checked against are the rules a person wrote.**
  Amended in place 2026-09-19: the engine was always correct and the *binary*
  never gave it the layers. `orrery_harness::ResolvedConfig` carried a
  `policy_toml` field with no writer anywhere in the tree, so `assemble` fell
  back to the hardcoded `DEFAULT_RULES` in every build and `[permissions]` was
  inert — while `permissions explain`, reading the layers directly, reported
  denials nothing enforced. The field is now the *resolved* rule set
  (`Option<Arc<ResolvedRules>>`), `orrery-cli`'s `layers::rules` is the single
  place it is chosen, and both halves read that one set.
  `orrery-cli/tests/permissions_enforced.rs` asserts the equivalence — explain
  says deny ⇒ the run is denied, explain says allow ⇒ the run performs it —
  across the user, workspace and managed layers and across profiles.
- No credential value appears anywhere in the audit stream.

## Open questions

1. **Consent prompt ownership.** The prompt is minted by the policy engine but rendered in the client's own chrome (§6.2 distinguishes it from a `question` surface). Confirm the `ConsentPrompt` type carries enough for a client to render it without inventing copy.

   **Confirmed, with one addition on our side.** Plan 01's `ConsentPrompt`
   carries the prompt id, the subject, the capabilities asked for, a `reason` in
   words, the rule id and an optional `Surface` — enough to render without
   inventing copy. What it does *not* carry is the **call id**, because a prompt
   is a thing shown to a person. So `Decision::Ask` carries `call`, `aspect` and
   the resolved `scope` alongside the prompt, and `PolicyEngine::consent` mints
   from those. The prompt shape is unchanged; the extra fields never leave the
   engine.
2. **`re:` default.** Off with a warning is specified. Should a managed layer be able to forbid it outright? Probably yes, as `ext`-style load-time policy. Cheap to add now.

   **Decided: yes.** `PolicyBuilder::forbid_regex()` rejects any layer carrying a
   `re:` rule, and `build` rejects one that slipped in. Off by default is a load
   **error**, not a silent ignore — a rule that would not apply is worse than one
   that does not parse. On deliberately, it parses and raises a `Warning` naming
   the file and line. Covered by `parse::re_escape_hatch_warns` and
   `parse::a_managed_layer_can_forbid_the_escape_hatch`.
3. **Audit sink rotation.** JSONL grows forever. Size-based rotation with a retention count is the obvious answer; confirm before a long-running session fills a disk.

   **Decided: size-based rotation with a retention count**, as
   `orrery_audit::Rotation` — 16 MiB and four rolled files by default. A
   time-based scheme needs a policy about idle days that nobody wants to reason
   about. The live handle is dropped before the rename because Windows will not
   rename an open file, and an append that cannot be written increments
   `FileSink::errors` rather than killing a turn.
4. **macOS memory ceilings.** Best-effort sampling is a real gap. Document it prominently, or refuse `memory_bytes` grants on macOS rather than pretending? Leaning toward documenting and reporting `Unenforced` in the ledger.

   **Decided: document and report `Unenforced`.** `contain::MemoryEnforcement` is
   `NotRequested | Enforced | Unenforced`, returned by `Child::memory_enforcement`
   and carried on `Output`. Refusing the grant would make a portable manifest
   unportable for a platform difference the manifest did not cause; pretending is
   worse. `contain::memory_ceiling` asserts `Enforced` on Windows and Linux and
   **asserts `Unenforced` on macOS**, so the test says what is true rather than
   what we wish were true.

## State

**Landed**, `feat/harness_claude-0917`. `cargo test -p orrery-audit -p orrery-policy
-p orrery-broker` is green, including the three `compile_fail` doctests, and
clippy is clean across all targets.

- **`orrery-audit`** — `AuditEvent` with redaction in the schema (`Digest`,
  `ContentRef`), `AuditSink` with one method, `MemorySink`/`FileSink`/`NullSink`,
  size-based rotation, and the three `tracing` layers routed by target prefix
  (`orrery.load`, `orrery.audit`, `orrery.telemetry`).
  **Added 2026-09-19: `read`, the other half.** "Every decision is logged" was
  true of the *writer* and vacuous in the product: nothing could open the stream
  and ask it anything, and the CLI was passing `orrery_audit::null()`, so a real
  run logged into a bin. `orrery_audit::scan` now reads the JSONL back under a
  `Query` — stream, subject, rule (by id **or** by the text it was written as),
  and a limit counted from the end, because an audit is read from the end. A
  line that will not parse is *counted* (`Scan::skipped`) rather than crashing
  the scan or vanishing: a process killed mid-write leaves half a line, and an
  operator surface has to be able to say "487 records, 1 unreadable line". It is
  a scan and a filter, never an index: a second representation of the evidence
  is a second thing that can disagree with it. `orrery ledger` and
  `orrery telemetry` (plan 17 task 8) are the surface over it, one file per
  session under `<state-dir>/audit/`.
- **`orrery-policy`** — the grammar over all fourteen aspects with `*`, `**`,
  `prefix:`, `param:key=glob` and the `re:` escape hatch; `globset` + `dunce`
  path normalisation that resolves symlinks, `..`, UNC and drive-relative forms
  and folds case where the filesystem does; deny-before-ask-before-allow across
  every layer; subjects intersected with their parent (and **inheriting** when no
  rules were written about them at all); `CapabilityToken` with no public
  constructor, no `Serialize`, single-use nonces and per-call revocation;
  `review_narrowing` with `catch_unwind`; `explain`.
- **`orrery-broker`** — `LimitedReader` bounded *while* reading, temp-then-rename
  `WriteHandle`, per-call Windows job object / unix `setsid` containment with a
  wall-clock watchdog and stdout backpressure, `CredStore` with no `get`, and
  `gate.rs`: the real `PolicyCheck` for plan 04's dispatch plus `install`, which
  is what makes a denied `spawn` degrade.

**Also done here, outside this plan:** plan 04 task 5 — `orrery-audit` is now in
the workspace dependency table and `Registry::with_audit` sends every ledger
decision to it (`resolve::ambiguity_reaches_the_audit`).

**Deviations.**

- `trybuild` is not used; the two compile-fail proofs are `compile_fail`
  doctests. This environment has no network and `trybuild` is not in the registry
  cache, and a doctest proves the same property from the same vantage point.
- `TokenError` lives in `orrery-policy`, not `orrery-broker`: the ledger does the
  checking and the broker depends on policy, not the other way round.
  `BrokerError::Token` wraps it, so the plan's
  `redeem(..) -> Result<(), BrokerError>` shape is what a broker caller sees.
- `Decision::Ask` carries `call`, `aspect` and `scope` beside the prompt (see
  open question 1), and `prompt` is boxed because a prompt can hold a whole
  surface and a `Decision` is returned from every check.
- A `PermissionHandler` is shown a decision whose token was minted against an
  **inert ledger** — `CapabilityToken` is not `Clone`, and handing a reviewer the
  live capability it is judging would be the opposite of the point.
- `Broker::net` has no transport by default and answers `NoTransport`. Nothing in
  this repository dials anything.
- `read_is_bounded_while_reading` uses an instrumented 100 MB *source* rather
  than a 100 MB file on disk: the assertion is about peak bytes pulled, which
  only the source can witness.
- A subject nobody wrote rules about **inherits** its parent's set rather than
  getting nothing. "A rule file narrows a sub-agent" is about what a file does,
  not about the absence of one.
