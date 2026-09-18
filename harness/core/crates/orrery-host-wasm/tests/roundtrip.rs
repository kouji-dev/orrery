//! The arena across a real boundary, and the seam between the two copies of it.
//!
//! `orrery-wit` owns the arena and must not depend on `wasmtime`, so the
//! generated bindings are a second, structurally identical set of types.
//! `imports::from_wit` / `to_wit` is the seam; this is what keeps it honest.

mod common;

use std::sync::Arc;

use orrery_host_wasm::imports::{from_wit, to_wit};
use orrery_host_wasm::{Ceilings, Outcome, WasmHost};
use orrery_proto::surface::{Surface, SurfaceKind};
use orrery_wit::arena::flatten;

async fn call(tool: &str) -> Outcome {
    let host = WasmHost::new().expect("the engine builds");
    let component = host.compile(common::probe()).expect("the probe loads");
    host.call(
        &component,
        Ceilings::DEFAULT,
        Arc::new(common::Fake::denying()),
        host.cancel_handle(),
        tool,
        "",
    )
    .await
    .expect("the call runs")
}

/// A guest that built a table by hand, with no SDK, produces a `Surface` the
/// kernel recognises.
#[tokio::test(flavor = "multi_thread")]
async fn a_guest_built_table_arrives_as_a_surface() {
    let Outcome::Ok(surface) = call("table").await else {
        panic!("expected a surface");
    };
    match &surface.kind {
        SurfaceKind::Table { columns, rows } => {
            assert_eq!(columns, &["module", "reason"]);
            assert_eq!(rows.len(), 1);
            assert_eq!(rows[0][0].text, "core");
            assert_eq!(rows[0][1].text, "changed");
        }
        other => panic!("expected a table: {other:?}"),
    }
}

/// The recursive case, which is the whole reason the arena exists.
#[tokio::test(flavor = "multi_thread")]
async fn a_guest_built_stack_rebuilds_into_a_tree() {
    let Outcome::Ok(surface) = call("stack").await else {
        panic!("expected a surface");
    };
    match &surface.kind {
        SurfaceKind::Stack { children, .. } => {
            assert_eq!(children.len(), 2);
            let texts: Vec<&str> = children
                .iter()
                .map(|c| match &c.kind {
                    SurfaceKind::Text { value, .. } => value.as_str(),
                    other => panic!("expected text: {other:?}"),
                })
                .collect();
            assert_eq!(texts, ["one", "two"]);
        }
        other => panic!("expected a stack: {other:?}"),
    }
}

/// The seam, both directions, over a surface with every recursive shape in it.
#[test]
fn the_two_copies_of_the_arena_do_not_drift() {
    let surface = Surface::new(SurfaceKind::Stack {
        dir: orrery_proto::surface::StackDir::Column,
        title: Some("both shapes".into()),
        collapsed: false,
        children: vec![
            Surface::new(SurfaceKind::Markdown {
                value: "# hi".into(),
                complete: true,
            }),
            Surface::new(SurfaceKind::Custom {
                kind: "demo.thing".into(),
                payload: serde_json::json!({ "n": 3 }),
                fallback: Box::new(Surface::new(SurfaceKind::Text {
                    value: "a thing".into(),
                    style: None,
                })),
            }),
        ],
    });

    let ours = flatten(&surface);
    let theirs = to_wit(ours.clone());
    let back = from_wit(theirs);
    assert_eq!(back, ours, "the seam lost or reordered something");
    assert_eq!(
        orrery_wit::arena::rebuild(&back).expect("it rebuilds"),
        surface
    );
}
