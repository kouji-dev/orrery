//! The encoder's contract: every kernel frame maps, and the three shapes that
//! carry meaning beyond a name.

use orrery_agui::{AguiEvent, Encoder, PatchOp};
use orrery_proto::{
    CancelReason, Capability, ConsentPrompt, ErrorDetail, ErrorScope, Event, Outcome, PromptId,
    Subject, Surface, SurfaceId, SurfaceKind, SurfacePatch, ToolRef, TurnId, Usage,
};

fn sid(n: u8) -> SurfaceId {
    SurfaceId::from_uuid(uuid::Uuid::from_u128(
        0x0192_f3a0_0000_7000_8000_0000_0000_0000 + n as u128,
    ))
}

fn tid() -> TurnId {
    TurnId::from_uuid(uuid::Uuid::from_u128(
        0x0192_f3a0_0000_7000_8000_0000_0000_00ff,
    ))
}

fn enc() -> Encoder {
    Encoder::new("session-1")
}

/// Every `Event` variant produces at least one AG-UI event, and the ones the
/// mapping table names produce exactly what it says.
#[test]
fn maps_every_frame() {
    let call = orrery_proto::CallId::from_uuid(uuid::Uuid::from_u128(7));
    let cases: Vec<(Event, Vec<&'static str>)> = vec![
        (
            Event::TurnStarted {
                seq: orrery_proto::Seq(1),
                turn: tid(),
            },
            vec!["RUN_STARTED"],
        ),
        (
            Event::Delta {
                seq: orrery_proto::Seq(2),
                surface: sid(1),
                patch: SurfacePatch::Replace {
                    id: sid(1),
                    value: Surface {
                        id: Some(sid(1)),
                        status: None,
                        kind: SurfaceKind::Markdown {
                            value: String::new(),
                            complete: false,
                        },
                    },
                },
            },
            vec!["TEXT_MESSAGE_START"],
        ),
        (
            Event::Delta {
                seq: orrery_proto::Seq(3),
                surface: sid(1),
                patch: SurfacePatch::Append {
                    id: sid(1),
                    text: "hi".into(),
                },
            },
            vec!["TEXT_MESSAGE_CONTENT"],
        ),
        (
            Event::ToolStarted {
                seq: orrery_proto::Seq(4),
                call,
                r#ref: "builtin.read".parse::<ToolRef>().unwrap(),
            },
            vec!["TOOL_CALL_START"],
        ),
        (
            Event::ToolSettled {
                seq: orrery_proto::Seq(5),
                call,
                outcome: Outcome::ok(),
            },
            vec!["TOOL_CALL_END", "TOOL_CALL_RESULT"],
        ),
        (
            Event::ConsentRequest {
                seq: orrery_proto::Seq(6),
                prompt: ConsentPrompt {
                    id: PromptId::from_uuid(uuid::Uuid::from_u128(9)),
                    subject: Subject::Ext("builtin".parse().unwrap()),
                    capabilities: Vec::<Capability>::new(),
                    reason: "rm -rf target".into(),
                    rule: None,
                    surface: None,
                },
                deadline_ms: 30_000,
            },
            vec!["CUSTOM"],
        ),
        (
            Event::TurnSettled {
                seq: orrery_proto::Seq(7),
                turn: tid(),
                usage: Usage::default(),
            },
            // The markdown surface opened two cases up is still open, so the
            // turn closes it before it finishes. A client never holds an
            // unterminated message across a turn boundary.
            vec!["TEXT_MESSAGE_END", "RUN_FINISHED"],
        ),
        (
            Event::Error {
                seq: orrery_proto::Seq(8),
                scope: ErrorScope::Turn,
                detail: ErrorDetail {
                    code: "provider.unavailable".into(),
                    message: "nope".into(),
                    retryable: true,
                    data: None,
                },
            },
            vec!["RUN_ERROR"],
        ),
    ];

    let mut e = enc();
    for (frame, expected) in cases {
        let got: Vec<&str> = e.encode(&frame).iter().map(AguiEvent::type_name).collect();
        assert_eq!(got, expected, "mapping for {frame:?}");
    }

    // The cancelled outcome is the one `Outcome` the table folds into the same
    // pair rather than into RUN_ERROR.
    let mut e = enc();
    let got: Vec<&str> = e
        .encode(&Event::ToolSettled {
            seq: orrery_proto::Seq(9),
            call,
            outcome: Outcome::Cancelled {
                reason: CancelReason::User,
            },
        })
        .iter()
        .map(AguiEvent::type_name)
        .collect();
    assert_eq!(got, vec!["TOOL_CALL_END", "TOOL_CALL_RESULT"]);
}

/// `SurfacePatch::Set` is an RFC 6902 `replace`, pointed at the field it names.
#[test]
fn state_delta_is_json_patch_shaped() {
    let mut e = enc();
    let out = e.encode(&Event::Delta {
        seq: orrery_proto::Seq(1),
        surface: sid(2),
        patch: SurfacePatch::Set {
            id: sid(2),
            path: vec!["kind".into(), "rows".into()],
            value: serde_json::json!([]),
        },
    });
    let AguiEvent::StateDelta { delta } = &out[0] else {
        panic!("expected STATE_DELTA, got {:?}", out[0]);
    };
    assert_eq!(
        delta,
        &vec![PatchOp::Replace {
            path: format!("/surfaces/{}/kind/rows", sid(2)),
            value: serde_json::json!([]),
        }]
    );
}

