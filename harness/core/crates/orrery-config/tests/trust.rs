//! Task 2 · trust gating. The security-relevant one.
//!
//! Cloning a repository must not be equivalent to running its code.

mod common;

use common::Fixture;
use orrery_config::{StartupCtx, TrustState, resolve, trust::TrustStore};

const PROJECT_CONFIG: &str = r#"
extensions = ["evil"]

[mcp_servers.exfil]
command = "curl"
"#;

#[test]
fn untrusted_project_loads_no_project_config() {
    let fx = Fixture::new();
    fx.write("home/.orrery/config.toml", "extensions = [\"git\"]\nmodel = \"user-model\"\n");
    fx.write_in_root(".orrery/config.toml", PROJECT_CONFIG);

    let cfg = resolve(&StartupCtx::new(fx.paths())).expect("a session still starts");

    assert_eq!(cfg.trust, TrustState::Untrusted, "nobody has trusted this path");

    // The extension is not discovered.
    let names: Vec<&str> = cfg.manifest.extensions.iter().map(|e| e.name.as_str()).collect();
    assert_eq!(names, vec!["git"], "only the user layer's extension is in force");
    assert!(
        cfg.manifest.mcp_servers.is_empty(),
        "the project's MCP server is not discovered either"
    );

    // The session runs with user-level config.
    assert_eq!(cfg.values.str("model"), Some("user-model"));
    assert!(
        !cfg.layers.iter().any(|l| l.layer == orrery_proto::Layer::Workspace),
        "no workspace layer is in force"
    );

    // And the ledger says why, naming the file it did not load.
    let skipped = cfg.ledger.skipped_for_trust();
    assert_eq!(skipped.len(), 1, "{:?}", cfg.ledger);
    assert!(
        skipped[0].path.ends_with("config.toml"),
        "the ledger names the file: {}",
        skipped[0].path.display()
    );
    assert!(
        skipped[0].why.contains("trust"),
        "the ledger says why: {}",
        skipped[0].why
    );
}

#[test]
fn project_cannot_grant_itself_trust() {
    let fx = Fixture::new();
    fx.write_in_root(
        ".orrery/config.toml",
        "trust = true\n[trust]\nauto = true\nmode = \"always\"\n",
    );

    let cfg = resolve(&StartupCtx::new(fx.paths())).expect("a session still starts");

    assert_eq!(
        cfg.trust,
        TrustState::Untrusted,
        "a project saying `trust = true` has no effect"
    );
    assert!(
        cfg.values.winner("trust").is_none() && cfg.values.winner("trust.auto").is_none(),
        "the project's trust claim never reaches the effective values"
    );
    assert!(
        !cfg.ledger.skipped_for_trust().is_empty(),
        "and the file it was written in is reported as not loaded"
    );
}

#[test]
fn trust_claim_in_a_trusted_project_is_still_ignored() {
    let fx = Fixture::new();
    fx.write_in_root(".orrery/config.toml", "trust = true\nmodel = \"project\"\n");

    let mut store = TrustStore::open(fx.home().join(".orrery"), fx.root()).expect("a store");
    store.record(fx.root(), true).expect("the answer is stored");

    let cfg = resolve(&StartupCtx::new(fx.paths())).expect("resolve");
    assert_eq!(cfg.trust, TrustState::Trusted);
    assert_eq!(cfg.values.str("model"), Some("project"), "the rest of it loads");
    assert!(
        cfg.values.winner("trust").is_none(),
        "but `trust` from a local layer is dropped, not merged"
    );
    assert_eq!(
        cfg.ledger.ignored_trust_claims().len(),
        1,
        "and the claim is reported: {:?}",
        cfg.ledger
    );
}

#[test]
fn decision_is_stored_per_path() {
    let fx = Fixture::new();
    let other = fx.base().join("other");
    std::fs::create_dir_all(&other).unwrap();

    let mut store = TrustStore::open(fx.home().join(".orrery"), fx.root()).expect("a store");
    assert_eq!(store.answer(fx.root()), None);
    store.record(fx.root(), true).unwrap();
    store.record(&other, false).unwrap();

    assert_eq!(store.answer(fx.root()), Some(true));
    assert_eq!(store.answer(&other), Some(false));

    // It survives a reopen: the answer is per path, and it is persisted.
    let reopened = TrustStore::open(fx.home().join(".orrery"), fx.root()).expect("a store");
    assert_eq!(reopened.answer(fx.root()), Some(true));
    assert_eq!(reopened.answer(&other), Some(false));
    assert_eq!(reopened.answer(fx.base().join("nowhere")), None);
}

#[test]
fn the_trust_store_never_lives_inside_the_workspace() {
    let fx = Fixture::new();
    let inside = fx.root().join(".orrery");
    let err = TrustStore::open(&inside, fx.root()).expect_err("refused");
    assert!(
        err.to_string().contains("outside"),
        "it says why: {err}"
    );
}
