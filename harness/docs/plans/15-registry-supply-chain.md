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

**Default target: `~/.orrery/extensions/<id>/`** — the user layer, so an install is personal and available in every project, exactly as Pi behaves. `--workspace` installs into `<workspace>/.orrery/extensions/<id>/` for an extension the repo itself needs and should commit. `--user` is accepted for symmetry. Discovery already scans `<layer-root>/extensions/` at every layer (plan 10, landed), so nothing new is needed on the read side.

**Sources, Pi-style.** One positional argument, disambiguated by prefix:

| Form | Resolves to |
|---|---|
| `orrery install buildgraph` | the signed **registry** index (the default, and the only verified path) |
| `orrery install buildgraph@1.2.0` | the same, pinned to a version |
| `orrery install github:owner/repo` · `…#tag` · `…#sha` | a git host; `sha` is the only form that is reproducible |
| `orrery install https://…/repo.git` | any git URL |
| `orrery install crate:orrery-ext-buildgraph` | crates.io |
| `orrery install npm:@scope/orrery-ext-x` | npm |
| `orrery install ./path` · `file:../path` | a local directory |
| `orrery install ./path --link` | symlink instead of copy — the development loop, never for real use |

`remove` finds the extension across layers and, if it is installed at more than one, names them and asks which rather than guessing.

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
- `harness/core/crates/orrery-registry/tests/{verify,pin,diff,tamper}.rs`
- `harness/core/crates/orrery-registry/tests/fixtures/` — a signed index, a tampered index, key pairs

---

## Tasks

### Task 1 · The index format

Files: `src/index.rs`

- [ ] **Failing test first.** `index::parses_and_round_trips`; `index::unknown_schema_is_refused`; `index::expired_index_is_refused`.
- [ ] Implement.

### Task 2 · Signatures

Files: `src/verify.rs`, `tests/verify.rs`

- [ ] **Decision to record in this file:** `ed25519-dalek` (simple, small, we manage the keys) vs. sigstore (no key management, but a large dependency and an online dependency at verify time). **Recommendation: ed25519-dalek**, because an air-gapped enterprise is exactly the customer here and sigstore's transparency log assumes reachability. Write the decision and the reasoning down.
- [ ] **Failing test first.** `verify::good_signature_passes`; `verify::tampered_index_fails`; `verify::wrong_key_fails`; `verify::rotated_key_with_overlap_works`.
- [ ] Implement.

### Task 3 · Fetch and hash

Files: `src/fetch.rs`

- [ ] **Failing test first.** `fetch::hash_mismatch_refuses` — a package whose bytes do not match the pinned `sha256` is refused, with the expected and actual hashes in the error.
- [ ] `fetch::nothing_executes_before_verification` — the important one. Fetch a package containing a build script that would touch a sentinel file; assert the sentinel is untouched after a failed verification.
- [ ] Implement over the broker's `net`.

### Task 4 · `requires` mirroring

Files: `src/verify.rs`, `tests/tamper.rs`

- [ ] **Failing test first.** `tamper::manifest_requires_more_than_the_index` — a package whose manifest asks for `creds(*)` while the index says only `read`; assert a **hard failure**, audited as a tampering signal, not a prompt.
- [ ] Implement.

### Task 5 · The grant diff surface

Files: `src/diff.rs`, `tests/diff.rs`

- [ ] **Failing test first.** `diff::new_capability_on_upgrade_is_flagged` — 1.2.0 → 1.3.0 adding `net`; the diff marks it new and defaults to deny.
- [ ] `diff::unchanged_capabilities_are_shown_as_already_allowed`.
- [ ] `diff::renders_as_a_surface` — a `Surface` (a `form` or a `stack` of `question`s), so every client draws it.
- [ ] `diff::deny_degrades_not_fails` — denying `spawn` disables the spawn-needing tool and leaves the rest working, ledger says `degraded`.
- [ ] Implement.

### Task 6 · Pinning and managed enforcement

Files: `src/pin.rs`, `tests/pin.rs`

- [ ] **Failing test first, and it is the phase-8 criterion.** `pin::unpinned_refuses_under_managed` — with `unpinned = "refuse"`, an extension absent from the index is `Skipped { Unsigned }`, the reason and the index URL are in the ledger, and **a user-layer setting cannot override it**.
- [ ] `pin::warn_mode_loads_with_a_ledger_warning`.
- [ ] `pin::version_set_is_exact` — a pinned 1.2.0 refuses 1.2.1.
- [ ] Implement.

