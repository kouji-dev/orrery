//! `seq`, replay and the per-client coalescer — the three things that decide
//! whether two clients on one session see the same run.

use std::collections::BTreeMap;
use std::time::Duration;

use orrery_agui::{AguiEvent, Frame, PatchOp};
use orrery_proto::{Event, Seq, Surface, SurfaceId, SurfaceKind, SurfacePatch, TurnId};
use orrery_transport::{Coalescer, Hub, ReplayError, ReplayRing};

fn sid(n: u8) -> SurfaceId {
    SurfaceId::from_uuid(uuid::Uuid::from_u128(u128::from(n)))
}

fn turn() -> TurnId {
    TurnId::from_uuid(uuid::Uuid::from_u128(0xff))
}

fn open_markdown(seq: u64, id: SurfaceId) -> Event {
    Event::Delta {
        seq: Seq(seq),
        surface: id,
        patch: SurfacePatch::Replace {
            id,
            value: Surface {
                id: Some(id),
                status: None,
                kind: SurfaceKind::Markdown {
                    value: String::new(),
                    complete: false,
                },
            },
        },
    }
}

fn append(seq: u64, id: SurfaceId, text: &str) -> Event {
    Event::Delta {
        seq: Seq(seq),
        surface: id,
        patch: SurfacePatch::Append {
            id,
            text: text.to_owned(),
        },
    }
}

/// What a client ends up believing, reduced to the one thing every case here
/// asserts: the text of each message.
fn fold(frames: &[Frame]) -> BTreeMap<String, String> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    for f in frames {
        match &f.event {
            AguiEvent::TextMessageStart { message_id, .. } => {
                out.entry(message_id.clone()).or_default();
            }
            AguiEvent::TextMessageContent { message_id, delta } => {
                out.entry(message_id.clone()).or_default().push_str(delta);
            }
            AguiEvent::StateDelta { delta } => {
                for op in delta {
                    if let PatchOp::Append { path, value } = op {
                        out.entry(path.clone()).or_default().push_str(value);
                    }
                }
            }
            _ => {}
        }
    }
    out
}

/// Two connected clients see identical `seq` values for the same event.
///
/// `seq` is assigned once, per session, at the differ's output. If it were
/// assigned per connection, a client that re-attached after talking to a second
/// client would ask for a `since` that meant something else.
#[tokio::test]
async fn seq_is_assigned_once() {
    let hub = Hub::new("sess-1");
    let mut fast = hub.subscribe(Duration::ZERO);
    let mut slow = hub.subscribe(Duration::from_millis(33));

    hub.publish(&Event::TurnStarted {
        seq: Seq(1),
        turn: turn(),
    });
    hub.publish(&open_markdown(2, sid(1)));
    hub.publish(&append(3, sid(1), "hello"));

    let a: Vec<u64> = drain(&mut fast).await.iter().map(|f| f.seq).collect();
    let b: Vec<u64> = drain(&mut slow).await.iter().map(|f| f.seq).collect();
    assert_eq!(a, vec![1, 2, 3]);
    assert_eq!(b, vec![1, 2, 3]);
}

async fn drain(c: &mut orrery_transport::ClientStream) -> Vec<Frame> {
    let mut out = Vec::new();
    while let Some(batch) = c.next_batch().await {
        out.extend(batch);
        if c.is_idle() {
            break;
        }
    }
    out
}

/// 100 single-character appends in one tick become one append.
#[test]
fn coalesce_merges_appends() {
    let mut c = Coalescer::new(Duration::from_millis(33));
    for (i, ch) in "x".repeat(100).chars().enumerate() {
        c.push(Frame::new(
            i as u64 + 1,
            AguiEvent::TextMessageContent {
                message_id: "msg-1".into(),
                delta: ch.to_string(),
            },
        ));
    }
    let out = c.drain();
    assert_eq!(out.len(), 1, "100 appends should coalesce to one");
    assert_eq!(out[0].seq, 100, "the merged frame carries the last seq");
    assert_eq!(
        out[0].merged_from,
        Some(1),
        "and the first, so a client can tell a merge from a gap"
    );
    let AguiEvent::TextMessageContent { delta, .. } = &out[0].event else {
        panic!("expected TEXT_MESSAGE_CONTENT");
    };
    assert_eq!(delta.len(), 100);
}

/// A `Set` followed by a `Replace` of the whole surface collapses to the
/// `Replace`: the field write is already inside it.
#[test]
fn coalesce_collapses_to_last_replace() {
    let mut c = Coalescer::new(Duration::from_millis(33));
    c.push(Frame::new(
        1,
        AguiEvent::StateDelta {
            delta: vec![PatchOp::Replace {
                path: "/surfaces/a/kind/rows".into(),
                value: serde_json::json!([1]),
            }],
        },
    ));
    c.push(Frame::new(
        2,
        AguiEvent::StateDelta {
            delta: vec![PatchOp::Replace {
                path: "/surfaces/a".into(),
                value: serde_json::json!({"id": "a"}),
            }],
        },
    ));
    let out = c.drain();
    assert_eq!(out.len(), 1);
    let AguiEvent::StateDelta { delta } = &out[0].event else {
        panic!("expected STATE_DELTA");
    };
    assert_eq!(
        delta,
        &vec![PatchOp::Replace {
            path: "/surfaces/a".into(),
            value: serde_json::json!({"id": "a"}),
        }]
    );
}

/// A slow client and a fast client on one session. The fast one gets
/// fine-grained frames, the slow one gets merged ones, **and both end at the
/// same final state.** This is the test that matters.
#[tokio::test]
async fn coalesce_per_client() {
    let hub = Hub::new("sess-1");
    let mut fast = hub.subscribe(Duration::ZERO);
    let mut slow = hub.subscribe(Duration::from_millis(40));

    hub.publish(&open_markdown(1, sid(1)));
    for (i, ch) in "the quick brown fox".chars().enumerate() {
        hub.publish(&append(i as u64 + 2, sid(1), &ch.to_string()));
    }

    let fast_frames = drain(&mut fast).await;
    let slow_frames = drain(&mut slow).await;

    assert!(
        fast_frames.len() > slow_frames.len(),
        "the fast client should see finer frames: {} vs {}",
        fast_frames.len(),
        slow_frames.len()
    );
    assert_eq!(
        fold(&fast_frames),
        fold(&slow_frames),
        "both clients must end at the same state"
    );
}

/// Attach with `since = 5`, get 6..n with no gaps.
#[test]
fn replay_attach_since_is_contiguous() {
    let mut ring = ReplayRing::new(64);
    for seq in 1..=10u64 {
        ring.push(Frame::new(
            seq,
            AguiEvent::TextMessageContent {
                message_id: "msg-1".into(),
                delta: seq.to_string(),
            },
        ));
    }
    let got = ring.since(Some(5)).expect("within the ring");
    let seqs: Vec<u64> = got.iter().map(|f| f.seq).collect();
    assert_eq!(seqs, vec![6, 7, 8, 9, 10]);

    assert_eq!(
        ring.since(None).expect("from the start").len(),
        10,
        "no `since` means from the start"
    );

    // Past the ring is an error, not a silent hole: the caller has to go to the
    // session store, which has every settled turn in full.
    let mut small = ReplayRing::new(3);
    for seq in 1..=10u64 {
        small.push(Frame::new(
            seq,
            AguiEvent::TextMessageEnd {
                message_id: "msg-1".into(),
            },
        ));
    }
    assert!(matches!(
        small.since(Some(2)),
        Err(ReplayError::TooOld { earliest: 8 })
    ));
}
