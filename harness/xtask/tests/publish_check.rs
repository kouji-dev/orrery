//! Plan 18, task 5: every `publish = true` crate can actually be published.
//!
//! `cargo publish --dry-run` is the real thing, and it needs the network: it
//! resolves each dependency against the crates.io index. This repo's hard rule
//! is that **no test reaches the network**, so the dry run is a CI step and the
//! check here is the offline half — the preconditions cargo evaluates from the
//! manifests alone, which is where all of the failures we can actually cause
//! live:
//!
//! - a path dependency with no `version`, which `cargo package` strips and then
//!   cannot resolve;
//! - a dependency on a `publish = false` crate, which can never be resolved by
//!   anyone else;
//! - the manifest fields crates.io rejects an upload without.
//!
//! Each fixture under `tests/fixtures/` is a standalone workspace laid out like
//! the real repo and breaks exactly one of them.

use std::path::PathBuf;

use xtask::publish_check;

fn fixture(name: &str) -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("tests/fixtures")
        .join(name)
}

fn violations(name: &str) -> Vec<String> {
    publish_check::check(&fixture(name)).expect("cargo metadata runs on the fixture")
}

/// The whole point: this repository's published crates are publishable.
///
/// Scoped to `harness/`. The ADE's crates are a Tauri app that is shipped as an
/// installer rather than uploaded to crates.io; they are not what this rule is
/// about, and pretending otherwise would make the check something people turn
/// off.
#[test]
fn every_published_crate_packages() {
    let root = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..");
    assert_eq!(
        publish_check::check(&root).expect("cargo metadata runs"),
        Vec::<String>::new()
    );
}

/// A clean fixture reports nothing.
#[test]
fn a_clean_workspace_is_green() {
    assert_eq!(violations("ok"), Vec::<String>::new());
}

/// The failure this whole task exists to catch: `path` without `version`. It
/// builds in-tree and is unpublishable, and nothing says so until the upload.
#[test]
fn a_published_crate_may_not_depend_on_a_path_without_a_version() {
    let found = violations("publish-path-dep-without-version");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("fixture-core-facade") && found[0].contains("fixture-core-proto"),
        "the violation names both crates: {found:?}"
    );
    assert!(found[0].contains("version"), "{found:?}");
}

/// A published crate cannot depend on one nobody else can download.
#[test]
fn a_published_crate_may_not_depend_on_an_unpublished_one() {
    let found = violations("publish-depends-on-unpublished");
    assert_eq!(found.len(), 1, "{found:?}");
    assert!(
        found[0].contains("fixture-core-facade") && found[0].contains("fixture-core-kernel"),
        "the violation names both crates: {found:?}"
    );
}

/// crates.io refuses an upload with no description, no license and no
/// repository. Cheap to check here; expensive to discover at release time.
#[test]
fn a_published_crate_carries_the_fields_crates_io_requires() {
    let found = violations("publish-missing-metadata");
    assert_eq!(found.len(), 1, "{found:?}");
    for field in ["description", "license", "repository"] {
        assert!(found[0].contains(field), "{field} is not named: {found:?}");
    }
}

/// A `publish = false` crate is exempt from all of it. It is the escape hatch,
/// and it has to keep working or the rule becomes a reason to publish things
/// that should not be.
#[test]
fn an_unpublished_crate_is_not_held_to_any_of_this() {
    assert_eq!(
        violations("publish-unpublished-is-exempt"),
        Vec::<String>::new()
    );
}
