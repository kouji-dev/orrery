# 15 · Registry and supply chain — pinned, signed, auditable

**Goal.** An organisation can say "these versions, these signatures, nothing else" and have the harness enforce it. A developer can install a community extension and see exactly what it is asking for before it runs. When this is done, an admin pins a version set and unpinned extensions refuse to load.

**Covers.** §8 phase 8 · §4.9's organisation layer · objective 7 (pinned, auditable supply chain) · the distribution step of §4.7.

**Crate.** `core/crates/orrery-registry`.

**Depends on.** [`06`](06-extension-host.md) (manifests and the ledger), [`07`](07-policy-broker-audit.md) (the grant diff), [`10`](10-config-layers.md) (the managed and org layers).

---

## Constraints

From [`00-overview.md`](00-overview.md):

- **The registry is a pin list, not a package host.** Extensions ship on crates.io and npm; the registry indexes names, versions and signatures.
- Managed config can say "refuse anything unpinned", and that is not user-overridable.
- Extensions are publishable crates — a community extension is the same shape as a first-party one.
- Every load outcome, including `Skipped { reason: Unsigned }`, lands in the ledger and the audit stream.

---

## Architecture

### The install command — decided with the user, 2026-09-18

**Spelling: `orrery install <source>`, a bare top-level verb, Pi-style.** `orrery remove <name>` likewise. The fuller `orrery ext list` / `ext test` forms stay for the things that are not everyday actions. Plan 17's command tree is amended to match.

**Default target: `~/.orrery/extensions/<id>/`** — the user layer, so an install is personal and available in every project, exactly as Pi behaves. The workspace layer, `<workspace>/.orrery/extensions/<id>/`, is for an extension the repo itself needs and should commit.

**Amended while implementing: the flag is `--to <user|workspace>`, not `--workspace`.** `--workspace <PATH>` is already a **global** flag on the `orrery` binary naming the workspace root, and clap will not let one long name mean two things. `--user` is accepted as the bare shorthand, as the plan asks. Discovery already scans `<layer-root>/extensions/` at every layer (plan 10, landed), so nothing new is needed on the read side.

**Sources, Pi-style.** One positional argument, disambiguated by prefix:

| Form | Resolves to |
|---|---|
| `orrery install buildgraph` | the signed **registry** index (the default, and the only verified path) |
| `orrery install buildgraph@1.2.0` | the same, pinned to a version |
| `orrery install github:owner/repo` · `…#tag` · `…#sha` | a git host; `sha` is the only form that is reproducible |
| `orrery install https://…/repo.git` | any git URL |
| `orrery install crates-io:orrery-ext-buildgraph` (`crate:` is the same thing) | crates.io |
| `orrery install npm:@scope/orrery-ext-x` | npm |
| `orrery install ./path` · `file:../path` · `/abs/path` | a local directory |
| `orrery install ./path --link` | symlink instead of copy — the development loop, never for real use |

`remove` finds the extension across layers and, if it is installed at more than one, names them and asks which rather than guessing.

### Authoring an index — added 2026-09-19, round 7

**The plan described a workflow whose first step did not exist.** Everything
below "an admin pins a version set" assumed a signed index had somehow been
produced: `orrery install` states that fetching a remote index is not
implemented, and no command in the binary could **create, sign or publish** a
local one. So phase 8 was reachable only by handing the admin a file made by a
tool that does not ship here. The tests proved the client; nothing proved the
workflow, because the workflow had a missing half.

Four verbs now close it, `orrery-cli/src/cmd/registry.rs` over
`orrery-registry/src/author.rs`:

```text
orrery registry init   --index <path> [--key <path>] [--key-id <id>] [--expires <instant>]
orrery registry add    --index <path> --id <id> --version <v> --source <crates-io:|npm:|url:>
orrery registry sign   --index <path> --key <path>
orrery registry verify --index <path> [--key <hex>]
```

- **Offline.** `add` stages the package out of the *same mirror directory the
  installer fetches from* (`cmd::install::mirror_dir`, one function) through the
  same `PackageFetcher::stage` + `tree_sha256` pair `fetch_and_verify` uses. A
  hash the install would never compute is not expressible.
- **`requires` is read out of the staged manifest**, not typed: the index's
  `requires` exists to be compared against that manifest, and an admin typing it
  by hand would be pinning their own transcription.
- **One rendering, one signature.** `author::sign_index` returns the bytes it
  signed, and `author::write_signed` writes those bytes plus the `.sig`.
  `author::signature_path` is the single answer to where the `.sig` lives, asked
  by the writer and by `cmd::install`'s reader.
