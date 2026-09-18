//! A review describes a diff and the question that goes with it.

use orrery_ext_api::NativeExtension;
use orrery_ext_api::testing::load_path_for_test;
use orrery_proto::{Outcome, SurfaceKind};
use patch_review::{MANIFEST, PatchReview};

fn input() -> serde_json::Value {
    serde_json::json!({
        "path": "harness/core/crates/orrery-surface/src/sink.rs",
        "hunks": [{
            "old_start": 28, "old_lines": 2, "new_start": 28, "new_lines": 2,
            "lines": [
                { "op": "context", "text": "use orrery_proto::Surface;" },
                { "op": "del", "text": "use orrery_ext_api::SurfaceSink;" },
                { "op": "add", "text": "use crate::ctx::SurfaceSink;" }
            ]
        }]
    })
}

#[test]
fn ships_a_manifest_a_third_party_would_ship() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("orrery.toml");
    let harness = load_path_for_test(&path, &[]).expect("the manifest parses");
    assert_eq!(harness.manifest().name.as_str(), "example-patch-review");
    assert_eq!(harness.manifest().provides.tools, vec!["review".to_owned()]);
    assert_eq!(
        std::fs::read_to_string(&path).unwrap().replace("\r\n", "\n"),
        MANIFEST.replace("\r\n", "\n")
    );
}

#[tokio::test]
async fn describes_a_diff_and_a_question() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("orrery.toml");
    let harness = load_path_for_test(&path, &[]).expect("the manifest parses");
    let outcome = PatchReview
        .call("review", input(), &harness.ctx("review"))
        .await
        .expect("the call is carried");
    let Outcome::Ok { surface, .. } = outcome else {
        panic!("expected a surface, got {outcome:?}");
    };
    let surface = surface.expect("a review shows something");
    assert!(surface.id.is_some(), "it mints the id it re-emits under");

    let SurfaceKind::Stack { children, .. } = &surface.kind else {
        panic!("expected a stack, got {:?}", surface.kind);
    };
    assert_eq!(children.len(), 2, "the diff, then the question");

    let SurfaceKind::Diff { path, hunks } = &children[0].kind else {
        panic!("expected a diff, got {:?}", children[0].kind);
    };
    assert_eq!(path, "harness/core/crates/orrery-surface/src/sink.rs");
    assert_eq!(hunks.len(), 1);
    assert_eq!(hunks[0].lines.len(), 3, "context, removed, added");

    let SurfaceKind::Question {
        prompt,
        choices,
        default,
        ..
    } = &children[1].kind
    else {
        panic!("expected a question, got {:?}", children[1].kind);
    };
    assert!(
        prompt.contains("sink.rs"),
        "the prompt names the file: {prompt}"
    );
    assert_eq!(choices.len(), 3, "apply, skip, explain");
    assert_eq!(
        default.as_deref(),
        Some("skip"),
        "unattended, a patch is not applied"
    );
}

/// A patch with no hunks is a failure with a message, not an empty diff nobody
/// can act on.
#[tokio::test]
async fn an_empty_patch_is_refused() {
    let ctx = orrery_ext_api::CallCtx::inert("example-patch-review".parse().unwrap(), "review");
    let outcome = PatchReview
        .call(
            "review",
            serde_json::json!({ "path": "a.rs", "hunks": [] }),
            &ctx,
        )
        .await
        .unwrap();
    match outcome {
        Outcome::Failed { code, .. } => assert_eq!(code, "empty-patch"),
        other => panic!("expected a failure, got {other:?}"),
    }
}
