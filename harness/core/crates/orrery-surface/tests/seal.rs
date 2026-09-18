//! Surfaces seal at `turn.settled`. A patch for a settled surface is refused
//! at the kernel and reported to the extension, not sent to a client that
//! cannot apply it.

use orrery_proto::{Status, Surface, SurfaceId, SurfaceKind, SurfacePatch, TurnId};
use orrery_surface::{SurfaceError, SurfaceStore};

fn text(value: &str) -> Surface {
    Surface::new(SurfaceKind::Text {
        value: value.to_owned(),
        style: None,
    })
}

/// Emit, seal, emit again: the second emission comes back as an error **to the
/// extension**, and produces no frame.
#[test]
fn patch_after_settle_is_refused() {
    let mut store = SurfaceStore::new();
    let turn = TurnId::new();
    let id = SurfaceId::new();

    let patches = store
        .emit(turn, id, text("working"))
        .expect("the first emit");
    assert_eq!(
        patches.len(),
        1,
        "a new surface is one replace: {patches:?}"
    );
    assert!(matches!(patches[0], SurfacePatch::Replace { .. }));

    store.seal(turn);
    assert!(store.is_sealed(&turn));

    let refused = store.emit(turn, id, text("working, revised"));
    match refused {
        Err(SurfaceError::Sealed { turn: t, surface }) => {
            assert_eq!(t, turn);
            assert_eq!(surface, id);
        }
        other => panic!("a sealed turn refuses the patch, got {other:?}"),
    }

    // And nothing was produced: an `Err` is the whole answer, so there is no
    // frame for a transport to send.
    assert_eq!(
        store.get(&turn, &id).map(|s| s.kind.clone()),
        Some(SurfaceKind::Text {
            value: "working".into(),
            style: None
        }),
        "the stored surface is the one the client already has"
    );

    // Removing from a sealed turn is refused on the same rule.
    assert!(matches!(
        store.remove(turn, id),
        Err(SurfaceError::Sealed { .. })
    ));
}

/// An extension with something to add after its turn ends is not stuck: it
/// emits a new surface in the turn that is current now.
#[test]
fn new_surface_in_the_current_turn_is_fine() {
    let mut store = SurfaceStore::new();
    let first = TurnId::new();
    let id = SurfaceId::new();
    store
        .emit(first, id, text("the first answer"))
        .expect("emits");
    store.seal(first);

    let current = TurnId::new();
    let late = SurfaceId::new();
    let patches = store
        .emit(current, late, text("and one more thing"))
        .expect("a new surface in the current turn is fine");
    assert_eq!(patches.len(), 1);
    assert!(matches!(patches[0], SurfacePatch::Replace { .. }));

    // Including under an id the sealed turn already used: surfaces are keyed
    // per turn, so nothing collides.
    assert!(store.emit(current, id, text("re-said here")).is_ok());
    assert!(!store.is_sealed(&current));
}

/// Re-emitting the whole surface is what an extension does; the store is what
/// turns that into something small.
#[test]
fn re_emission_is_diffed_not_resent() {
    let mut store = SurfaceStore::new();
    let turn = TurnId::new();
    let id = SurfaceId::new();

    store
        .emit(turn, id, text("the workspace has"))
        .expect("emits");
    let patches = store
        .emit(turn, id, text("the workspace has three crates"))
        .expect("emits");
    assert_eq!(patches.len(), 1, "{patches:?}");
    let SurfacePatch::Append { text: added, .. } = &patches[0] else {
        panic!("a growing text is an append, got {:?}", patches[0]);
    };
    assert_eq!(added, " three crates");

    // The same surface twice is no traffic at all.
    let again = store
        .emit(turn, id, text("the workspace has three crates"))
        .expect("emits");
    assert!(again.is_empty(), "{again:?}");
}

/// A surface no renderer could draw never reaches the store.
#[test]
fn a_malformed_surface_is_refused_before_it_is_stored() {
    let mut store = SurfaceStore::new();
    let turn = TurnId::new();
    let id = SurfaceId::new();
    let bad = Surface::new(SurfaceKind::Custom {
        kind: "flamegraph".into(),
        payload: serde_json::json!({}),
        fallback: Box::new(text("the build took 41s, mostly in codegen")),
    });
    assert!(matches!(
        store.emit(turn, id, bad),
        Err(SurfaceError::Malformed(_))
    ));
    assert!(store.get(&turn, &id).is_none());
}

/// A status change is a set, not a new surface.
#[test]
fn a_status_change_is_one_set() {
    let mut store = SurfaceStore::new();
    let turn = TurnId::new();
    let id = SurfaceId::new();
    let mut running = text("reading");
    running.status = Some(Status::Running);
    store.emit(turn, id, running.clone()).expect("emits");

    let mut done = running;
    done.status = Some(Status::Done);
    let patches = store.emit(turn, id, done).expect("emits");
    assert_eq!(patches.len(), 1, "{patches:?}");
    let SurfacePatch::Set { path, .. } = &patches[0] else {
        panic!("expected a set, got {:?}", patches[0]);
    };
    assert_eq!(path, &["status".to_owned()]);
}
