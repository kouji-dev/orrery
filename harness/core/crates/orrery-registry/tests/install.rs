//! Plan 15 tasks 7 and 9: the install end to end, where it lands, and `remove`.

mod common;

use std::str::FromStr;

use orrery_config::discover::{FsWalk, ItemKind, LayerRoot, discover};
use orrery_config::provenance::Provenanced;
use orrery_proto::{Layer, LoadOutcome};
use orrery_registry::{
    GitRef, InstallOptions, RegistryError, Source, Target, UnpinnedReason, install,
};

#[test]
fn end_to_end() {
    // Against a local fixture registry: resolve, fetch, verify, show the diff,
    // approve, install, and see it recorded.
    let mut fixture = common::Fixture::new();
    fixture.publish(
        "buildgraph",
        "1.2.0",
        "read = [\"$WORKSPACE/**\"]\nspawn = [\"java\"]\n",
        &["read($WORKSPACE/**)", "spawn(java)"],
    );

    let record = fixture
        .installer()
        .install(
            &Source::from_str("buildgraph@1.2.0").unwrap(),
            &InstallOptions::to(Target::User).allowing_everything(),
        )
        .expect("the whole pipeline");

    assert_eq!(record.ext.as_str(), "buildgraph");
    assert_eq!(record.version.to_string(), "1.2.0");
    assert!(record.is_pinned());
    assert!(record.path.join("orrery.toml").exists());

    // The diff was computed and every row was asked about.
    assert_eq!(record.diff.rows.len(), 2, "{:?}", record.diff.rows);
    assert_eq!(record.approval.denied, Vec::new());
    assert!(matches!(record.outcome, LoadOutcome::Ok { .. }), "{:?}", record.outcome);

    // And the supply-chain line says what verified it.
    let line = record.record.line();
    assert!(line.contains("pinned by"), "{line}");
    assert!(line.contains(common::INDEX_URL), "{line}");
}

#[test]
fn offline_uses_the_cache() {
    let mut fixture = common::Fixture::new();
    fixture.publish("buildgraph", "1.2.0", "read = [\"./**\"]", &["read(./**)"]);
    let source = Source::from_str("buildgraph@1.2.0").unwrap();
    let options = InstallOptions {
        force: true,
        allow_all: true,
        ..InstallOptions::to(Target::User)
    };

    fixture
        .installer()
        .install(&source, &options)
        .expect("the first install populates the cache");

    // Now take the mirror away entirely: an offline machine.
    std::fs::remove_dir_all(fixture.mirror_root()).unwrap();
    let again = fixture
        .installer()
        .install(&source, &options)
        .expect("a previously verified package installs with no fetcher behind it");
    assert!(again.is_pinned(), "the cached copy is re-verified, not trusted");

    // And a cache whose bytes no longer match the pin is not used as a
    // shortcut: it fails the same way a bad download would.
    let cached = fixture.tmp.path().join("cache/buildgraph/1.2.0/src/lib.rs");
    std::fs::write(&cached, "// tampered in the cache\n").unwrap();
    let err = fixture.installer().install(&source, &options).unwrap_err();
    assert!(
        matches!(err, RegistryError::Fetch { .. } | RegistryError::HashMismatch { .. }),
        "{err:?}"
    );
}

#[test]
fn defaults_to_the_user_layer() {
    // `orrery install x` with no flag lands in `~/.orrery/extensions/x/` and is
    // discovered from a *different* workspace — which is the whole reason the
    // default is the user layer.
    let mut fixture = common::Fixture::new();
    fixture.publish("buildgraph", "1.2.0", "read = [\"./**\"]", &["read(./**)"]);

    let record = fixture
        .installer()
        .install(
            &Source::from_str("buildgraph@1.2.0").unwrap(),
            &InstallOptions::default(),
        )
        .expect("an install with no target flag");

    assert_eq!(record.target, Target::User);
    assert_eq!(
        record.path,
        fixture.layout.user_dir.join("extensions/buildgraph")
    );

    // Discovery, from somewhere else entirely.
    let elsewhere = fixture.tmp.path().join("another-workspace");
    std::fs::create_dir_all(&elsewhere).unwrap();
    let manifest = discover(
        &[
            LayerRoot {
                layer: Layer::User,
                dir: fixture.layout.user_dir.clone(),
            },
            LayerRoot {
                layer: Layer::Workspace,
                dir: elsewhere.join(".orrery"),
            },
        ],
        &Provenanced::default(),
        &FsWalk,
    );
    assert!(
        manifest.has(ItemKind::Extension, "buildgraph"),
        "a user-layer install must be visible from any workspace: {manifest:?}"
    );
}

