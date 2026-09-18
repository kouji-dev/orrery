//! The provider-neutral conversation shape, and the one `Outcome` that is
//! shared between the transcript and the wire.

mod message {
    use orrery_proto::{
        CallId, CancelReason, ContentBlock, Message, MessageRole, Outcome, RuleId, Surface,
        SurfaceKind,
    };

    #[test]
    fn tool_result_carries_denial() {
        let call = CallId::new();
        let rule = RuleId::new();
        let message = Message {
            role: MessageRole::Assistant,
            content: vec![ContentBlock::ToolResult {
                call,
                outcome: Outcome::Denied {
                    rule,
                    reason: "write outside the workspace".into(),
                },
            }],
        };

        let json = serde_json::to_value(&message).unwrap();
        assert_eq!(json["content"][0]["t"], serde_json::json!("tool-result"));
        assert_eq!(
            json["content"][0]["outcome"]["t"],
            serde_json::json!("denied")
        );

        let back: Message = serde_json::from_value(json).unwrap();
        assert_eq!(back, message);
        let ContentBlock::ToolResult {
            outcome: Outcome::Denied { rule: got, .. },
            ..
        } = &back.content[0]
        else {
            panic!("the denial did not survive the round-trip");
        };
        assert_eq!(
            *got, rule,
            "the rule id must survive: a denial names what denied it"
        );
    }

    #[test]
    fn content_block_tags() {
        let call = CallId::new();
        let blocks = [
            (ContentBlock::Text { text: "hi".into() }, "text"),
            (
                ContentBlock::Image {
                    media_type: "image/png".into(),
                    data: "AA==".into(),
                },
                "image",
            ),
            (
                ContentBlock::ToolUse {
                    call,
                    name: "builtin.read".into(),
                    input: serde_json::json!({ "path": "a.rs" }),
                },
                "tool-use",
            ),
            (
                ContentBlock::ToolResult {
                    call,
                    outcome: Outcome::Ok {
                        surface: None,
                        value: None,
                    },
                },
                "tool-result",
            ),
            (ContentBlock::Thinking { text: "hmm".into() }, "thinking"),
        ];
        for (block, tag) in blocks {
            let json = serde_json::to_value(&block).unwrap();
            assert_eq!(json["t"], serde_json::json!(tag));
            assert_eq!(serde_json::from_value::<ContentBlock>(json).unwrap(), block);
        }
    }

    #[test]
    fn outcome_variants_round_trip() {
        let surface = Surface {
            id: None,
            status: None,
            kind: SurfaceKind::Text {
                value: "done".into(),
                style: None,
            },
        };
        let outcomes = [
            (
                Outcome::Ok {
                    surface: Some(surface),
                    value: None,
                },
                "ok",
            ),
            (
                Outcome::Denied {
                    rule: RuleId::new(),
                    reason: "no".into(),
                },
                "denied",
            ),
            (
                Outcome::Truncated {
                    surface: None,
                    bytes_emitted: 4096,
                    limit: 1024,
                },
                "truncated",
            ),
            (
                Outcome::Cancelled {
                    reason: CancelReason::User,
                },
                "cancelled",
            ),
            (
                Outcome::Unloaded {
                    ext: "buildgraph".parse().unwrap(),
                },
                "unloaded",
            ),
            (
                Outcome::Failed {
                    code: "enoent".into(),
                    message: "gone".into(),
                },
                "failed",
            ),
        ];
        for (outcome, tag) in outcomes {
            let json = serde_json::to_value(&outcome).unwrap();
            assert_eq!(json["t"], serde_json::json!(tag));
            assert_eq!(serde_json::from_value::<Outcome>(json).unwrap(), outcome);
        }
    }

    #[test]
    fn message_roles_are_kebab() {
        for (role, wire) in [
            (MessageRole::System, "system"),
            (MessageRole::User, "user"),
            (MessageRole::Assistant, "assistant"),
        ] {
            assert_eq!(serde_json::to_value(role).unwrap(), serde_json::json!(wire));
        }
    }
}
