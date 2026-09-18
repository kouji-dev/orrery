//! Plan 15 task 2: signatures, and key rotation with an overlap window.

mod common;

use orrery_registry::index::Index;
use orrery_registry::verify::{verify_detached, verify_index};
use orrery_registry::{Keyring, PublicKey, RegistryError, Timestamp};

#[test]
fn good_signature_passes() {
    let signer = common::key("org-2026", &common::SEED_CURRENT);
    let ring = common::keyring(&[&signer]);
    let index = common::index_of(Vec::new());
    let (text, sig) = common::sign_index(&signer, &index);

    let parsed = Index::parse(&text, "index.toml").unwrap();
    let key = verify_index(
        &parsed,
        &text,
        &sig,
        &ring,
        &common::now(),
        "index.toml",
        common::INDEX_URL,
    )
    .expect("a good signature");
    assert_eq!(key, "org-2026", "the ledger has to name what verified it");
}

#[test]
fn tampered_index_fails() {
    let signer = common::key("org-2026", &common::SEED_CURRENT);
    let ring = common::keyring(&[&signer]);
    let dir = tempfile::tempdir().unwrap();
    let pkg = common::write_package(dir.path(), "buildgraph", "1.2.0", "read = [\"./**\"]");
    let entry = common::entry_for(&signer, &pkg, "buildgraph", "1.2.0", &["read(./**)"]);
    let index = common::index_of(vec![entry]);
    let (text, sig) = common::sign_index(&signer, &index);

    // One character of the pinned hash, changed after signing.
    let tampered = text.replacen("sha256 = \"", "sha256 = \"0", 1);
    assert_ne!(tampered, text, "the fixture did not actually change");

    let parsed = Index::parse(&tampered, "index.toml").unwrap();
    let err = verify_index(
        &parsed,
        &tampered,
        &sig,
        &ring,
        &common::now(),
        "index.toml",
        common::INDEX_URL,
    )
    .unwrap_err();
    assert!(matches!(err, RegistryError::BadSignature { .. }), "{err:?}");
}

#[test]
fn wrong_key_fails() {
    let signer = common::key("stranger", &common::SEED_STRANGER);
    let org = common::key("org-2026", &common::SEED_CURRENT);
    let ring = common::keyring(&[&org]);
    let index = common::index_of(Vec::new());
    let (text, sig) = common::sign_index(&signer, &index);

    let parsed = Index::parse(&text, "index.toml").unwrap();
    let err = verify_index(
        &parsed,
        &text,
        &sig,
        &ring,
        &common::now(),
        "index.toml",
        common::INDEX_URL,
    )
    .unwrap_err();
    // The signature names a key the ring does not hold at all, which is a
    // different and more useful message than "it does not verify".
    let RegistryError::UnknownKey { key, .. } = &err else {
        panic!("expected UnknownKey, got {err:?}");
    };
    assert_eq!(key, "stranger");
}

#[test]
fn rotated_key_with_overlap_works() {
    // The scenario open question 1 settles on as the minimum viable one: two
    // keys, an overlap window, and an `expires` on the index.
    let old = common::key("org-2025", &common::SEED_PREVIOUS);
    let new = common::key("org-2026", &common::SEED_CURRENT);
    let ring = Keyring::new(vec![
        PublicKey::new(
            "org-2025",
            &old.public_hex(),
            "2025-01-01T00:00:00Z",
            // Overlaps the new key by a quarter.
            "2026-12-31T00:00:00Z",
        )
        .unwrap(),
        PublicKey::new(
            "org-2026",
            &new.public_hex(),
            "2026-09-01T00:00:00Z",
            "2027-12-31T00:00:00Z",
        )
        .unwrap(),
    ]);

    let message = b"an index, in the abstract";
    let during = common::now();
    for signer in [&old, &new] {
        let sig = signer.sign(message);
        let who = verify_detached("index", message, &sig, &ring, &during).expect("in force");
        assert_eq!(who, signer.id());
    }

    // After the overlap closes, only the new one still verifies.
    let after = Timestamp::parse("now", "2027-02-01T00:00:00Z").unwrap();
    let err = verify_detached("index", message, &old.sign(message), &ring, &after).unwrap_err();
    assert!(matches!(err, RegistryError::UnknownKey { .. }), "{err:?}");
    verify_detached("index", message, &new.sign(message), &ring, &after).expect("still in force");
}

#[test]
fn an_entry_signature_does_not_travel_between_indexes() {
    // The entry signature covers (id, version, source, sha256), so lifting a
    // signed entry out of one index and giving it another id does not verify.
    let signer = common::key("org-2026", &common::SEED_CURRENT);
    let ring = common::keyring(&[&signer]);
    let dir = tempfile::tempdir().unwrap();
    let pkg = common::write_package(dir.path(), "buildgraph", "1.2.0", "read = [\"./**\"]");
    let mut entry = common::entry_for(&signer, &pkg, "buildgraph", "1.2.0", &["read(./**)"]);
    orrery_registry::verify::verify_entry(&entry, &ring, &common::now()).expect("as signed");

    entry.id = "something-else".to_owned();
    let err = orrery_registry::verify::verify_entry(&entry, &ring, &common::now()).unwrap_err();
    assert!(matches!(err, RegistryError::BadSignature { .. }), "{err:?}");
}

#[test]
fn a_malformed_signature_says_so() {
    let signer = common::key("org-2026", &common::SEED_CURRENT);
    let ring = common::keyring(&[&signer]);
    let err = verify_detached("index", b"x", "org-2026:beef", &ring, &common::now()).unwrap_err();
    assert!(matches!(err, RegistryError::Malformed { .. }), "{err:?}");
}
