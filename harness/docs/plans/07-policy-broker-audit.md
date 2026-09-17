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
- Paths normalise before they match — symlinks, `..`, case-folding, UNC and drive-relative forms.

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
- `harness/core/crates/orrery-policy/src/{lib,rule,parse,match,engine,token,handler,explain,error}.rs`
- `harness/core/crates/orrery-policy/tests/{match_props,narrow,token}.rs`
- `harness/core/crates/orrery-broker/src/{lib,fs,proc,net,creds,limit,contain,error}.rs`
- `harness/core/crates/orrery-broker/tests/{limits,contain,atomic}.rs`

---

## Tasks

### Task 1 · Audit first

Files: `orrery-audit/src/*`

Everything else writes to it, so it comes first.

- [ ] **Failing test first.** `audit::inputs_are_hashed` — record a tool call with a secret in its input; assert the raw value appears nowhere in the sink and the hash is stable.
- [ ] `audit::append_only` — no API mutates or deletes.
- [ ] Implement the event enum, the file sink (JSONL), the three tracing layers, `redact`.

### Task 2 · Rule parsing

Files: `orrery-policy/src/{rule,parse}.rs`

- [ ] **Failing test first.** `parse::every_aspect_form` — a table over the grammar table above; each string parses to the expected `Rule`.
- [ ] `parse::re_escape_hatch_warns` — `re:` parses but emits a load warning and is rejected when disabled.
- [ ] `parse::bad_rule_names_the_file_and_line`.
- [ ] Implement.

### Task 3 · The matcher

Files: `orrery-policy/src/match.rs`, `tests/match_props.rs`

- [ ] **Failing test first.** `match::deny_beats_allow` — `deny(write(./**))` plus `allow(write(./src/**))` denies, because allow never carves an exception out of deny.
- [ ] `match::first_match_wins_within_a_list`.
- [ ] `match::paths_normalise` — a symlink out of the workspace, a `..` traversal, a UNC path and a drive-relative path all resolve before matching. This is the test that stops the rule being defeated by a link.
- [ ] `match::case_folding_where_the_fs_is_insensitive` — Windows and macOS only.
- [ ] Proptest: `match::never_panics` on arbitrary selectors and inputs.
- [ ] Implement with `globset` and `dunce`.

### Task 4 · Layers and subjects

Files: `orrery-policy/src/engine.rs`

- [ ] **Failing test first.** `layer::managed_deny_is_final` — a user allow cannot relax a managed deny.
- [ ] `layer::deny_is_a_union` — denies from three layers all apply.
- [ ] `subject::child_is_intersected` — a sub-agent's allow list is narrowed by its parent's, never widened.
- [ ] `subject::string_forms` — cross-check plan 01's `Subject` serialisation round-trips from TOML keys (translation #3).
- [ ] Implement layer resolution, `ArcSwap` hot-swap.

### Task 5 · Tokens

Files: `orrery-policy/src/token.rs`, `tests/token.rs`

- [ ] **Failing test first (compile-fail).** `token::cannot_be_constructed_externally` — `trybuild` proving an out-of-crate `CapabilityToken(..)` does not compile.
- [ ] `token::is_not_serializable` — a compile-fail test on `serde_json::to_string(&token)`.
- [ ] `token::single_use` — redeeming the same nonce twice fails.
- [ ] `token::revoked_on_cancel` — revoke the call, then redeem; fails `Revoked`.
- [ ] `token::expires` — past the deadline, redemption fails even with a live nonce.
- [ ] Implement `CapabilityToken`, `TokenMinter`, `TokenLedger`.

### Task 6 · PermissionHandler

Files: `orrery-policy/src/handler.rs`, `tests/narrow.rs`