#[test]
fn workspace_flag() {
    let mut fixture = common::Fixture::new();
    fixture.publish("buildgraph", "1.2.0", "read = [\"./**\"]", &["read(./**)"]);
    let record = fixture
        .installer()
        .install(
            &Source::from_str("buildgraph@1.2.0").unwrap(),
            &InstallOptions::to(Target::Workspace),
        )
        .expect("a workspace install");
    assert_eq!(
        record.path,
        fixture
            .layout
            .workspace_root
            .join(".orrery/extensions/buildgraph")
    );
    assert!(
        !fixture
            .layout
            .user_dir
            .join("extensions/buildgraph")
            .exists(),
        "--workspace must not also write the user layer"
    );
}

#[test]
fn link_is_a_symlink_and_marked_development() {
    let fixture = common::Fixture::new();
    let dev = common::write_package(
        &fixture.tmp.path().join("dev"),
        "buildgraph",
        "0.0.0-dev",
        "read = [\"./**\"]",
    );
    let options = InstallOptions::to(Target::User).linked();
    let source = Source::Path { path: dev.clone() };

    // Creating a directory symlink needs Developer Mode or
    // SeCreateSymbolicLinkPrivilege on Windows. Either it links, or it says
    // exactly that — silently copying instead is the one wrong answer.
    if !install::symlinks_available(fixture.tmp.path()) {
        let err = fixture.installer().install(&source, &options).unwrap_err();
        let RegistryError::SymlinkUnavailable { message, .. } = &err else {
            panic!("expected SymlinkUnavailable, got {err:?}");
        };
        assert!(err.to_string().contains("Developer Mode"), "{message}");
        return;
    }

    let record = fixture
        .installer()
        .install(&source, &options)
        .expect("a development link");

    let meta = std::fs::symlink_metadata(&record.path).unwrap();
    assert!(meta.file_type().is_symlink(), "--link must link, not copy");
    assert!(record.pin.development, "a link is development");
    assert!(!record.is_pinned());
    assert_eq!(record.record.reason, Some(UnpinnedReason::Development));
    assert!(record.record.line().contains("[development]"), "{}", record.record.line());

    // It really is the developer's directory: a change shows through.
    std::fs::write(dev.join("src/lib.rs"), "// edited after installing\n").unwrap();
    let through = std::fs::read_to_string(record.path.join("src/lib.rs")).unwrap();
    assert!(through.contains("edited after installing"));

    // And removing the link does not delete the working copy.
    install::remove(&fixture.layout, "buildgraph", None).expect("removed");
    assert!(dev.join("src/lib.rs").exists(), "remove ate the working copy");
}

#[test]
fn remove_ambiguous_across_layers_asks() {
    let mut fixture = common::Fixture::new();
    fixture.publish("buildgraph", "1.2.0", "read = [\"./**\"]", &["read(./**)"]);
    let source = Source::from_str("buildgraph@1.2.0").unwrap();
    for target in [Target::User, Target::Workspace] {
        fixture
            .installer()
            .install(&source, &InstallOptions::to(target))
            .expect("installed at both layers");
    }

    let err = install::remove(&fixture.layout, "buildgraph", None).unwrap_err();
    let RegistryError::AmbiguousRemove { layers, .. } = &err else {
        panic!("expected AmbiguousRemove, got {err:?}");
    };
    assert!(layers.contains("user"), "{layers}");
    assert!(layers.contains("workspace"), "{layers}");

    // Naming one removes exactly that one.
    let gone = install::remove(&fixture.layout, "buildgraph", Some(Target::Workspace)).unwrap();
    assert!(!gone.exists());
    assert!(fixture.layout.dir_for(Target::User, "buildgraph").exists());

    // With one left, no question is needed.
    install::remove(&fixture.layout, "buildgraph", None).expect("unambiguous now");
    let err = install::remove(&fixture.layout, "buildgraph", None).unwrap_err();
    assert!(matches!(err, RegistryError::NotInstalled { .. }), "{err:?}");
}

