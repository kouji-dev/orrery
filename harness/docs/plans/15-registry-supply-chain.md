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

### Task 7 · `ext install`

Files: `src/lib.rs` (the CLI surface is plan 17)

- [ ] **Failing test first.** `install::end_to_end` — against a local fixture registry: resolve, fetch, verify, show the diff, approve, load, and see it in the ledger.
- [ ] `install::offline_uses_the_cache` — a previously verified package installs without network.
- [ ] Implement.

---

## Done when

- `cargo test -p orrery-registry` green, including the tamper tests.
- An admin pins a version set; unpinned extensions refuse to load and say why.
- A capability added in an upgrade is surfaced distinctly and defaults to deny.
- No package code executes before its signature is verified.

## Open questions

1. **Key rotation and distribution.** One key in managed config is simple but has no revocation story. Minimum viable: two keys with an overlap window and an `expires` on the index. Is that enough for a real security team? Ask one.
2. **npm packages.** Node extensions ship on npm, which has its own integrity story (`integrity` in the lockfile). Do we duplicate the hash pin, or lean on npm's? Duplicating is more uniform and more work. Lean duplicate — one verification path for every runtime.
3. **What about the ADE's existing extension registry?** `ade/src-tauri/src/extensions/registry.rs` already does sha256-verified zips from a release repo, with a `minOrreryVersion` gate. It is a different problem (tree-sitter grammars, not code with capabilities) but the fetch-and-verify code may be worth reading before writing this.
4. **Transitive dependencies.** We pin an extension; its crates.io dependencies are pinned by its lockfile, which we do not review. A malicious transitive dependency is still a hole — the wasm sandbox closes it, the `native` and `node` runtimes do not. Say this plainly in the threat model rather than implying the registry solves it.
