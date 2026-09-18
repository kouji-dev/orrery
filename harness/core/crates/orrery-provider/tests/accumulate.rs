//! Task 2: tool-call arguments arrive as fragments; they are reassembled once,
//! here, rather than once per provider and once more in the kernel.

use orrery_proto::CallId;
use orrery_provider::{ModelEvent, ToolCallAccumulator};

fn script(call: CallId, name: &str, fragments: &[&str]) -> Vec<ModelEvent> {
    let mut out = vec![ModelEvent::ToolUseStart {
        call,
        name: name.to_owned(),
    }];
    out.extend(fragments.iter().map(|f| ModelEvent::ToolUseDelta {
        call,
        json_fragment: (*f).to_owned(),
    }));
    out.push(ModelEvent::ToolUseEnd { call });
    out
}

#[test]
fn reassembles_split_json() {
    let call = CallId::new();
    let mut acc = ToolCallAccumulator::new();
    let mut done = Vec::new();
    for ev in script(call, "builtin.read", &["{\"pa", "th\":\"/tmp", "\"}"]) {
        if let Some(r) = acc.feed(&ev) {
            done.push(r.expect("well-formed json"));
        }
    }
    assert_eq!(done.len(), 1);
    assert_eq!(done[0].call, call);
    assert_eq!(done[0].name, "builtin.read");
    assert_eq!(done[0].input, serde_json::json!({ "path": "/tmp" }));
}

#[test]
fn two_interleaved_calls() {
    let a = CallId::new();
    let b = CallId::new();
    let events = vec![
        ModelEvent::ToolUseStart {
            call: a,
            name: "builtin.read".to_owned(),
        },
        ModelEvent::ToolUseStart {
            call: b,
            name: "builtin.write".to_owned(),
        },
        ModelEvent::ToolUseDelta {
            call: a,
            json_fragment: "{\"path\":".to_owned(),
        },
        ModelEvent::ToolUseDelta {
            call: b,
            json_fragment: "{\"path\":\"/b\"}".to_owned(),
        },
        ModelEvent::ToolUseDelta {
            call: a,
            json_fragment: "\"/a\"}".to_owned(),
        },
        ModelEvent::ToolUseEnd { call: b },
        ModelEvent::ToolUseEnd { call: a },
    ];
    let mut acc = ToolCallAccumulator::new();
    let mut done = Vec::new();
    for ev in &events {
        if let Some(r) = acc.feed(ev) {
            done.push(r.expect("well-formed json"));
        }
    }
    assert_eq!(done.len(), 2);
    assert_eq!(done[0].call, b);
    assert_eq!(done[0].input, serde_json::json!({ "path": "/b" }));
    assert_eq!(done[1].call, a);
    assert_eq!(done[1].input, serde_json::json!({ "path": "/a" }));
}

#[test]
fn malformed_json_is_an_error_not_a_panic() {
    let call = CallId::new();
    let mut acc = ToolCallAccumulator::new();
    let mut last = None;
    for ev in script(call, "builtin.read", &["{\"path\": "]) {
        if let Some(r) = acc.feed(&ev) {
            last = Some(r);
        }
    }
    let err = last.expect("end yields a result").expect_err("malformed");
    assert!(err.to_string().contains("builtin.read"), "{err}");
}

#[test]
fn a_call_with_no_fragments_is_an_empty_object() {
    let call = CallId::new();
    let mut acc = ToolCallAccumulator::new();
    let mut done = None;
    for ev in script(call, "builtin.now", &[]) {
        if let Some(r) = acc.feed(&ev) {
            done = Some(r.expect("empty is an empty object"));
        }
    }
    assert_eq!(done.expect("one call").input, serde_json::json!({}));
}

#[test]
fn an_end_without_a_start_is_an_error_not_a_panic() {
    let mut acc = ToolCallAccumulator::new();
    let r = acc
        .feed(&ModelEvent::ToolUseEnd { call: CallId::new() })
        .expect("reported");
    assert!(r.is_err());
}

#[test]
fn other_events_are_ignored() {
    let mut acc = ToolCallAccumulator::new();
    assert!(
        acc.feed(&ModelEvent::TextDelta {
            text: "hi".to_owned()
        })
        .is_none()
    );
}