- **The private key is a file, and says so.** `init` writes the seed with a
  header saying it is a secret, and prints the public half for `managed.toml`.
  There is no key store, no agent and no passphrase — stated here rather than
  implied away.

**What is still not shipped**, and no line in this plan should imply otherwise:
publishing. There is no index *host*, no `orrery registry publish`, and
`orrery install <bare name>` still resolves only a **local** index file —
`--index <path>`, or a `[registry] index` in the managed layer that names a
path. Distributing the index and the mirror is the organisation's own job
(a share, a checkout, an image layer). Fetching an index over the network
remains out of this build, for the reason `cmd/install.rs` gives: a `net` call
goes through the broker, which means a session.

### The security consequence, stated rather than discovered later

The registry exists because a pin list is what makes a supply chain auditable. **Five of the seven source forms above bypass it.** That is a deliberate ergonomic choice, and it is only safe because the boundary is explicit:

- **A registry install is verified**: index signature → pinned `sha256` → manifest `requires` mirrored against the index → grant diff. Nothing executes before the signature check.
- **Every other source is UNPINNED.** It still gets the grant diff and the sandbox, but there is no signature and no hash to check it against, so it is recorded `Loaded { pinned: false }` in the ledger and the audit stream — visibly, not quietly.
- **Managed `unpinned = "refuse"` blocks all five outright**, which is the single line an enterprise sets. `unpinned = "warn"` installs with a loud ledger entry. `unpinned = "allow"` is the developer default.
- `--link` is always unpinned and additionally marked `Development` in the ledger, because a symlinked extension can change under the harness between runs.

So the rule to write in the docs: **the registry is how you trust an extension; the other sources are how you try one.** A managed deployment turns the others off.

Also worth saying plainly in `extensions/README.md`: a git or local install pulls a **transitive dependency tree we do not review**. The wasm runtime closes that hole; `native`, `node`, `python` and `process` do not. Cross-reference open question 4.

### What the registry is

An index document, signed, fetched from a URL an organisation controls:

```toml
# index.toml, signed; the signature covers the whole document
schema  = 1
issued  = "2026-09-18T00:00:00Z"
expires = "2026-12-18T00:00:00Z"

[[extension]]
id      = "buildgraph"
version = "1.2.0"
source  = { kind = "crates-io", name = "orrery-ext-buildgraph" }
sha256  = "…"
sig     = "…"                      # signature over (id, version, source, sha256)
requires = ["read($WORKSPACE/**)", "spawn(java)"]   # mirrored from the manifest, so the
                                                    # index alone tells you what it wants
```

Two levels of trust, deliberately separate:

1. **The index is signed** by the organisation. One key, rotated on a schedule, distributed through the managed layer.
2. **Each entry pins a hash**, so what is fetched is what was reviewed — the index does not have to trust crates.io or npm to be immutable.

`requires` is mirrored into the index on purpose: an admin reviewing a version bump can see a capability change **without downloading the package**. A mismatch between the index's `requires` and the fetched manifest's is a hard failure, not a warning — that is a tampering signal.

### Verification order

```
resolve pin → fetch → verify sha256 → verify signature → parse manifest
  → compare manifest.requires vs index.requires → grant diff → load
```

Any failure short-circuits to a `LoadOutcome` and is audited. Nothing is executed before the signature check — in particular, **no build script and no install hook runs before verification**, which means fetching a crate is a download, not a `cargo install`.

### The grant diff

§4.7's install prompt, as a `Surface` so every client renders it:

```
 buildgraph 1.2.0 requests:
   read     $WORKSPACE/**        [allow] [allow once] [deny]
   spawn    java                 [allow] [allow once] [deny]
```

On an **upgrade**, the diff is against what was already approved:

```
 buildgraph 1.2.0 → 1.3.0 changes:
 + net      api.buildgraph.io    [allow] [deny]        ← NEW
   read     $WORKSPACE/**        (already allowed)
```

A new capability on an upgrade is the thing this whole plan exists to catch. It must be visually distinct and it must default to deny.

Denying does not fail the install (§4.7): the affected tools are disabled, everything else works, the ledger records `degraded`.

### Managed enforcement

```toml
# managed.toml — not user-overridable
[registry]
index    = "https://registry.corp.internal/orrery/index.toml"
key      = "…"
unpinned = "refuse"        # refuse | warn | allow
```

