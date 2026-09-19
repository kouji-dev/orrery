//! The floor ships as an extension, and loads like any other.
//!
//! It loads through [`orrery_ext_api::testing::load_for_test`] — the published
//! mock-broker harness `orrery ext test` runs, and the one plan 18 tells a
//! community author to use. Loading the floor through it is the proof that the
//! path a third party is pointed at actually works: an extension's own tests
//! never reach for the host.

use orrery_ext_api::testing::load_for_test;
use orrery_ext_api::{LoopEvent, Placement};
use orrery_ext_views_default::{DefaultViews, MANIFEST};
use orrery_proto::{
    Contribution, ContributionKind, ErrorDetail, ErrorScope, Event, ExtId, LoadOutcome, Seq,
    SurfaceKind,
};

fn id() -> ExtId {
    ExtId::new("views-default").expect("a valid ext id")
}

/// The bundle loads through the published test harness and appears in the
/// ledger entry a real session would show.
///
/// Not "the kernel has a floor": the kernel has nothing, and this extension
/// puts it there through the same door a third-party bundle uses.
#[test]
fn ships_as_an_extension() {
    // It asks for nothing, so no grants at all are enough and the load is
    // clean rather than degraded.
    let harness = load_for_test(MANIFEST, &[])
        .expect("its own orrery.toml parses with the parser a third party is held to");

    assert_eq!(harness.manifest().name, id());

    let outcome = harness.load_outcome();
    let LoadOutcome::Ok { contributions, .. } = &outcome else {
        panic!("a view bundle needs no capabilities, so it loads clean: {outcome:?}");
    };
    for kind in orrery_ext_api::floor_kinds() {
        assert!(
            contributions.contains(&Contribution {
                kind: ContributionKind::View,
                name: kind.to_string(),
            }),
            "`{kind}` is contributed: {contributions:?}"
        );
    }

    // And the names in that entry are the floor, in order.
    let views: Vec<String> = contributions
        .iter()
        .filter(|c| c.kind == ContributionKind::View)
        .map(|c| c.name.clone())
        .collect();
    assert_eq!(views, DefaultViews::promised());

    // Nothing was asked of the broker: a view bundle is pure rendering data.
    assert!(harness.recorded().is_empty());
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

/// **A login prompt is drawn from data, not from a scraped string.**
///
/// `AuthState` gained `Pending { user_code, verification_uri, … }` — everything
/// a client needs to draw a device-code wait — and nothing rendered it. The
/// only consumer was `orrery-kernel`'s refusal *sentence*, so the one flow that
/// most needs a real surface was the one flow reduced to prose. This is the
/// binding, and the payload below is `AuthState`'s own serialisation: the shape
/// is pinned on the other side by `orrery-provider`'s
/// `the_tagged_shape_is_the_contract`.
#[test]
fn a_device_code_login_renders_as_a_surface() {
    let registry = DefaultViews.registry();

    let placed = registry.render(&LoopEvent::Other {
        kind: orrery_ext_api::EventKind::AUTH_STATE.into(),
        payload: serde_json::json!({
            "state": "pending",
            "userCode": "WDJB-MJHT",
            "verificationUri": "https://example.test/device",
            "verificationUriComplete": "https://example.test/device?code=WDJB-MJHT",
            "expiresAt": 1_800,
            "intervalSecs": 5,
        }),
    });
    assert_eq!(placed.len(), 1, "the floor binds `auth.state`");
    let drawn = format!("{:?}", placed[0].surface);
    assert!(drawn.contains("WDJB-MJHT"), "the code to type: {drawn}");
    assert!(
        drawn.contains("https://example.test/device"),
        "where to type it: {drawn}"
    );
    assert!(
        drawn.contains("WDJB-MJHT") && drawn.contains("?code="),
        "the pre-filled link too, because the two can be opened on different \
         devices: {drawn}"
    );
}

/// `NeedsLogin` is the other state a client must not be left guessing at.
#[test]
fn needing_a_login_renders_its_reason() {
    let registry = DefaultViews.registry();
    let placed = registry.render(&LoopEvent::Other {
        kind: orrery_ext_api::EventKind::AUTH_STATE.into(),
        payload: serde_json::json!({
            "state": "needs-login",
            "reason": "no credential is configured for `anthropic`",
        }),
    });
    assert_eq!(placed.len(), 1);
    let SurfaceKind::Text { value, .. } = &placed[0].surface.kind else {
        panic!("a refusal is text: {:?}", placed[0].surface.kind);
    };
    assert!(value.contains("no credential is configured"), "{value}");
}

/// A state this build has not been taught to draw still says *something*,
/// rather than rendering an empty box or panicking inside a renderer.
#[test]
fn an_unknown_auth_state_still_says_something() {
    let registry = DefaultViews.registry();
    let placed = registry.render(&LoopEvent::Other {
        kind: orrery_ext_api::EventKind::AUTH_STATE.into(),
        payload: serde_json::json!({ "state": "quantum-entangled" }),
    });
    assert_eq!(placed.len(), 1);
    let SurfaceKind::Text { value, .. } = &placed[0].surface.kind else {
        panic!("still text");
    };
    assert!(value.contains("quantum-entangled"), "{value}");
}
