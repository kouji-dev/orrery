//! Plan 15 task 4: the `requires` mirror, and why a mismatch is not a prompt.

mod common;

use orrery_ext_api::ExtensionManifest;
use orrery_registry::RegistryError;
use orrery_registry::verify::check_requires;

#[test]
fn manifest_requires_more_than_the_index() {
    let signer = common::key("org-2026", &common::SEED_CURRENT);
    let tmp = tempfile::tempdir().unwrap();

    // The index was reviewed as `read` only. The package asks for credentials.
    let pkg = common::write_package(
        tmp.path(),
        "buildgraph",
        "1.2.0",
        "read = [\"$WORKSPACE/**\"]\ncreds = true\n",
    );
    let entry = common::entry_for(
        &signer,
        &pkg,
        "buildgraph",
        "1.2.0",
        &["read($WORKSPACE/**)"],
    );
    let manifest = ExtensionManifest::from_path(pkg.join("orrery.toml")).unwrap();

    let err = check_requires(&entry, &manifest).unwrap_err();
    let RegistryError::RequiresMismatch { id, extra } = &err else {
        panic!("expected a hard RequiresMismatch, got {err:?}");
    };
    assert_eq!(id, "buildgraph");
    assert!(
        extra.contains("creds"),
        "the tampering signal must name what was asked for: {extra}"
    );
    // The message says what it is, so nobody reads it as something to approve.
    let text = err.to_string();
    assert!(text.contains("tampering signal"), "{text}");
    assert!(text.contains("not a prompt"), "{text}");
}

#[test]
fn a_widened_scope_is_a_mismatch_too() {
    // The subtle one: same aspect, wider pattern. The review covered
    // `$WORKSPACE/**`; the package wants the whole disk.
    let signer = common::key("org-2026", &common::SEED_CURRENT);
    let tmp = tempfile::tempdir().unwrap();
    let pkg = common::write_package(tmp.path(), "buildgraph", "1.2.0", "read = [\"/**\"]");
    let entry = common::entry_for(
        &signer,
        &pkg,
        "buildgraph",
        "1.2.0",
        &["read($WORKSPACE/**)"],
    );
    let manifest = ExtensionManifest::from_path(pkg.join("orrery.toml")).unwrap();
    let err = check_requires(&entry, &manifest).unwrap_err();
    assert!(matches!(err, RegistryError::RequiresMismatch { .. }), "{err:?}");
}

#[test]
fn asking_for_less_than_the_index_is_fine() {
    // A patch release that drops a capability is not tampering; the index is
    // simply ahead of it.
    let signer = common::key("org-2026", &common::SEED_CURRENT);
    let tmp = tempfile::tempdir().unwrap();
    let pkg = common::write_package(tmp.path(), "buildgraph", "1.2.1", "read = [\"$WORKSPACE/**\"]");
    let entry = common::entry_for(
        &signer,
        &pkg,
        "buildgraph",
        "1.2.1",
        &["read($WORKSPACE/**)", "spawn(java)"],
    );
    let manifest = ExtensionManifest::from_path(pkg.join("orrery.toml")).unwrap();
    check_requires(&entry, &manifest).expect("a narrower manifest is allowed");
}

#[test]
fn an_unqualified_index_entry_covers_any_scope() {
    let signer = common::key("org-2026", &common::SEED_CURRENT);
    let tmp = tempfile::tempdir().unwrap();
    let pkg = common::write_package(tmp.path(), "buildgraph", "1.2.0", "net = [\"docs.rs\"]");
    let entry = common::entry_for(&signer, &pkg, "buildgraph", "1.2.0", &["net"]);
    let manifest = ExtensionManifest::from_path(pkg.join("orrery.toml")).unwrap();
    check_requires(&entry, &manifest).expect("`net` unqualified covers `net(docs.rs)`");
}
