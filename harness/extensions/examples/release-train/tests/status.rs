//! The release train: a task list, a progress bar, and one custom surface whose
//! fallback has to be worth reading.

use orrery_ext_api::NativeExtension;
use orrery_ext_api::testing::load_path_for_test;
use orrery_proto::{Outcome, Status, SurfaceKind};
use release_train::{MANIFEST, ReleaseTrain};

fn input(shipped: u64) -> serde_json::Value {
    let ratatui = if shipped > 2 { "done" } else { "running" };
    serde_json::json!({
        "release": "v0.25.0",
        "stages": [
            { "id": "schema", "label": "surface schema", "status": "done" },
            { "id": "differ", "label": "kernel differ", "status": "done" },
            { "id": "ratatui", "label": "ratatui client", "status": ratatui },
            { "id": "ink", "label": "ink client", "status": "pending" },
            { "id": "ade", "label": "ade client", "status": "pending" }
        ]
    })
}

#[test]
fn ships_a_manifest_a_third_party_would_ship() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("orrery.toml");
    let harness = load_path_for_test(&path, &[]).expect("the manifest parses");
    assert_eq!(harness.manifest().name.as_str(), "example-release-train");
    assert_eq!(harness.manifest().provides.tools, vec!["status".to_owned()]);
    assert_eq!(
        std::fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n"),
        MANIFEST.replace("\r\n", "\n")
    );
}

#[tokio::test]
async fn describes_a_task_list_a_progress_bar_and_a_custom_surface() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("orrery.toml");
    let harness = load_path_for_test(&path, &[]).expect("the manifest parses");
    let outcome = ReleaseTrain
        .call("status", input(2), &harness.ctx("status"))
        .await
        .expect("the call is carried");
    let Outcome::Ok { surface, .. } = outcome else {
        panic!("expected a surface, got {outcome:?}");
    };
    let surface = surface.expect("the train shows something");
    let SurfaceKind::Stack { children, .. } = &surface.kind else {
        panic!("expected a stack, got {:?}", surface.kind);
    };
    assert_eq!(children.len(), 3, "tasks, progress, timeline");

    let SurfaceKind::Task { items } = &children[0].kind else {
        panic!("expected a task list, got {:?}", children[0].kind);
    };
    assert_eq!(items.len(), 5);
    assert_eq!(items[0].status, Status::Done);
    assert_eq!(items[2].status, Status::Running);

    let SurfaceKind::Progress { label, done, total } = &children[1].kind else {
        panic!("expected progress, got {:?}", children[1].kind);
    };
    assert_eq!(label, "v0.25.0");
    assert_eq!((*done, *total), (Some(2), Some(5)));

    let SurfaceKind::Custom {
        kind,
        payload,
        fallback,
    } = &children[2].kind
    else {
        panic!("expected a custom surface, got {:?}", children[2].kind);
    };
    assert_eq!(kind, "example-release-train.timeline");
    assert_eq!(payload["release"], serde_json::json!("v0.25.0"));

    // 6.2: the fallback is not optional and it is not a shrug. Every stage the
    // payload carries is named in the thing a client without a renderer draws.
    let text = flatten(fallback);
    for stage in [
        "surface schema",
        "kernel differ",
        "ratatui client",
        "ink client",
        "ade client",
    ] {
        assert!(text.contains(stage), "the fallback drops `{stage}`: {text}");
    }
    assert!(
        text.contains("2 of 5"),
        "the fallback says where the train is: {text}"
    );
}

/// Re-emission is the whole API: the same surface id, a later state.
#[tokio::test]
async fn re_emits_under_the_same_id() {
    let ctx = orrery_ext_api::CallCtx::inert("example-release-train".parse().unwrap(), "status");
    let first = ReleaseTrain.call("status", input(2), &ctx).await.unwrap();
    let second = ReleaseTrain.call("status", input(3), &ctx).await.unwrap();
    let (Outcome::Ok { surface: a, .. }, Outcome::Ok { surface: b, .. }) = (first, second) else {
        panic!("both calls show something");
    };
    let (a, b) = (a.unwrap(), b.unwrap());
    assert_eq!(a.id, b.id, "the extension re-emits; the kernel diffs");
    assert_ne!(a.kind, b.kind, "and the second one has moved on");
}

/// Everything, flattened, the way a text-only client would show it.
fn flatten(surface: &orrery_proto::Surface) -> String {
    match &surface.kind {
        SurfaceKind::Text { value, .. } | SurfaceKind::Markdown { value, .. } => value.clone(),
        SurfaceKind::Table { rows, .. } => rows
            .iter()
            .map(|r| {
                r.iter()
                    .map(|c| c.text.as_str())
                    .collect::<Vec<_>>()
                    .join(" ")
            })
            .collect::<Vec<_>>()
            .join("\n"),
        SurfaceKind::Stack {
            title, children, ..
        } => {
            let mut out = title.clone().unwrap_or_default();
            for child in children {
                out.push('\n');
                out.push_str(&flatten(child));
            }
            out
        }
        other => format!("{other:?}"),
    }
}