### Task 7 · `orrery install`

Files: `src/{lib,source,install}.rs` (the CLI surface is plan 17)

- [ ] **Failing test first.** `install::end_to_end` — against a local fixture registry: resolve, fetch, verify, show the diff, approve, load, and see it in the ledger.
- [ ] `install::offline_uses_the_cache` — a previously verified package installs without network.
- [ ] Implement.

### Task 8 · Source resolution

Files: `src/source.rs`

Every test here must run **offline**: a local bare git repo in a tempdir stands in for GitHub, and a local directory for crates.io/npm tarballs. No test fetches from a real host.

- [ ] **Failing test first.** `source::parses_every_form` — a table over the seven forms in the Architecture table above, each producing the right `Source` variant. Ambiguity rules pinned: a bare word is the registry, a leading `./` or `../` is a path, everything else needs its prefix.
- [ ] `source::bare_name_is_the_registry` — `orrery install buildgraph` never silently falls through to crates.io. If it is not in the index, that is an error naming the index URL, not a guess at another host.
- [ ] `source::version_pin` — `name@1.2.0` resolves to exactly that version; a missing version is an error, not the latest.
- [ ] `source::git_sha_is_reproducible_tag_is_not` — installing `github:o/r#v1.2` records the resolved commit sha in the ledger, so the install is auditable even though the ref is mutable.
- [ ] Implement the resolvers. Git via the broker's `spawn` of the system git (the ADE already decided gitoxide has no network features — reuse that reasoning); crates.io and npm via the broker's `net`; local via the broker's `read`.

### Task 9 · Install target and layers

Files: `src/install.rs`

- [ ] **Failing test first.** `install::defaults_to_the_user_layer` — `orrery install x` with no flag lands in `~/.orrery/extensions/x/` and is discovered from a different workspace.
- [ ] `install::workspace_flag` — `--workspace` lands in `<workspace>/.orrery/extensions/x/`.
- [ ] `install::link_is_a_symlink_and_marked_development` — `--link ./x` creates a link, and the ledger entry says `Development`, because the code can change under the harness between runs.
- [ ] `remove::ambiguous_across_layers_asks` — installed at user *and* workspace, `orrery remove x` names both and refuses to guess.
- [ ] Implement.

### Task 10 · Unpinned sources are visibly unpinned

Files: `src/install.rs`, `src/pin.rs`

This is the task that keeps the ergonomics honest.

- [ ] **Failing test first.** `pin::non_registry_source_is_recorded_unpinned` — a git install loads with `pinned: false` in the ledger and an audit entry naming the source; a registry install records `pinned: true` with the rule that verified it.
- [ ] `pin::managed_refuse_blocks_every_non_registry_source` — with `unpinned = "refuse"`, all five non-registry forms are refused, the message names the managed file, and **a user-layer setting cannot override it**.
- [ ] `pin::warn_mode_installs_loudly`.
- [ ] `pin::link_is_always_unpinned` — `--link` cannot be pinned even under `allow`.
- [ ] Implement.

---

## Done when

- `cargo test -p orrery-registry` green, including the tamper tests.
- An admin pins a version set; unpinned extensions refuse to load and say why.
- A capability added in an upgrade is surfaced distinctly and defaults to deny.
- No package code executes before its signature is verified.
- All seven source forms install, offline, in tests; the five non-registry ones are recorded `pinned: false` and are refused under managed `unpinned = "refuse"`.
- `orrery install x` with no flag lands in `~/.orrery/extensions/x/`.

## Open questions

1. **Key rotation and distribution.** One key in managed config is simple but has no revocation story. Minimum viable: two keys with an overlap window and an `expires` on the index. Is that enough for a real security team? Ask one.
2. **npm packages.** Node extensions ship on npm, which has its own integrity story (`integrity` in the lockfile). Do we duplicate the hash pin, or lean on npm's? Duplicating is more uniform and more work. Lean duplicate — one verification path for every runtime.
3. **What about the ADE's existing extension registry?** `ade/src-tauri/src/extensions/registry.rs` already does sha256-verified zips from a release repo, with a `minOrreryVersion` gate. It is a different problem (tree-sitter grammars, not code with capabilities) but the fetch-and-verify code may be worth reading before writing this.
4. **Transitive dependencies.** We pin an extension; its crates.io dependencies are pinned by its lockfile, which we do not review. A malicious transitive dependency is still a hole — the wasm sandbox closes it, the `native` and `node` runtimes do not. Say this plainly in the threat model rather than implying the registry solves it.
