//! Making an index, which is the half of plan 15 that did not exist.
//!
//! The crate could **verify** a signed index and refuse an unpinned extension,
//! and nothing anywhere could produce the index it verified. "An admin pins a
//! version set" was only true if somebody handed the admin a file made by a
//! tool that did not ship. This is that tool's library half; the commands are
//! `orrery registry init|add|sign|verify`.
//!
//! Everything here is offline: a local mirror directory stands in for
//! crates.io, and the signing key is derived from a seed in the test.

mod common;

use orrery_registry::author;
use orrery_registry::index::{EntrySource, Index};
use orrery_registry::{DirFetcher, verify};

use common::{SEED_CURRENT, key, keyring, now, write_package};

/// Sign, then verify, through the public API — and the text that is written is
/// the text that was signed.
///
/// The failure this closes: a signer that renders the document once to sign it
/// and the caller that renders it again to write it are two renderings, and a
/// signature over the first does not verify against the second. So signing
/// **returns the bytes**, and there is no way to get a signature without them.
#[test]
fn a_signed_index_verifies_and_the_text_is_the_signed_text() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let signer = key("managed", &SEED_CURRENT);
    let package = write_package(dir.path(), "buildgraph", "1.2.0", "read = [\"./**\"]");
    let mirror = dir.path().join("mirror");
    let staged = mirror.join("orrery-ext-buildgraph");
    orrery_registry::fetch::copy_tree(&package, &staged).expect("a mirror");

    let source = EntrySource::CratesIo {
        name: "orrery-ext-buildgraph".to_owned(),
    };
    let fetcher = DirFetcher::new(&mirror);
    let entry = author::entry_from_fetch(
        "buildgraph",
        "1.2.0".parse().expect("a semver"),
        source,
        &fetcher,
        &dir.path().join("staging"),
    )
    .expect("the package stages and hashes");
    assert!(
        entry.requires.iter().any(|r| r == "read(./**)"),
        "the entry mirrors what the manifest asks for: {:?}",
        entry.requires
    );

    let mut index = author::new_index("2026-09-18T00:00:00Z", "2026-12-18T00:00:00Z")
        .expect("a fresh index");
    index.extensions.push(entry);

    let signed = author::sign_index(&mut index, &signer).expect("it signs");
    let parsed = Index::parse(&signed.text, "index.toml").expect("the signed text parses");
    let ring = keyring(&[&signer]);
    let by = verify::verify_index(
        &parsed,
        &signed.text,
        &signed.signature,
        &ring,
        &now(),
        "index.toml",
        common::INDEX_URL,
    )
    .expect("the document signature verifies");
    assert_eq!(by, "managed");

    // …and every entry signature with it.
    for entry in &parsed.extensions {
        verify::verify_entry(entry, &ring, &now()).expect("the entry signature verifies");
    }
}

/// The hash an admin pins is the hash the installer will compute, because both
/// come from staging through the same fetcher.
#[test]
fn the_pinned_hash_is_what_the_installer_stages() {
    let dir = tempfile::tempdir().expect("a temporary directory");
    let package = write_package(dir.path(), "buildgraph", "1.2.0", "read = [\"./**\"]");
    let mirror = dir.path().join("mirror");
    orrery_registry::fetch::copy_tree(&package, &mirror.join("orrery-ext-buildgraph"))
        .expect("a mirror");

    let source = EntrySource::CratesIo {
        name: "orrery-ext-buildgraph".to_owned(),
    };
    let fetcher = DirFetcher::new(&mirror);
    let entry = author::entry_from_fetch(
        "buildgraph",
        "1.2.0".parse().expect("a semver"),
        source.clone(),
        &fetcher,
        &dir.path().join("staging"),
    )
    .expect("it stages");

    // What `fetch_and_verify` will do, on its own path.
    let quarantine = dir.path().join("quarantine");
    std::fs::create_dir_all(&quarantine).expect("a tempdir");
    orrery_registry::fetch::PackageFetcher::stage(&fetcher, "buildgraph", &source, &quarantine)
        .expect("it stages again");
    assert_eq!(
        entry.sha256,
        orrery_registry::tree_sha256(&quarantine).expect("a readable tree"),
        "the admin pins the hash the install will see"
    );
}

/// A source spelling round-trips, so `--source crates-io:x` and what the index
/// prints are the same string.
#[test]
fn a_source_round_trips_through_its_spelling() {
    for text in [
        "crates-io:orrery-ext-buildgraph",
        "npm:@scope/orrery-ext-x",
        "url:https://packages.corp.internal/buildgraph-1.2.0.tar.gz",
    ] {
        let source: EntrySource = text.parse().expect("a source spelling");
        assert_eq!(source.to_string(), text);
    }
    assert!("github:owner/repo".parse::<EntrySource>().is_err());
}

/// Editing the document after it is signed stops it verifying. The signature is
/// over the text, and the text changed.
#[test]
fn an_edited_index_stops_verifying() {
    let signer = key("managed", &SEED_CURRENT);
    let mut index = author::new_index("2026-09-18T00:00:00Z", "2026-12-18T00:00:00Z")
        .expect("a fresh index");
    let signed = author::sign_index(&mut index, &signer).expect("it signs");
    let tampered = signed.text.replace("2026-12-18", "2027-12-18");
    assert_ne!(tampered, signed.text);

    let parsed = Index::parse(&tampered, "index.toml").expect("still TOML");
    let ring = keyring(&[&signer]);
    assert!(
        verify::verify_index(
            &parsed,
            &tampered,
            &signed.signature,
            &ring,
            &now(),
            "index.toml",
            common::INDEX_URL,
        )
        .is_err(),
        "an index somebody edited after signing is not a signed index"
    );
}

/// The signature lives at one path, and one function says where — the reader
/// and the writer cannot disagree about it.
#[test]
fn the_signature_path_is_one_answer() {
    let path = std::path::Path::new("/srv/registry/index.toml");
    assert_eq!(
        author::signature_path(path),
        std::path::Path::new("/srv/registry/index.toml.sig")
    );
}