#[test]
fn a_git_install_records_the_commit_it_resolved_to() {
    if !common::git_available() {
        eprintln!("no git on PATH; skipping");
        return;
    }
    let fixture = common::Fixture::new();
    let pkg = common::write_package(
        &fixture.tmp.path().join("src-repo"),
        "buildgraph",
        "1.2.0",
        "read = [\"./**\"]",
    );
    let (bare, sha) = common::bare_repo_with(fixture.tmp.path(), "buildgraph", &pkg, "v1.2");

    let record = fixture
        .installer()
        .install(
            &Source::Git {
                url: common::local_remote(&bare),
                reference: GitRef::Named("v1.2".to_owned()),
                shorthand: Some("github:o/r#v1.2".to_owned()),
            },
            &InstallOptions::to(Target::User).allowing_everything(),
        )
        .expect("a git install");

    assert_eq!(
        record.resolved.as_deref(),
        Some(sha.as_str()),
        "a mutable ref must still record the commit it became"
    );
    assert!(!record.is_pinned(), "git is not the verified path");
    assert!(record.path.join("orrery.toml").exists());
    assert!(
        !record.path.join(".git").exists(),
        "clone bookkeeping is not part of the installed extension"
    );
}

#[test]
fn an_upgrade_that_adds_a_capability_denies_it_by_default() {
    let mut fixture = common::Fixture::new();
    fixture.publish("buildgraph", "1.2.0", "read = [\"./**\"]", &["read(./**)"]);
    fixture
        .installer()
        .install(
            &Source::from_str("buildgraph@1.2.0").unwrap(),
            &InstallOptions::to(Target::User).allowing_everything(),
        )
        .unwrap();

    fixture.publish(
        "buildgraph",
        "1.3.0",
        "read = [\"./**\"]\nnet = [\"api.buildgraph.io\"]\n",
        &["read(./**)", "net(api.buildgraph.io)"],
    );
    let upgrade = fixture
        .installer()
        .install(
            &Source::from_str("buildgraph@1.3.0").unwrap(),
            // No answers at all: the non-interactive path.
            &InstallOptions {
                force: true,
                ..InstallOptions::to(Target::User)
            },
        )
        .expect("the upgrade installs");

    assert!(upgrade.diff.has_new(), "{:?}", upgrade.diff.rows);
    assert_eq!(upgrade.diff.new_rows()[0].text(), "net(api.buildgraph.io)");
    assert!(
        upgrade
            .approval
            .denied
            .iter()
            .any(|c| c.aspect == orrery_proto::Aspect::Net),
        "a new capability defaults to deny"
    );
    // Denying degrades rather than failing.
    assert!(
        matches!(upgrade.outcome, LoadOutcome::Degraded { .. }),
        "{:?}",
        upgrade.outcome
    );
}

#[test]
fn a_package_whose_manifest_over_asks_never_lands() {
    let mut fixture = common::Fixture::new();
    // The index was reviewed as `read`; the package asks for creds as well.
    fixture.publish(
        "sneaky",
        "1.0.0",
        "read = [\"./**\"]\ncreds = true\n",
        &["read(./**)"],
    );
    let err = fixture
        .installer()
        .install(
            &Source::from_str("sneaky@1.0.0").unwrap(),
            &InstallOptions::to(Target::User).allowing_everything(),
        )
        .unwrap_err();
    assert!(matches!(err, RegistryError::RequiresMismatch { .. }), "{err:?}");
    assert!(
        !fixture.layout.dir_for(Target::User, "sneaky").exists(),
        "a tampering signal must stop the install before anything is placed"
    );
}
