//! The census describes a section, a table and a summary — and nothing else.

use orrery_ext_api::NativeExtension;
use orrery_ext_api::testing::load_path_for_test;
use orrery_proto::{Outcome, SurfaceKind};
use workspace_census::{MANIFEST, WorkspaceCensus};

fn input() -> serde_json::Value {
    serde_json::json!({
        "crates": [
            { "name": "orrery-proto", "area": "core", "published": true },
            { "name": "orrery-surface", "area": "core", "published": false },
            { "name": "orrery-ext-git", "area": "extension", "published": false }
        ]
    })
}

/// The manifest on disk is the one the code carries, parsed by the parser a
/// third party is held to.
#[test]
fn ships_a_manifest_a_third_party_would_ship() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("orrery.toml");
    let harness = load_path_for_test(&path, &[]).expect("the manifest parses");
    assert_eq!(harness.manifest().name.as_str(), "example-workspace-census");
    assert_eq!(
        harness.manifest().provides.tools,
        vec!["census".to_owned()],
        "the manifest promises the tool the code contributes"
    );
    assert_eq!(
        std::fs::read_to_string(&path)
            .unwrap()
            .replace("\r\n", "\n"),
        MANIFEST.replace("\r\n", "\n"),
        "the compiled-in manifest is the file, not a copy of it"
    );
    assert_eq!(
        WorkspaceCensus.tools().len(),
        1,
        "one tool, the one the manifest names"
    );
}

/// The whole output is one surface tree: a titled section holding a summary, a
/// table and a footer.
#[tokio::test]
async fn describes_a_section_a_table_and_a_summary() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("orrery.toml");
    let harness = load_path_for_test(&path, &[]).expect("the manifest parses");
    let ctx = harness.ctx("census");
    let outcome = WorkspaceCensus
        .call("census", input(), &ctx)
        .await
        .expect("the call is carried");

    let Outcome::Ok { surface, value } = outcome else {
        panic!("expected a surface, got {outcome:?}");
    };
    let surface = surface.expect("the census shows something");
    assert!(
        surface.id.is_some(),
        "the extension mints the id it will re-emit under"
    );

    let SurfaceKind::Stack {
        title, children, ..
    } = &surface.kind
    else {
        panic!("expected a section, got {:?}", surface.kind);
    };
    assert_eq!(title.as_deref(), Some("workspace census"));
    assert_eq!(children.len(), 3, "summary, table, footer");
    assert!(matches!(children[0].kind, SurfaceKind::Markdown { .. }));

    let SurfaceKind::Table { columns, rows } = &children[1].kind else {
        panic!("expected a table, got {:?}", children[1].kind);
    };
    assert_eq!(columns, &["crate", "area", "publish"]);
    assert_eq!(rows.len(), 3);
    assert_eq!(rows[0][0].text, "orrery-proto");
    assert_eq!(rows[0][2].text, "published");
    assert_eq!(rows[1][2].text, "private");

    assert_eq!(
        value.expect("the model gets counts")["published"],
        serde_json::json!(1),
        "what the model reads is not what the person sees"
    );
}

/// Every surface it describes goes through `ctx.ui`, which is the only way it
/// could reach a client at all.
#[tokio::test]
async fn everything_is_described_through_ctx_ui() {
    let path = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("orrery.toml");
    let harness = load_path_for_test(&path, &[]).expect("the manifest parses");
    let ctx = harness.ctx("census");
    let _ = WorkspaceCensus.call("census", input(), &ctx).await.unwrap();
    let described = harness.surfaces();
    assert!(
        described.len() >= 4,
        "three children and the section that holds them: {described:?}"
    );
    assert!(
        harness.recorded().is_empty(),
        "it asked the broker for nothing: drawing a table needs no capability"
    );
}

/// An unknown tool is a failure the harness carries, not a panic.
#[tokio::test]
async fn an_unknown_tool_fails_cleanly() {
    let ctx = orrery_ext_api::CallCtx::inert("example-workspace-census".parse().unwrap(), "nope");
    let outcome = WorkspaceCensus
        .call("nope", serde_json::json!({}), &ctx)
        .await
        .unwrap();
    assert!(matches!(outcome, Outcome::Failed { .. }), "{outcome:?}");
}
