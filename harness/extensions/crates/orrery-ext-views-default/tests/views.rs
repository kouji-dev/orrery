//! The floor ships as an extension, and loads like any other.

use std::sync::Arc;

use orrery_ext_views_default::DefaultViews;
use orrery_host::{ExtensionTable, NativeHost, NativeRegistry};
use orrery_proto::{
    Contribution, ContributionKind, ErrorDetail, ErrorScope, Event, ExtId, Grant, Layer,
    LoadOutcome, Seq, SurfaceKind,
};
use orrery_surface::{LoopEvent, Placement};

fn id() -> ExtId {
    ExtId::new("views-default").expect("a valid ext id")
}

/// The bundle loads through `orrery-host` and appears in the ledger.
///
/// Not "the kernel has a floor": the kernel has nothing, and this extension
/// puts it there through the same door a third-party bundle uses.
#[tokio::test]
async fn ships_as_an_extension() {
    let mut natives = NativeRegistry::new();
    natives.register(Arc::new(DefaultViews));
    assert!(
        natives.broken().is_empty(),
        "its own orrery.toml parses with the parser a third party is held to: {:?}",
        natives.broken()
    );

    let host = Arc::new(NativeHost::new(natives));
    let manifest = host.manifest_of(&id()).expect("the manifest is registered");
    assert_eq!(manifest.name, id());

    let table = ExtensionTable::new();
    let outcome = table
        .load(host.clone(), manifest, Layer::Managed, Grant::nothing())
        .await;

    // It asks for nothing, so `Grant::nothing()` is enough and the load is
    // clean rather than degraded.
    let LoadOutcome::Ok { contributions, .. } = &outcome else {
        panic!("a view bundle needs no capabilities, so it loads clean: {outcome:?}");
    };
    for kind in orrery_surface::floor_kinds() {
        assert!(
            contributions.contains(&Contribution {
                kind: ContributionKind::View,
                name: kind.to_string(),
            }),
            "`{kind}` is contributed: {contributions:?}"
        );
    }

    // And the ledger is where a person goes to see it.
    let recorded = table.ledger().of(&id()).expect("the ledger has it");
    assert!(matches!(recorded, LoadOutcome::Ok { .. }));
    let views: Vec<String> = table
        .ledger()
        .contributions()
        .into_iter()
        .filter(|(ext, c)| ext == &id() && c.kind == ContributionKind::View)
        .map(|(_, c)| c.name)
        .collect();
    assert_eq!(views, DefaultViews::promised());
}

/// What it loads is what it renders. The extension is not a manifest with
/// nothing behind it.
#[test]
fn the_loaded_bundle_renders_the_floor() {
    let registry = DefaultViews.registry();

    let placed = registry.render(&LoopEvent::AssistantText {
        text: "the workspace has three crates".into(),
        complete: false,
    });
    assert_eq!(placed.len(), 1);
    assert_eq!(placed[0].placement, Placement::Inline);
    assert!(
        matches!(
            placed[0].surface.kind,
            SurfaceKind::Markdown {
                complete: false,
                ..
            }
        ),
        "streaming prose is markdown that says it is not finished yet"
    );

    let placed = registry.render(&LoopEvent::Frame(Box::new(Event::Error {
        seq: Seq(7),
        scope: ErrorScope::Session,
        detail: ErrorDetail {
            code: "ext.crashed".into(),
            message: "the guest went away".into(),
            retryable: false,
            data: None,
        },
    })));
    assert_eq!(placed.len(), 1);
    let SurfaceKind::Text { value, .. } = &placed[0].surface.kind else {
        panic!("an error is text");
    };
    assert!(value.contains("ext.crashed"), "{value}");

    // An event outside the floor is still nobody's business until something
    // binds it. The floor is a floor, not the whole building.
    assert!(
        registry
            .render(&LoopEvent::Other {
                kind: "router.decided".into(),
                payload: serde_json::json!({ "model": "fixture" }),
            })
            .is_empty()
    );
}
