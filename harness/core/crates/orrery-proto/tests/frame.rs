//! The frame tags are dotted, every request carries an id, and a tool
//! reference splits on the last dot.

use std::path::{Path, PathBuf};

fn fixture_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/fixtures")
}

/// Every fixture file, sorted, as (name, value).
pub fn fixtures(prefix: &str) -> Vec<(String, serde_json::Value)> {
    let mut out: Vec<(String, serde_json::Value)> = std::fs::read_dir(fixture_dir())
        .expect("fixtures/ is missing")
        .map(|e| e.expect("unreadable fixture").path())
        .filter(|p| p.extension().is_some_and(|e| e == "json"))
        .filter(|p| p.file_name().unwrap().to_string_lossy().starts_with(prefix))
        .map(|p| {
            let name = p.file_stem().unwrap().to_string_lossy().into_owned();
            let raw = std::fs::read_to_string(&p).unwrap();
            let value =
                serde_json::from_str(&raw).unwrap_or_else(|e| panic!("{name} is not JSON: {e}"));
            (name, value)
        })
        .collect();
    out.sort_by(|a, b| a.0.cmp(&b.0));
    assert!(!out.is_empty(), "no fixtures matched `{prefix}`");
    out
}

mod frame {
    use orrery_proto::{
        Aspect, BranchId, CallId, Capability, PromptId, ReqId, RuleId, SessionId, Subject,
        SurfaceId, TurnId,
    };
    use orrery_proto::{
        CancelReason, ConsentAnswerKind, ConsentPrompt, ErrorDetail, ErrorScope, Event, Outcome,
        QueryOf, Request, Seq, Surface, SurfaceKind, SurfacePatch, ToolRef, Usage, UserInput,
    };

    fn tag(value: &serde_json::Value) -> &str {
        value["t"].as_str().expect("every frame carries a `t`")
    }

    #[test]
    fn tag_names_are_dotted() {
        let id = ReqId::new();
        let session = SessionId::new();
        let turn = TurnId::new();
        let surface = SurfaceId::new();

        let requests: Vec<(Request, &str)> = vec![
            (
                Request::SessionCreate {
                    id,
                    profile: "default".into(),
                    workspace: "/w".into(),
                },
                "session.create",
            ),
            (
                Request::SessionAttach {
                    id,
                    session,
                    since: Some(Seq(3)),
                },
                "session.attach",
            ),
            (
                Request::TurnSubmit {
                    id,
                    session,
                    input: UserInput::text("hi"),
                },
                "turn.submit",
            ),
            (Request::TurnCancel { id, session, turn }, "turn.cancel"),
            (
                Request::Intent {
                    id,
                    session,
                    surface,
                    value: serde_json::json!({ "a": 1 }),
                },
                "intent",
            ),
            (
                Request::ConsentAnswer {
                    id,
                    prompt: PromptId::new(),
                    answer: ConsentAnswerKind::AllowOnce,
                },
                "consent.answer",
            ),
            (
                Request::Command {
                    id,
                    session,
                    name: "compact".into(),
                    args: None,
                },
                "command",
            ),
            (
                Request::Query {
                    id,
                    of: QueryOf::Sessions {},
                },
                "query",
            ),
        ];
        for (request, wire) in &requests {
            let json = serde_json::to_value(request).unwrap();
            assert_eq!(
                tag(&json),
                *wire,
                "wrong tag; a dotted name needs an explicit rename"
            );
            assert_eq!(&serde_json::from_value::<Request>(json).unwrap(), request);
        }

        let events: Vec<(Event, &str)> = vec![
            (Event::TurnStarted { seq: Seq(1), turn }, "turn.started"),
            (
                Event::Delta {
                    seq: Seq(2),
                    surface,
                    patch: SurfacePatch::Append {
                        id: surface,
                        text: "x".into(),
                    },
                },
                "delta",
            ),
            (
                Event::ToolStarted {
                    seq: Seq(3),
                    call: CallId::new(),
                    r#ref: "ripgrep.search".parse::<ToolRef>().unwrap(),
                },
                "tool.started",
            ),
            (
                Event::ToolSettled {
                    seq: Seq(4),
                    call: CallId::new(),
                    outcome: Outcome::ok(),
                },
                "tool.settled",
            ),
            (
                Event::ConsentRequest {
                    seq: Seq(5),
                    prompt: ConsentPrompt {
                        id: PromptId::new(),
                        subject: Subject::Agent,
                        capabilities: vec![Capability::all(Aspect::Write)],
                        reason: "write outside the workspace".into(),
                        rule: Some(RuleId::new()),
                        surface: None,
                    },
                    deadline_ms: 30_000,
                },
                "consent.request",
            ),
            (
                Event::TurnSettled {
                    seq: Seq(6),
                    turn,
                    usage: Usage::default(),
                },
                "turn.settled",
            ),
            (
                Event::Error {
                    seq: Seq(7),
                    scope: ErrorScope::Turn,
                    detail: ErrorDetail {
                        code: "provider.unavailable".into(),
                        message: "no".into(),
                        retryable: true,
                        data: None,
                    },
                },
                "error",
            ),
        ];
        for (event, wire) in &events {
            let json = serde_json::to_value(event).unwrap();
            assert_eq!(tag(&json), *wire);
            assert_eq!(&serde_json::from_value::<Event>(json).unwrap(), event);
        }

        // No variant may be spelled kebab-case by accident.
        for (request, _) in &requests {
            let json = serde_json::to_value(request).unwrap();
            assert!(
                !tag(&json).contains('-'),
                "`{}` looks like a rename_all leak",
                tag(&json)
            );
        }

        let _ = (
            CancelReason::User,
            Surface::new(SurfaceKind::Text {
                value: String::new(),
                style: None,
            }),
            BranchId::new(),
        );
    }