With `unpinned = "refuse"`, anything not in the index is `Skipped { reason: Unsigned }` at load. The developer sees why, in the ledger, with the index URL — not a silent absence.

---

## File structure

**Create**

- `harness/core/crates/orrery-registry/src/{lib,index,verify,fetch,pin,diff,error}.rs`
  — plus `author.rs`, added round 7: making an index, not only reading one.
- `harness/core/crates/orrery-registry/tests/{verify,pin,diff,tamper,author}.rs`
- `harness/core/crates/orrery-cli/src/cmd/registry.rs` and
  `harness/core/crates/orrery-cli/tests/registry.rs` — the four admin verbs, and
  the phase-8 workflow driven end to end through the built binary.
- `harness/core/crates/orrery-registry/tests/fixtures/` — a signed index, a tampered index, key pairs

---

## Tasks

### Task 1 · The index format

Files: `src/index.rs`

- [x] **Failing test first.** `index::parses_and_round_trips`; `index::unknown_schema_is_refused`; `index::expired_index_is_refused`.
- [x] Implement.

### Task 2 · Signatures

Files: `src/verify.rs`, `tests/verify.rs`

- [x] **Decision, recorded here and in `verify.rs`:** `ed25519-dalek` (simple, small, we manage the keys) vs. sigstore (no key management, but a large dependency and an online dependency at verify time). **ed25519 wins**, because an air-gapped enterprise is exactly the customer here and sigstore's transparency log assumes reachability — a verification step that needs the network is one that fails in the room where it matters most.
  **Amended while implementing: the ed25519 comes from `ring`, not `ed25519-dalek`.** `rustls` already puts `ring` in this workspace, so taking `ed25519-dalek` would mean two curve25519 implementations in one binary — two things to audit, two to patch on an advisory — for an identical algorithm and an identical wire format. The API surface used is four calls wide, so the implementation is swappable without a format change.
- [x] **Failing test first.** `verify::good_signature_passes`; `verify::tampered_index_fails`; `verify::wrong_key_fails`; `verify::rotated_key_with_overlap_works`.
- [x] Implement.

### Task 3 · Fetch and hash

Files: `src/fetch.rs`

- [x] **Failing test first.** `fetch::hash_mismatch_refuses` — a package whose bytes do not match the pinned `sha256` is refused, with the expected and actual hashes in the error.
- [x] `fetch::nothing_executes_before_verification` — the important one. Fetch a package containing a build script that would touch a sentinel file; assert the sentinel is untouched after a failed verification.
- [x] Implement over the broker's `net`.

### Task 4 · `requires` mirroring

Files: `src/verify.rs`, `tests/tamper.rs`

- [x] **Failing test first.** `tamper::manifest_requires_more_than_the_index` — a package whose manifest asks for `creds(*)` while the index says only `read`; assert a **hard failure**, audited as a tampering signal, not a prompt.
- [x] Implement.

### Task 5 · The grant diff surface

Files: `src/diff.rs`, `tests/diff.rs`

- [x] **Failing test first.** `diff::new_capability_on_upgrade_is_flagged` — 1.2.0 → 1.3.0 adding `net`; the diff marks it new and defaults to deny.
- [x] `diff::unchanged_capabilities_are_shown_as_already_allowed`.
- [x] `diff::renders_as_a_surface` — a `Surface` (a `form` or a `stack` of `question`s), so every client draws it.
- [x] `diff::deny_degrades_not_fails` — denying `spawn` disables the spawn-needing tool and leaves the rest working, ledger says `degraded`.
- [x] Implement.

### Task 6 · Pinning and managed enforcement

Files: `src/pin.rs`, `tests/pin.rs`

- [x] **Failing test first, and it is the phase-8 criterion.** `pin::unpinned_refuses_under_managed` — with `unpinned = "refuse"`, an extension absent from the index is `Skipped { Unsigned }`, the reason and the index URL are in the ledger, and **a user-layer setting cannot override it**.
- [x] `pin::warn_mode_loads_with_a_ledger_warning`.
- [x] `pin::version_set_is_exact` — a pinned 1.2.0 refuses 1.2.1.
- [x] Implement.

### Task 7 · `orrery install`

Files: `src/{lib,source,install}.rs` (the CLI surface is plan 17)

- [x] **Failing test first.** `install::end_to_end` — against a local fixture registry: resolve, fetch, verify, show the diff, approve, load, and see it in the ledger.
- [x] `install::offline_uses_the_cache` — a previously verified package installs without network.
- [x] Implement.

### Task 8 · Source resolution

Files: `src/source.rs`

