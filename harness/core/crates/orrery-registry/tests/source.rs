//! Plan 15 task 8: source resolution, entirely offline.
//!
//! A local bare git repository stands in for GitHub and a local directory for
//! a crates.io or npm mirror. Nothing here opens a socket.

mod common;

use std::path::PathBuf;
use std::str::FromStr;

use orrery_registry::source::SystemGit;
use orrery_registry::{GitRef, GitRunner, RegistryError, Source};

#[test]
fn parses_every_form() {
    let cases: Vec<(&str, Source)> = vec![
        (
            "buildgraph",
            Source::Registry {
                name: "buildgraph".to_owned(),
                version: None,
            },
        ),
        (
            "buildgraph@1.2.0",
            Source::Registry {
                name: "buildgraph".to_owned(),
                version: Some("1.2.0".parse().unwrap()),
            },
        ),
        (
            "github:owner/repo",
            Source::Git {
                url: "https://github.com/owner/repo.git".to_owned(),
                reference: GitRef::Default,
                shorthand: Some("github:owner/repo".to_owned()),
            },
        ),
        (
            "github:owner/repo#v1.2",
            Source::Git {
                url: "https://github.com/owner/repo.git".to_owned(),
                reference: GitRef::Named("v1.2".to_owned()),
                shorthand: Some("github:owner/repo#v1.2".to_owned()),
            },
        ),
        (
            "https://git.corp.internal/x/repo.git",
            Source::Git {
                url: "https://git.corp.internal/x/repo.git".to_owned(),
                reference: GitRef::Default,
                shorthand: None,
            },
        ),
        (
            "crate:orrery-ext-buildgraph",
            Source::Crate {
                name: "orrery-ext-buildgraph".to_owned(),
                version: None,
            },
        ),
        (
            "npm:@scope/orrery-ext-x",
            Source::Npm {
                name: "@scope/orrery-ext-x".to_owned(),
                version: None,
            },
        ),
        (
            "./path",
            Source::Path {
                path: PathBuf::from("./path"),
            },
        ),
        (
            "file:../path",
            Source::Path {
                path: PathBuf::from("../path"),
            },
        ),
    ];
    for (input, expected) in cases {
        let got = Source::from_str(input).unwrap_or_else(|e| panic!("`{input}`: {e}"));
        assert_eq!(got, expected, "`{input}`");
    }

    // A 40-hex ref is a sha and is reproducible; a tag is not.
    let sha = "a".repeat(40);
    let Source::Git { reference, .. } = Source::from_str(&format!("github:o/r#{sha}")).unwrap()
    else {
        panic!()
    };
    assert_eq!(reference, GitRef::Sha(sha));
    assert!(reference.is_reproducible());
    assert!(!GitRef::Named("v1.2".to_owned()).is_reproducible());
    assert!(!GitRef::Default.is_reproducible());
}

#[test]
fn an_unprefixed_url_like_thing_is_refused() {
    // The ambiguity rule, stated as a refusal: everything that is not a bare
    // name or a `./` path needs its prefix.
    for input in [
        "github.com/owner/repo",
        "owner/repo",
        "C:/somewhere/local",
        "@scope/pkg",
    ] {
        let err = Source::from_str(input).unwrap_err();
        assert!(
            matches!(err, RegistryError::SourceSyntax { .. }),
            "`{input}` parsed as something: {err:?}"
        );
    }
}

#[test]
fn bare_name_is_the_registry() {
    // Never a silent fall-through to crates.io: an id the index does not hold
    // is an error that names the index.
    let source = Source::from_str("buildgraph").unwrap();
    assert!(source.is_registry());

    let index = common::index_of(Vec::new());
    let err = orrery_registry::pin::resolve_entry(&index, &source, common::INDEX_URL).unwrap_err();
    let RegistryError::NotInIndex { id, index: url } = &err else {
        panic!("expected NotInIndex, got {err:?}");
    };
    assert_eq!(id, "buildgraph");
    assert_eq!(url, common::INDEX_URL);
    assert!(
        !err.to_string().contains("crates.io"),
        "the message must not suggest another host: {err}"
    );
}

#[test]
fn version_pin() {
    let signer = common::key("org-2026", &common::SEED_CURRENT);
    let tmp = tempfile::tempdir().unwrap();
    let a = common::write_package(tmp.path(), "buildgraph", "1.2.0", "read = [\"./**\"]");
    let b = common::write_package(&tmp.path().join("v121"), "buildgraph", "1.2.1", "read = [\"./**\"]");
    let index = common::index_of(vec![
        common::entry_for(&signer, &a, "buildgraph", "1.2.0", &["read(./**)"]),
        common::entry_for(&signer, &b, "buildgraph", "1.2.1", &["read(./**)"]),
    ]);

    let exact = Source::from_str("buildgraph@1.2.0").unwrap();
    let entry =
        orrery_registry::pin::resolve_entry(&index, &exact, common::INDEX_URL).expect("pinned");
    assert_eq!(entry.version.to_string(), "1.2.0");

    // A version the index does not have is refused, listing what it has.
    let missing = Source::from_str("buildgraph@9.9.9").unwrap();
    let err =
        orrery_registry::pin::resolve_entry(&index, &missing, common::INDEX_URL).unwrap_err();
    let RegistryError::VersionNotInIndex { available, .. } = &err else {
        panic!("expected VersionNotInIndex, got {err:?}");
    };
    assert!(available.contains("1.2.0") && available.contains("1.2.1"), "{available}");

    // No version, several in the index: an error, **not** the latest. Picking
    // newest would make one command mean two things either side of a publish.
    let bare = Source::from_str("buildgraph").unwrap();
    let err = orrery_registry::pin::resolve_entry(&index, &bare, common::INDEX_URL).unwrap_err();
    assert!(matches!(err, RegistryError::VersionRequired { .. }), "{err:?}");
}

#[test]
fn git_sha_is_reproducible_tag_is_not() {
    if !common::git_available() {
        eprintln!("no git on PATH; skipping");
        return;
    }
    let tmp = tempfile::tempdir().unwrap();
    let pkg = common::write_package(tmp.path(), "buildgraph", "1.2.0", "read = [\"./**\"]");
    let (bare, sha) = common::bare_repo_with(tmp.path(), "buildgraph", &pkg, "v1.2");

    let source = Source::Git {
        url: common::local_remote(&bare),
        reference: GitRef::Named("v1.2".to_owned()),
        shorthand: Some("github:o/r#v1.2".to_owned()),
    };
    let Source::Git { url, reference, .. } = &source else {
        unreachable!()
    };
    assert!(
        !reference.is_reproducible(),
        "a tag is mutable, so the source string alone is not a pin"
    );

    let dest = tmp.path().join("clone");
    let resolved = SystemGit
        .clone_at(url, reference, &dest)
        .expect("a local clone");
    assert_eq!(
        resolved, sha,
        "the resolved commit sha is what makes a tag install auditable"
    );
    assert!(dest.join("orrery.toml").exists());
}
