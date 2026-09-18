//! `deps-check` is the one thing in the scaffold that enforces a rule, so it is
//! the one thing with real tests.
//!
//! Each fixture under `tests/fixtures/` is a standalone cargo workspace laid out
//! like the real repo — `harness/core/crates/*`, `harness/extensions/crates/*` —
//! and violates exactly one rule. `18-writing-an-extension.md` task 2 owns these
//! tests and extends them; they start here.

use std::path::PathBuf;

use xtask::deps_check;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn violations(name: &str) -> Vec<String> {
    deps_check::check(&fixture(name)).expect("cargo metadata runs on the fixture")
}

/// A workspace that follows every rule reports nothing.
#[test]
fn clean_workspace_is_green() {
    assert_eq!(violations("ok"), Vec::<String>::new());
}

/// Rule 1 — no `core/` crate depends on an `extensions/` or `clients/` crate.
#[test]
fn core_may_not_depend_on_an_extension() {
    let found = violations("core-depends-on-extension");
    assert_eq!(
        found.len(),
        1,
        "expected exactly one violation, got {found:#?}"
    );
    let msg = &found[0];
    assert!(
        msg.contains("fixture-core-kernel"),
        "names the core crate: {msg}"
    );
    assert!(
        msg.contains("fixture-ext-thing"),
        "names the extension crate: {msg}"
    );
}

/// Rule 1's single allow-list entry: `orrery-harness`, the facade, links the
/// first-party set behind cargo features.
#[test]
fn orrery_harness_is_the_one_exception() {
    assert_eq!(violations("harness-facade-exception"), Vec::<String>::new());
}

/// Rule 2 — an `extensions/` → `core/` dependency must carry a version, or the
/// crate cannot be published.
#[test]
fn extension_dependency_must_carry_a_version() {
    let found = violations("extension-without-version");
    assert_eq!(
        found.len(),
        1,
        "expected exactly one violation, got {found:#?}"
    );
    let msg = &found[0];
    assert!(
        msg.contains("fixture-ext-thing"),
        "names the extension crate: {msg}"
    );
    assert!(
        msg.contains("fixture-core-proto"),
        "names the core crate: {msg}"
    );
    assert!(msg.contains("version"), "says what is missing: {msg}");
}

/// Rule 2 — and it must name a `publish = true` crate. Kernel internals are not
/// an API.
#[test]
fn extension_dependency_must_be_published() {
    let found = violations("extension-on-unpublished-core");
    assert_eq!(
        found.len(),
        1,
        "expected exactly one violation, got {found:#?}"
    );
    let msg = &found[0];
    assert!(
        msg.contains("fixture-ext-thing"),
        "names the extension crate: {msg}"
    );
    assert!(
        msg.contains("fixture-core-kernel"),
        "names the core crate: {msg}"
    );
    assert!(msg.contains("publish"), "says why: {msg}");
}

/// Rule 3 — not even a dev-dependency. Extension tests use the mock broker in
/// `orrery-ext-api::testing`, the same one `orrery ext test` uses.
#[test]
fn extension_may_not_dev_depend_on_an_unpublished_core_crate() {
    let found = violations("extension-dev-depends-unpublished");
    assert_eq!(
        found.len(),
        1,
        "expected exactly one violation, got {found:#?}"
    );
    let msg = &found[0];
    assert!(
        msg.contains("fixture-ext-thing"),
        "names the extension crate: {msg}"
    );
    assert!(
        msg.contains("fixture-core-kernel"),
        "names the core crate: {msg}"
    );
    assert!(
        msg.contains("dev-depend"),
        "says it is a dev-dependency: {msg}"
    );
}

/// The real workspace passes its own check.
#[test]
fn this_repo_is_green() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("../..")
        .canonicalize()
        .expect("the repo root exists");
    assert_eq!(
        deps_check::check(&root).expect("cargo metadata runs"),
        Vec::<String>::new()
    );
}