**Decided while implementing: a bare name with *several* versions in the index is an error, not "the latest".** The plan's "a missing version is an error" and the Architecture table's bare `orrery install buildgraph` are both right, and they meet here: when the index holds exactly one version of an id, that is the pin; when it holds more, `orrery install x` says which it has and asks for one. Picking newest would make one command resolve differently on two machines either side of a publish, which is the thing a pin list exists to stop.

Every test here must run **offline**: a local bare git repo in a tempdir stands in for GitHub, and a local directory for crates.io/npm tarballs. No test fetches from a real host.

- [x] **Failing test first.** `source::parses_every_form` — a table over the seven forms in the Architecture table above, each producing the right `Source` variant. Ambiguity rules pinned: a bare word is the registry, a leading `./` or `../` is a path, everything else needs its prefix.
- [x] `source::bare_name_is_the_registry` — `orrery install buildgraph` never silently falls through to crates.io. If it is not in the index, that is an error naming the index URL, not a guess at another host.
- [x] `source::version_pin` — `name@1.2.0` resolves to exactly that version; a missing version is an error, not the latest.
- [x] `source::git_sha_is_reproducible_tag_is_not` — installing `github:o/r#v1.2` records the resolved commit sha in the ledger, so the install is auditable even though the ref is mutable.
- [x] Implement the resolvers. Git via the broker's `spawn` of the system git (the ADE already decided gitoxide has no network features — reuse that reasoning); crates.io and npm via the broker's `net`; local via the broker's `read`.

### Task 9 · Install target and layers

Files: `src/install.rs`

- [x] **Failing test first.** `install::defaults_to_the_user_layer` — `orrery install x` with no flag lands in `~/.orrery/extensions/x/` and is discovered from a different workspace.
- [x] `install::workspace_flag` — the workspace target lands in `<workspace>/.orrery/extensions/x/`. *(Spelled `--to workspace` on the command line; see the amendment in Architecture.)*
- [x] `install::link_is_a_symlink_and_marked_development` — `--link ./x` creates a link, and the ledger entry says `Development`, because the code can change under the harness between runs. *(On Windows a directory symlink needs Developer Mode or `SeCreateSymbolicLinkPrivilege`. The test probes for it and asserts the other half where it is missing: `--link` either links, or returns `SymlinkUnavailable` naming both. **Silently copying instead is the one wrong answer**, and neither branch allows it.)*
- [x] `remove::ambiguous_across_layers_asks` — installed at user *and* workspace, `orrery remove x` names both and refuses to guess.
- [x] Implement.

### Task 10 · Unpinned sources are visibly unpinned

Files: `src/install.rs`, `src/pin.rs`

This is the task that keeps the ergonomics honest.

- [x] **Failing test first.** `pin::non_registry_source_is_recorded_unpinned` — a non-registry install loads with `pinned: false` in the supply-chain ledger and the source named; a registry install records `pinned: true` with the rule that verified it. *(The unpinned half uses a local path, which needs no git on the machine; the git source is covered by `install::a_git_install_records_the_commit_it_resolved_to`, which asserts both `pinned: false` and the resolved commit sha.)*
- [x] `pin::managed_refuse_blocks_every_non_registry_source` — with `unpinned = "refuse"`, all five non-registry forms are refused, the message names the managed file, and **a user-layer setting cannot override it**.
- [x] `pin::warn_mode_installs_loudly`.
- [x] `pin::link_is_always_unpinned` — `--link` cannot be pinned even under `allow`.
- [x] Implement.

---

## Done when

- [x] `cargo test -p orrery-registry` green, including the tamper tests. *(42 tests
  across `index`, `verify`, `fetch`, `tamper`, `diff`, `source`, `pin` and
  `install`, every one of them offline.)*
- [x] **A refusal to load is logged by the run that refused it, added 2026-09-19.**
  It was visible in `ext list` and `ext test` and nowhere else: a fresh run under
  managed `unpinned = "refuse"` left `audit/*.jsonl` holding two `model.request`
  lines and not a word about the extension the session had just refused. Section 8
  phase 3 says every decision is logged, and a refusal is a decision — `assemble`
  now appends `AuditEvent::ExtensionLoad` with status `skipped` and the reason
  before it moves on (`orrery-cli/tests/ext.rs::a_refused_load_is_recorded_by_the_run_that_refused_it`,
  which reads the stream the run wrote). The refusal message also stopped naming
  an index it does not have: a managed layer may refuse everything unpinned and
  set no `index`, and the sentence interpolated the empty string and ended "may be
  loaded from  " (`pin::a_refusal_with_no_index_does_not_name_an_empty_one`).
  **Amended round 10: so is a refusal to INSTALL.** The same sentence was only
  half true. `orrery install ./buildgraph` under managed `unpinned = "refuse"`
  exited 4 with a good message and wrote nothing at all, so `ledger`,
  `ledger --stream load` and `ledger --subject ext:buildgraph` each answered
  "nothing recorded" about a decision the binary had just made. `cmd::install`
  now writes the same `ext.load` / `skipped` event into
  `<state-dir>/audit/install.jsonl`
  (`install::a_refused_install_is_in_the_ledger`).
