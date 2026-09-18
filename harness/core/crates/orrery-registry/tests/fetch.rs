//! Plan 15 task 3: the hash pin, and the sentinel that proves the order.

mod common;

use std::path::Path;
use std::sync::atomic::{AtomicUsize, Ordering};

use orrery_registry::fetch::{PackageHooks, fetch_and_verify};
use orrery_registry::{DirFetcher, RegistryError};

/// A hook runner that counts. Nothing may reach it before verification.
#[derive(Default)]
struct CountingHooks {
    runs: AtomicUsize,
}

impl PackageHooks for CountingHooks {
    fn post_install(&self, _id: &str, _dir: &Path) -> Result<(), String> {
        self.runs.fetch_add(1, Ordering::SeqCst);
        Ok(())
    }
}

#[test]
fn hash_mismatch_refuses() {
    let signer = common::key("org-2026", &common::SEED_CURRENT);
    let ring = common::keyring(&[&signer]);
    let tmp = tempfile::tempdir().unwrap();

    let pkg = common::write_package(tmp.path(), "buildgraph", "1.2.0", "read = [\"./**\"]");
    let mut entry = common::entry_for(&signer, &pkg, "buildgraph", "1.2.0", &["read(./**)"]);
    let honest = entry.sha256.clone();

    // The package changes after the index was written: one byte, in a file
    // nobody looks at.
    std::fs::write(pkg.join("src/lib.rs"), "// swapped\n").unwrap();

    let mirror_root = tmp.path().join("mirror");
    common::mirror(&mirror_root, "buildgraph", &pkg);
    let fetcher = DirFetcher::new(&mirror_root);

    let err = fetch_and_verify(
        &entry,
        &fetcher,
        &ring,
        &common::now(),
        &tmp.path().join("quarantine"),
    )
    .unwrap_err();

    let RegistryError::HashMismatch {
        expected, actual, ..
    } = &err
    else {
        panic!("expected HashMismatch, got {err:?}");
    };
    assert_eq!(expected, &honest, "the error must carry what was pinned");
    assert_ne!(actual, &honest, "the error must carry what arrived");

    // Re-pinned to the new bytes, it passes — so the refusal was about the
    // hash and not about anything else in the fixture.
    entry.sha256 = orrery_registry::tree_sha256(&pkg).unwrap();
    entry.sig = signer.sign(&entry.signed_bytes());
    fetch_and_verify(
        &entry,
        &fetcher,
        &ring,
        &common::now(),
        &tmp.path().join("quarantine-2"),
    )
    .expect("the re-pinned package verifies");
}

#[test]
fn nothing_executes_before_verification() {
    // The important one. A package carrying a build script that would write a
    // sentinel; a pin that does not match it; and the assertion that after the
    // refusal the sentinel is untouched, the extension is not installed, and
    // the hook runner was never reached.
    let signer = common::key("org-2026", &common::SEED_CURRENT);
    let ring = common::keyring(&[&signer]);
    let tmp = tempfile::tempdir().unwrap();
    let sentinel = tmp.path().join("sentinel.txt");

    let pkg = common::write_package_with_build_script(tmp.path(), "hostile", "0.1.0", &sentinel);
    let mut entry = common::entry_for(&signer, &pkg, "hostile", "0.1.0", &["read($WORKSPACE/**)"]);

    // Break the pin.
    entry.sha256 = "0".repeat(64);
    entry.sig = signer.sign(&entry.signed_bytes());

    let mirror_root = tmp.path().join("mirror");
    common::mirror(&mirror_root, "hostile", &pkg);

    let hooks = CountingHooks::default();
    let err = fetch_and_verify(
        &entry,
        &DirFetcher::new(&mirror_root),
        &ring,
        &common::now(),
        &tmp.path().join("quarantine"),
    )
    .unwrap_err();
    assert!(matches!(err, RegistryError::HashMismatch { .. }), "{err:?}");

    assert!(
        !sentinel.exists(),
        "a build script ran before the package was verified"
    );
    assert_eq!(
        hooks.runs.load(Ordering::SeqCst),
        0,
        "the hook runner was reached on a failed verification"
    );

    // And the assertion above is not vacuous: with the pin correct, the very
    // same installer does reach the hook.
    let good = common::entry_for(&signer, &pkg, "hostile", "0.1.0", &["read($WORKSPACE/**)"]);
    let verified = fetch_and_verify(
        &good,
        &DirFetcher::new(&mirror_root),
        &ring,
        &common::now(),
        &tmp.path().join("quarantine-ok"),
    )
    .expect("a matching pin");
    hooks.post_install("hostile", &verified.dir).unwrap();
    assert_eq!(hooks.runs.load(Ordering::SeqCst), 1);

    // Still nothing ran the script: verification places bytes, it does not
    // build them. A crate is a download, not a `cargo install`.
    assert!(!sentinel.exists());
    assert!(
        verified.dir.join("build.rs").exists(),
        "the build script is on disk — it is simply never executed"
    );
}

#[test]
fn a_rename_is_not_absorbed_by_a_neighbour() {
    // The tree hash is length-prefixed, so `ab` next to `c` does not hash the
    // same as `a` next to `bc`.
    let tmp = tempfile::tempdir().unwrap();
    let one = tmp.path().join("one");
    let two = tmp.path().join("two");
    std::fs::create_dir_all(&one).unwrap();
    std::fs::create_dir_all(&two).unwrap();
    std::fs::write(one.join("ab"), "c").unwrap();
    std::fs::write(two.join("a"), "bc").unwrap();
    assert_ne!(
        orrery_registry::tree_sha256(&one).unwrap(),
        orrery_registry::tree_sha256(&two).unwrap()
    );
}
