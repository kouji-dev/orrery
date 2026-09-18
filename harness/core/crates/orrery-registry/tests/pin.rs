//! Plan 15 tasks 6 and 10: pinning, managed enforcement, and the rule that
//! keeps the ergonomics honest — an unpinned source is *visibly* unpinned.

mod common;

use std::path::PathBuf;
use std::str::FromStr;

use orrery_proto::{ExtId, LoadOutcome, SkipReason};
use orrery_registry::pin::{decide, effective_unpinned, resolve_entry};
use orrery_registry::{
    InstallOptions, ManagedRegistry, RegistryError, Source, SupplyChainLedger, SupplyChainRecord,
    Target, Unpinned, UnpinnedReason,
};

fn managed(unpinned: &str) -> ManagedRegistry {
    ManagedRegistry::from_toml(
        &format!(
            "[registry]\nindex = \"{}\"\nunpinned = \"{unpinned}\"\n",
            common::INDEX_URL
        ),
        PathBuf::from("/etc/orrery/managed.toml"),
    )
    .unwrap()
    .unwrap()
}

#[test]
fn unpinned_refuses_under_managed() {
    // The phase-8 criterion. An extension that is not in the index is refused,
    // the reason and the index URL are recorded, and a user-layer setting
    // cannot override it.
    let m = managed("refuse");
    let source = Source::from_str("github:owner/repo").unwrap();

    let err = decide(&source, Some(&m), None, false).unwrap_err();
    let RegistryError::Unpinned { file, index, .. } = &err else {
        panic!("expected Unpinned, got {err:?}");
    };
    assert!(file.contains("managed.toml"), "{file}");
    assert_eq!(index, common::INDEX_URL);

    // A user layer that says `allow` changes nothing, and is not even
    // consulted: that is what "not user-overridable" means.
    let err = decide(&source, Some(&m), Some(Unpinned::Allow), false).unwrap_err();
    assert!(matches!(err, RegistryError::Unpinned { .. }), "{err:?}");
    assert_eq!(effective_unpinned(Some(&m), Some(Unpinned::Allow)), Unpinned::Refuse);

    // And the refusal is a supply-chain event with the reason and the index in
    // it, not a silent absence.
    let record = SupplyChainRecord {
        ext: ExtId::new("repo").unwrap(),
        source: source.label(),
        pinned: false,
        development: false,
        rule: None,
        reason: Some(UnpinnedReason::Unsigned),
        index: Some(common::INDEX_URL.to_owned()),
        refused: Some(err.to_string()),
    };
    let line = record.line();
    assert!(line.contains("UNPINNED"), "{line}");
    assert!(line.contains("unsigned"), "{line}");
    assert!(line.contains(common::INDEX_URL), "{line}");
    assert!(matches!(
        record.load_outcome(),
        LoadOutcome::Skipped {
            reason: SkipReason::PolicyDenied,
            ..
        }
    ));
}

#[test]
fn warn_mode_loads_with_a_ledger_warning() {
    let m = managed("warn");
    let source = Source::from_str("github:owner/repo").unwrap();
    let decision = decide(&source, Some(&m), None, false).expect("warn installs");
    assert!(!decision.pinned);
    let warning = decision.warning.expect("warn mode must be loud");
    assert!(warning.contains("unpinned"), "{warning}");
    assert!(warning.contains("github:owner/repo"), "{warning}");
}

#[test]
fn version_set_is_exact() {
    // A pinned 1.2.0 refuses 1.2.1: a pin is a version, not a range.
    let mut fixture = common::Fixture::new();
    fixture.publish("buildgraph", "1.2.0", "read = [\"./**\"]", &["read(./**)"]);
    let asked = Source::from_str("buildgraph@1.2.1").unwrap();
    let err = resolve_entry(&fixture.index, &asked, common::INDEX_URL).unwrap_err();
    let RegistryError::VersionNotInIndex { available, .. } = &err else {
        panic!("expected VersionNotInIndex, got {err:?}");
    };
    assert_eq!(available, "1.2.0");
}