- [x] An admin pins a version set; unpinned extensions refuse to load and say why.
  *(`pin::unpinned_refuses_under_managed`, plus `pin::version_set_is_exact` for
  the pin being a version rather than a range.)*
  **Amended round 7: an admin can now MAKE the version set.** Until this round
  the sentence assumed a signed index that no part of the product could produce:
  the client verified one, and nothing created, signed or published one.
  `orrery registry init|add|sign|verify` is the missing half, and
  `orrery-cli/tests/registry.rs::an_admin_can_produce_the_pin_set_they_are_meant_to_pin`
  drives the whole of it through the built binary — key, index, pin, signature,
  check-back, then a local path refused and the pinned version installed and
  recorded "pinned by key managed". What is **not** shipped is publishing: no
  index host, no network fetch, `--index <path>` or a managed `[registry] index`
  naming a path. See "Authoring an index" in Architecture.
  **Amended round 6: this was true of install and not of load.** An extension
  installed *before* an admin set `unpinned = "refuse"` went on loading
  afterwards, because `ManagedRegistry::read` was reached from
  `cmd/install.rs` and from nowhere else. The load path had nothing to go on —
  a directory and a manifest say nothing about whether a signature was ever
  checked — so an install now writes what pinning decided to
  `<layer>/registry/receipts/<id>.toml`, beside the extension rather than inside
  it, and `pin::load_refusal` answers the load-time question from it. A missing
  receipt refuses: the only way to have none is to predate the check or to have
  been placed by hand. The refusal is a `LoadOutcome::Skipped` the ledger and
  `orrery ext list` both show, and
  `orrery-cli/tests/ext.rs::an_unpinned_extension_refuses_to_load_once_the_pin_is_set`
  drives the whole sequence through the binary.
- [x] A capability added in an upgrade is surfaced distinctly and defaults to deny.
  *(`diff::new_capability_on_upgrade_is_flagged`, and end to end in
  `install::an_upgrade_that_adds_a_capability_denies_it_by_default`.)*
- [x] No package code executes before its signature is verified.
  *(`fetch::nothing_executes_before_verification`, with both halves asserted so
  the first is not vacuous.)*
- [x] All seven source forms install, offline, in tests; the five non-registry ones are recorded `pinned: false` and are refused under managed `unpinned = "refuse"`.
  **Amended round 10: one vocabulary, and it takes absolute paths.** There were
  two names for one concept — `registry add --source` took `crates-io:<name>`
  and `install` took `crate:<name>`, and neither answered to the other's word, so
  somebody who read one command's help and typed it at the other was told their
  source did not exist. Both parsers now take both spellings and print
  `orrery_registry::VOCABULARY` when they refuse, and a `Source` **displays** as
  `crates-io:` whichever was typed, so one string means one thing in the ledger
  and beside an index entry. `install` also accepted `./path` and refused the
  absolute path it resolves to, which is a distinction nothing downstream makes.
  `source::crate_and_crates_io_are_one_source`,
  `source::an_absolute_path_is_a_path`,
  `source::a_refusal_names_the_shared_vocabulary`,
  `install::one_vocabulary_for_a_crates_io_source`,
  `install::an_absolute_path_installs`.
- [x] **A remote index is not reported on, added round 10.** `orrery install
  <bare-name>` under a managed layer naming a remote index answered
  `buildgraph: not in the registry index https://registry.corp.internal/…`
  instantly and with no fetch, because fetching a remote index is not in this
  build. It stated a fact it had not checked and read as "I looked and it is not
  there". `RegistryError::RemoteIndex` says what is true of this build and names
  `--index <path>`, which works. `cmd::install::local_index` and the installer
  decide "is this remote" through one function, `orrery_registry::is_remote`.
  (`install::a_remote_index_says_it_cannot_be_fetched_rather_than_that_it_looked`.)