/// Consent rides `Custom`, name and payload intact.
#[test]
fn consent_is_custom() {
    let prompt = ConsentPrompt {
        id: PromptId::from_uuid(uuid::Uuid::from_u128(9)),
        subject: Subject::Ext("builtin".parse().unwrap()),
        capabilities: Vec::<Capability>::new(),
        reason: "rm -rf target".into(),
        rule: None,
        surface: None,
    };
    let mut e = enc();
    let out = e.encode(&Event::ConsentRequest {
        seq: orrery_proto::Seq(1),
        prompt: prompt.clone(),
        deadline_ms: 30_000,
    });
    let AguiEvent::Custom { name, value } = &out[0] else {
        panic!("expected CUSTOM");
    };
    assert_eq!(name, orrery_agui::CONSENT_REQUEST);
    assert_eq!(value["deadline_ms"], 30_000);
    assert_eq!(
        value["prompt"],
        serde_json::to_value(&prompt).expect("prompt serialises")
    );
}

/// A tool call's arguments reach the client, and a client can rebuild the input
/// from the event stream alone.
///
/// Our frame vocabulary has no "tool arguments" event and AG-UI does. The
/// bridge is the surface a call streams into: a `delta` on the surface whose id
/// **is** the call's id, while that call is open, is that call's arguments.
#[test]
fn tool_args_are_emitted() {
    let call = orrery_proto::CallId::from_uuid(uuid::Uuid::from_u128(11));
    let as_surface = SurfaceId::from_uuid(*call.as_uuid());
    let mut encoder = enc();

    let started = encoder.encode(&Event::ToolStarted {
        seq: orrery_proto::Seq(1),
        call,
        r#ref: "builtin.read".parse::<ToolRef>().unwrap(),
    });
    assert_eq!(
        started.iter().map(AguiEvent::type_name).collect::<Vec<_>>(),
        vec!["TOOL_CALL_START"]
    );

    // The fragments a streaming provider emits, split mid-token.
    let mut rebuilt = String::new();
    for fragment in ["{\"pa", "th\":\"Cargo.toml\"}"] {
        let out = encoder.encode(&Event::Delta {
            seq: orrery_proto::Seq(2),
            surface: as_surface,
            patch: SurfacePatch::Append {
                id: as_surface,
                text: fragment.to_owned(),
            },
        });
        match out.as_slice() {
            [AguiEvent::ToolCallArgs {
                tool_call_id,
                delta,
            }] => {
                assert_eq!(tool_call_id, &call.to_string(), "argued under its own call");
                rebuilt.push_str(delta);
            }
            other => panic!("a delta on an open call is its arguments, not {other:?}"),
        }
    }
    assert_eq!(
        serde_json::from_str::<serde_json::Value>(&rebuilt).expect("valid JSON"),
        serde_json::json!({ "path": "Cargo.toml" }),
        "the input is reconstructible from the stream alone"
    );

    let settled = encoder.encode(&Event::ToolSettled {
        seq: orrery_proto::Seq(4),
        call,
        outcome: Outcome::ok(),
    });
    assert_eq!(
        settled.iter().map(AguiEvent::type_name).collect::<Vec<_>>(),
        vec!["TOOL_CALL_END", "TOOL_CALL_RESULT"]
    );

    // With the call settled, the same surface is an ordinary surface again.
    let after = encoder.encode(&Event::Delta {
        seq: orrery_proto::Seq(5),
        surface: as_surface,
        patch: SurfacePatch::Append {
            id: as_surface,
            text: "late".into(),
        },
    });
    assert_eq!(
        after.iter().map(AguiEvent::type_name).collect::<Vec<_>>(),
        vec!["STATE_DELTA"],
        "arguments are a thing an open call has, not a property of the id"
    );
}

/// Two calls open at once keep their arguments apart.
#[test]
fn interleaved_calls_keep_their_arguments() {
    let one = orrery_proto::CallId::from_uuid(uuid::Uuid::from_u128(21));
    let two = orrery_proto::CallId::from_uuid(uuid::Uuid::from_u128(22));
    let mut encoder = enc();
    for call in [one, two] {
        encoder.encode(&Event::ToolStarted {
            seq: orrery_proto::Seq(1),
            call,
            r#ref: "builtin.read".parse::<ToolRef>().unwrap(),
        });
    }

    let mut rebuilt = std::collections::BTreeMap::<String, String>::new();
    for (call, fragment) in [
        (one, "{\"path\":"),
        (two, "{\"pattern\":"),
        (one, "\"a.txt\"}"),
        (two, "\"TODO\"}"),
    ] {
        let id = SurfaceId::from_uuid(*call.as_uuid());
        let out = encoder.encode(&Event::Delta {
            seq: orrery_proto::Seq(2),
            surface: id,
            patch: SurfacePatch::Append {
                id,
                text: fragment.to_owned(),
            },
        });
        let [
            AguiEvent::ToolCallArgs {
                tool_call_id,
                delta,
            },
        ] = out.as_slice()
        else {
            panic!("arguments, not {out:?}");
        };
        rebuilt
            .entry(tool_call_id.clone())
            .or_default()
            .push_str(delta);
    }

    assert_eq!(rebuilt[&one.to_string()], "{\"path\":\"a.txt\"}");
    assert_eq!(rebuilt[&two.to_string()], "{\"pattern\":\"TODO\"}");
}
