//! Plan 15 task 5: the grant diff, as a surface every client can draw.

mod common;

use orrery_ext_api::ExtensionManifest;
use orrery_proto::{Aspect, Capability, ExtId, LoadOutcome, SurfaceKind};
use orrery_registry::{Decision, GrantDiff, RowState};

fn caps(items: &[(Aspect, &[&str])]) -> Vec<Capability> {
    items
        .iter()
        .map(|(aspect, scope)| Capability {
            aspect: *aspect,
            scope: scope.iter().map(|s| (*s).to_owned()).collect(),
        })
        .collect()
}

#[test]
fn new_capability_on_upgrade_is_flagged() {
    let before = caps(&[(Aspect::Read, &["$WORKSPACE/**"])]);
    let after = caps(&[
        (Aspect::Read, &["$WORKSPACE/**"]),
        (Aspect::Net, &["api.buildgraph.io"]),
    ]);
    let diff = GrantDiff::upgrade(
        ExtId::new("buildgraph").unwrap(),
        "1.2.0".parse().unwrap(),
        "1.3.0".parse().unwrap(),
        &after,
        &before,
    );

    assert!(diff.has_new());
    let new = diff.new_rows();
    assert_eq!(new.len(), 1, "{:?}", diff.rows);
    assert_eq!(new[0].text(), "net(api.buildgraph.io)");

    // It defaults to deny, and that is what an unanswered install applies.
    let defaults = diff.defaults();
    let net = defaults
        .iter()
        .find(|(k, _)| k == "net(api.buildgraph.io)")
        .unwrap();
    assert_eq!(net.1, Decision::Deny);

    let approval = diff.apply(&[]);
    assert_eq!(approval.granted, before, "only the old grant survives");
    assert_eq!(approval.denied, caps(&[(Aspect::Net, &["api.buildgraph.io"])]));
}

#[test]
fn unchanged_capabilities_are_shown_as_already_allowed() {
    let before = caps(&[(Aspect::Read, &["$WORKSPACE/**"])]);
    let diff = GrantDiff::upgrade(
        ExtId::new("buildgraph").unwrap(),
        "1.2.0".parse().unwrap(),
        "1.3.0".parse().unwrap(),
        &before,
        &before,
    );
    assert!(!diff.has_new());
    assert_eq!(diff.rows[0].state, RowState::AlreadyAllowed);
    // Nothing to ask, so an unanswered upgrade keeps what it had.
    assert_eq!(diff.apply(&[]).granted, before);
}

#[test]
fn renders_as_a_surface() {
    let diff = GrantDiff::upgrade(
        ExtId::new("buildgraph").unwrap(),
        "1.2.0".parse().unwrap(),
        "1.3.0".parse().unwrap(),
        &caps(&[
            (Aspect::Read, &["$WORKSPACE/**"]),
            (Aspect::Net, &["api.buildgraph.io"]),
        ]),
        &caps(&[(Aspect::Read, &["$WORKSPACE/**"])]),
    );
    let surface = diff.surface();
    surface.validate().expect("a valid surface");

    let SurfaceKind::Stack { children, .. } = &surface.kind else {
        panic!("expected a stack, got {:?}", surface.kind);
    };

    let questions: Vec<&SurfaceKind> = children
        .iter()
        .map(|c| &c.kind)
        .filter(|k| matches!(k, SurfaceKind::Question { .. }))
        .collect();
    assert_eq!(questions.len(), 1, "only the new row is asked about");

    let SurfaceKind::Question {
        prompt, default, ..
    } = questions[0]
    else {
        unreachable!()
    };
    assert!(prompt.contains("NEW"), "a new row must be distinct: {prompt}");
    assert!(prompt.starts_with('+'), "{prompt}");
    assert_eq!(default.as_deref(), Some("deny"));

    // The heading names both versions, so a client renders the change and not
    // just the request.
    assert!(diff.title().contains("1.2.0"), "{}", diff.title());
    assert!(diff.title().contains("1.3.0"), "{}", diff.title());
}

#[test]
fn a_fresh_install_asks_about_everything() {
    let diff = GrantDiff::fresh(
        ExtId::new("buildgraph").unwrap(),
        "1.2.0".parse().unwrap(),
        &caps(&[(Aspect::Read, &["$WORKSPACE/**"]), (Aspect::Spawn, &["java"])]),
    );
    assert!(!diff.has_new(), "nothing is *new* on a first install");
    assert_eq!(diff.rows.len(), 2);
    assert!(diff.title().ends_with("requests:"), "{}", diff.title());

    let SurfaceKind::Stack { children, .. } = &diff.surface().kind else {
        panic!()
    };
    let questions = children
        .iter()
        .filter(|c| matches!(c.kind, SurfaceKind::Question { .. }))
        .count();
    assert_eq!(questions, 2);
}

#[test]
fn deny_degrades_not_fails() {
    let tmp = tempfile::tempdir().unwrap();
    let pkg = common::write_package(
        tmp.path(),
        "buildgraph",
        "1.2.0",
        "read = [\"$WORKSPACE/**\"]\nspawn = [\"java\"]\n",
    );
    let manifest = ExtensionManifest::from_path(pkg.join("orrery.toml")).unwrap();
    let diff = GrantDiff::fresh(
        manifest.name.clone(),
        manifest.version.clone(),
        &manifest.capabilities(),
    );

    // Allow the read, deny the spawn.
    let answers = vec![
        ("read($WORKSPACE/**)".to_owned(), Decision::Allow),
        ("spawn(java)".to_owned(), Decision::Deny),
    ];
    let approval = diff.apply(&answers);
    let outcome = approval.outcome(&manifest, 3);

    let LoadOutcome::Degraded {
        problems,
        contributions,
        ..
    } = &outcome
    else {
        panic!("a denial must degrade, not fail: {outcome:?}");
    };
    assert!(
        problems.iter().any(|p| p.contains("spawn")),
        "the ledger must say what was lost: {problems:?}"
    );
    assert!(
        !contributions.is_empty(),
        "the rest of the extension still works"
    );

    // And allowing everything is a clean load, so `degraded` is about the
    // denial and not about the fixture.
    let all = diff.apply(
        &diff
            .rows
            .iter()
            .map(|r| (r.field(), Decision::Allow))
            .collect::<Vec<_>>(),
    );
    assert!(matches!(all.outcome(&manifest, 3), LoadOutcome::Ok { .. }));
}