- [x] `orrery install x` with no flag lands in `~/.orrery/extensions/x/`.
  *(`install::defaults_to_the_user_layer`, which then finds it with
  `orrery_config::discover` from a **different** workspace.)*

## State

**Round 7, 2026-09-19: the authoring half landed.** `orrery registry
init|add|sign|verify` (`src/author.rs`, `orrery-cli/src/cmd/registry.rs`) makes
the signed index this crate could previously only read, offline, from the mirror
the installer stages from. Phase 8 is MET as a workflow rather than as a client.
Publishing is still not shipped, and the Architecture section says so in the one
place somebody would look.

**All ten tasks are done, and `orrery install` / `orrery remove` are in the
command tree (plan 17 task 7).** `cargo test -p orrery-registry` is 42 tests
green; `cargo clippy -p orrery-registry --all-targets -- -D warnings` is clean;
`cargo xtask deps-check` says ok.

Five things decided while implementing that are not in the tasks above:

- **The pin is over a canonical *tree* hash, not a tarball.** A package arrives
  as a directory — that is what a git clone, a local path and an unpacked crate
  all are — so `fetch::tree_sha256` hashes every file's relative path and bytes,
  sorted and length-prefixed, and skips `.git`. One hash for every runtime,
  which is open question 2's "lean duplicate" answered in the only way that
  keeps a single verification path.
- **The ordering is a type, not a comment.** `fetch_and_verify` is the only way
  to obtain a `Verified`, and everything downstream takes one, so "nothing
  executes before verification" is enforced by there being nothing to execute
  *from* until it has happened. The `PackageHooks` trait exists so the sentinel
  test can assert the hook does **not** fire on a failure and **does** on a
  success.
- **`LoadOutcome` has no `Skipped { reason: Unsigned }` in this build, and this
  crate did not add one.** `SkipReason` is a closed set in `orrery-proto`, with
  a generated-types tail behind it. So the supply-chain facts live in
  `pin::SupplyChainRecord` — source, `pinned`, the reason, the index URL, the
  rule that verified it — and `SupplyChainRecord::load_outcome` projects a
  refusal into `Skipped { PolicyDenied }`, which is the closest true statement
  the current set can make. **When `orrery-proto` next gains a variant, this is
  the one to add**, and `pin::unpinned_refuses_under_managed` is the test that
  should then assert it.
- **Test fixtures are built from committed seeds, not committed as bytes.** The
  plan's file list asks for a signed index and key pairs under
  `tests/fixtures/`. A committed signature cannot be regenerated when a
  canonical form changes, so the first field reorder would turn the whole suite
  red with nothing to say why. The seeds are committed; a seed is 32 bytes of
  nothing in particular, and there is no secret in this repository.
- **Fetching a *remote* index is not built.** Reading one is: `--index <path>`,
  or a managed `[registry] index` naming a file. Fetching over HTTPS is a `net`
  call, a `net` call goes through the broker, and the broker needs a session —
  which an install command does not have. Everything downstream of "here is the
  index text" is complete and tested, so this is one function and a decision
  about where an install gets a broker, not a redesign.

## Open questions

1. **Key rotation and distribution.** One key in managed config is simple but has no revocation story. Minimum viable: two keys with an overlap window and an `expires` on the index. Is that enough for a real security team? Ask one.
2. **npm packages.** Node extensions ship on npm, which has its own integrity story (`integrity` in the lockfile). Do we duplicate the hash pin, or lean on npm's? Duplicating is more uniform and more work. Lean duplicate — one verification path for every runtime.
3. **What about the ADE's existing extension registry?** `ade/src-tauri/src/extensions/registry.rs` already does sha256-verified zips from a release repo, with a `minOrreryVersion` gate. It is a different problem (tree-sitter grammars, not code with capabilities) but the fetch-and-verify code may be worth reading before writing this.
   **Partly answered by building it:** the keyring holds several keys, each with
   its own validity window, so rotation is an overlap rather than a cutover
   (`verify::rotated_key_with_overlap_works`), and the index carries `expires`.
   What is still open is revocation *inside* a window, and the managed schema
   this build reads carries one key without a window of its own — which is the
   next thing to widen, and the reason to ask a security team before doing so.

4. **Transitive dependencies.** We pin an extension; its crates.io dependencies are pinned by its lockfile, which we do not review. A malicious transitive dependency is still a hole — the wasm sandbox closes it, the `native` and `node` runtimes do not. Say this plainly in the threat model rather than implying the registry solves it.