- [ ] **Failing test first.** `narrow::widening_is_dropped` (proptest) — for arbitrary proposed and returned decisions, the result is never more permissive than proposed, and a widening attempt is logged.
- [ ] `narrow::panic_fails_closed` — a handler that panics ⇒ the call is denied, the session survives.
- [ ] `narrow::never_reaches_managed` — a handler cannot affect a managed-layer decision.
- [ ] Implement with `catch_unwind` at the boundary.

### Task 7 · Broker — filesystem and limits

Files: `orrery-broker/src/{fs,limit}.rs`, `tests/{limits,atomic}.rs`

- [ ] **Failing test first.** `limits::read_is_bounded_while_reading` — a 100 MB file, a 4 KB ceiling; assert via an instrumented reader that total bytes pulled never exceeded the ceiling plus one buffer. Not "the result was truncated" — **peak** matters.
- [ ] `atomic::write_reverts_on_cancel` — cancel mid-write; original intact, no temp file left.
- [ ] `limits::no_token_no_call` — every broker method rejects without a valid token.
- [ ] Implement `LimitedReader`, `take_bytes`, `WriteHandle` with temp-then-rename.

### Task 8 · Broker — processes

Files: `orrery-broker/src/{proc,contain}.rs`, `tests/contain.rs`

- [ ] **Failing test first.** `contain::grandchildren_die` — spawn a child that spawns a grandchild; kill the call; assert both are gone. Windows via Job Object (port `ade/src-tauri/src/runtime/jobobj.rs`), unix via setsid + process-group kill.
- [ ] `contain::wall_clock_watchdog` — a child that ignores SIGTERM is SIGKILLed after the grace window.
- [ ] `contain::memory_ceiling` — Windows and Linux assert enforcement; **macOS asserts best-effort sampling and the test is marked as such.**
- [ ] `contain::stdout_backpressure` — a child writing faster than we read gets EPIPE rather than growing our heap.
- [ ] Implement.

### Task 9 · Credentials

Files: `orrery-broker/src/creds.rs`

- [ ] **Failing test first.** `creds::value_never_returned` — the API has no method returning a secret; a doc test shows the `use_it` shape.
- [ ] `creds::rotation_is_transparent` — rewrite the stored value; a held reference keeps working.
- [ ] Implement: OS keychain where available, `0600` file otherwise.

### Task 10 · `permissions explain`

Files: `orrery-policy/src/explain.rs`

- [ ] **Failing test first.** `explain::names_rule_layer_and_file` — dry-run a call; the explanation carries the rule id, the layer, the source file and line, and the verdict.
- [ ] Implement. The CLI surface is plan 17.

### Task 11 · Degrade, end to end

Files: `orrery-broker/tests/`

- [ ] **Failing test first, and it is the phase-3 criterion.** `degrade::denied_spawn_degrades` — install an extension requesting `spawn`; deny it; assert the install succeeds, the spawn-needing tool is disabled, its other tools work, the ledger says `degraded`, and the audit holds the decision with its rule.

---

## Done when

- `cargo test -p orrery-audit -p orrery-policy -p orrery-broker` green, including every compile-fail test.
- An extension denied `spawn` degrades rather than failing.
- Every decision appears in the audit with the rule that produced it.
- No credential value appears anywhere in the audit stream.

## Open questions

1. **Consent prompt ownership.** The prompt is minted by the policy engine but rendered in the client's own chrome (§6.2 distinguishes it from a `question` surface). Confirm the `ConsentPrompt` type carries enough for a client to render it without inventing copy.
2. **`re:` default.** Off with a warning is specified. Should a managed layer be able to forbid it outright? Probably yes, as `ext`-style load-time policy. Cheap to add now.
3. **Audit sink rotation.** JSONL grows forever. Size-based rotation with a retention count is the obvious answer; confirm before a long-running session fills a disk.
4. **macOS memory ceilings.** Best-effort sampling is a real gap. Document it prominently, or refuse `memory_bytes` grants on macOS rather than pretending? Leaning toward documenting and reporting `Unenforced` in the ledger.