    #[test]
    fn every_request_has_an_id() {
        for (name, value) in crate::fixtures("frame-request-") {
            assert!(
                value.get("id").is_some_and(serde_json::Value::is_string),
                "{name}: every request carries an `id`"
            );
            let request: Request = serde_json::from_value(value.clone())
                .unwrap_or_else(|e| panic!("{name} did not deserialize: {e}"));
            assert_eq!(
                serde_json::to_value(&request).unwrap(),
                value,
                "{name} did not round-trip"
            );
        }
    }

    #[test]
    fn every_event_round_trips() {
        for (name, value) in crate::fixtures("frame-event-") {
            assert!(
                value.get("seq").is_some_and(serde_json::Value::is_u64),
                "{name}: every event carries a `seq`"
            );
            let event: Event = serde_json::from_value(value.clone())
                .unwrap_or_else(|e| panic!("{name} did not deserialize: {e}"));
            assert_eq!(
                serde_json::to_value(&event).unwrap(),
                value,
                "{name} did not round-trip"
            );
        }
    }

    #[test]
    fn fixtures_cover_every_variant() {
        let tags: Vec<String> = crate::fixtures("frame-")
            .into_iter()
            .map(|(_, v)| v["t"].as_str().unwrap().to_owned())
            .collect();
        for wire in [
            "session.create",
            "session.attach",
            "turn.submit",
            "turn.cancel",
            "intent",
            "consent.answer",
            "command",
            "query",
            "turn.started",
            "delta",
            "tool.started",
            "tool.settled",
            "consent.request",
            "turn.settled",
            "error",
        ] {
            assert!(tags.iter().any(|t| t == wire), "no fixture for `{wire}`");
        }
    }

    #[test]
    fn tool_ref_splits_on_the_last_dot() {
        let plain: ToolRef = "ripgrep.search".parse().unwrap();
        assert_eq!(plain.ext.as_str(), "ripgrep");
        assert_eq!(plain.name, "search");
        assert_eq!(plain.to_string(), "ripgrep.search");

        let mcp: ToolRef = "mcp.jira.create_issue".parse().unwrap();
        assert_eq!(mcp.ext.as_str(), "mcp.jira");
        assert_eq!(mcp.name, "create_issue");
        assert_eq!(mcp.to_string(), "mcp.jira.create_issue");
        assert!(mcp.ext.is_mcp());

        assert_eq!(
            serde_json::to_value(&mcp).unwrap(),
            serde_json::json!("mcp.jira.create_issue")
        );
        assert_eq!(
            serde_json::from_value::<ToolRef>(serde_json::json!("mcp.jira.create_issue")).unwrap(),
            mcp
        );

        assert!(
            "search".parse::<ToolRef>().is_err(),
            "a bare name has no extension"
        );
        assert!("ripgrep.".parse::<ToolRef>().is_err());
        assert!(
            "a.b.c.d".parse::<ToolRef>().is_err(),
            "`a.b.c` is not a valid ExtId"
        );
    }

    #[test]
    fn consent_answers_are_kebab() {
        for (answer, wire) in [
            (ConsentAnswerKind::AllowOnce, "allow-once"),
            (ConsentAnswerKind::AllowAlways, "allow-always"),
            (ConsentAnswerKind::Deny, "deny"),
            (ConsentAnswerKind::DenyAlways, "deny-always"),
        ] {
            assert_eq!(
                serde_json::to_value(answer).unwrap(),
                serde_json::json!(wire)
            );
        }
    }
}