#[test]
fn non_registry_source_is_recorded_unpinned() {
    let mut fixture = common::Fixture::new();
    let pkg = fixture.publish("signed", "1.0.0", "read = [\"./**\"]", &["read(./**)"]);
    let loose = common::write_package(
        &fixture.tmp.path().join("loose"),
        "loose",
        "0.1.0",
        "read = [\"./**\"]",
    );
    let _ = pkg;

    let mut ledger = SupplyChainLedger::default();

    let from_registry = fixture
        .installer()
        .install(
            &Source::from_str("signed@1.0.0").unwrap(),
            &InstallOptions::to(Target::User).allowing_everything(),
        )
        .expect("a registry install");
    assert!(from_registry.is_pinned());
    let rule = from_registry.pin.rule.clone().expect("a pinned install names its rule");
    assert!(rule.contains("org-2026"), "{rule}");
    ledger.record(from_registry.record.clone());

    let from_path = fixture
        .installer()
        .install(
            &Source::Path { path: loose },
            &InstallOptions::to(Target::User).allowing_everything(),
        )
        .expect("an unpinned install, under the developer default");
    assert!(!from_path.is_pinned());
    assert_eq!(
        from_path.record.reason,
        Some(UnpinnedReason::NotFromTheRegistry)
    );
    ledger.record(from_path.record.clone());

    let unpinned = ledger.unpinned();
    assert_eq!(unpinned.len(), 1, "{:?}", ledger.all());
    assert_eq!(unpinned[0].ext.as_str(), "loose");
    assert!(unpinned[0].line().contains("UNPINNED"), "{}", unpinned[0].line());
    assert!(
        ledger.of("signed").unwrap().line().contains("pinned by"),
        "{}",
        ledger.of("signed").unwrap().line()
    );
}

#[test]
fn managed_refuse_blocks_every_non_registry_source() {
    let m = managed("refuse");
    let sources = [
        "github:owner/repo#v1",
        "https://git.corp.internal/x/repo.git",
        "crate:orrery-ext-x",
        "npm:@scope/orrery-ext-x",
        "./local",
    ];
    for raw in sources {
        let source = Source::from_str(raw).unwrap();
        let err = decide(&source, Some(&m), Some(Unpinned::Allow), false).unwrap_err();
        let text = err.to_string();
        assert!(text.contains("managed.toml"), "`{raw}`: {text}");
        assert!(
            text.contains("cannot override"),
            "`{raw}` must say the user layer cannot override it: {text}"
        );
    }
    // The registry itself still installs, which is the whole point of the
    // switch: it turns the other six off, not everything.
    let ok = decide(
        &Source::from_str("buildgraph@1.2.0").unwrap(),
        Some(&m),
        None,
        false,
    )
    .expect("the registry is the one allowed path");
    assert!(ok.pinned);
}

#[test]
fn warn_mode_installs_loudly() {
    let mut fixture = common::Fixture::new();
    fixture.managed("warn");
    let loose = common::write_package(
        &fixture.tmp.path().join("loose"),
        "loose",
        "0.1.0",
        "read = [\"./**\"]",
    );
    let record = fixture
        .installer()
        .install(
            &Source::Path { path: loose },
            &InstallOptions::to(Target::User).allowing_everything(),
        )
        .expect("warn installs");
    assert!(!record.is_pinned());
    assert!(record.pin.warning.is_some(), "warn mode must say so");
    assert!(record.path.join("orrery.toml").exists(), "it really installed");
}

#[test]
fn link_is_always_unpinned() {
    // Even under `allow`, and even though a local path install is already
    // unpinned: a symlink is marked `Development` because the code can change
    // under the harness between one run and the next.
    let source = Source::from_str("./x").unwrap();
    let allowed = decide(&source, None, Some(Unpinned::Allow), true).unwrap();
    assert!(!allowed.pinned);
    assert!(allowed.development);
    assert_eq!(allowed.reason, Some(UnpinnedReason::Development));

    // And a registry source with `--link` is not promoted to pinned either.
    let registry = Source::from_str("buildgraph@1.2.0").unwrap();
    let linked = decide(&registry, None, Some(Unpinned::Allow), true).unwrap();
    assert!(!linked.pinned, "--link cannot be pinned");
    assert_eq!(linked.reason, Some(UnpinnedReason::Development));
}

#[test]
fn the_default_is_allow_and_the_user_can_tighten_it() {
    assert_eq!(effective_unpinned(None, None), Unpinned::Allow);
    assert_eq!(
        effective_unpinned(None, Some(Unpinned::Refuse)),
        Unpinned::Refuse,
        "with no managed layer, a user may still refuse unpinned sources"
    );
}

#[test]
fn a_nonsense_unpinned_value_is_a_syntax_error() {
    let err = ManagedRegistry::from_toml(
        "[registry]\nunpinned = \"sometimes\"\n",
        PathBuf::from("managed.toml"),
    )
    .unwrap_err();
    assert!(err.to_string().contains("refuse, warn, allow"), "{err}");
}
