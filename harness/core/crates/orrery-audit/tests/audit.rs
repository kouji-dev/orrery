//! Task 1 of `harness/docs/plans/07-policy-broker-audit.md`.

use orrery_audit::{AuditEvent, AuditSink, Digest, FileSink, MemorySink, CallOutcome, Stream};
use orrery_proto::CallId;

const SECRET: &str = "sk-ant-not-a-real-key-000000";

fn call_with_secret() -> AuditEvent {
    AuditEvent::tool_call(
        CallId::new(),
        "shell.exec",
        &serde_json::json!({ "cmd": "curl", "header": SECRET }),
        CallOutcome::Ok,
    )
}

#[test]
fn inputs_are_hashed() {
    let sink = MemorySink::new();
    sink.append(call_with_secret());

    let jsonl = sink.to_jsonl();
    assert!(
        !jsonl.contains(SECRET),
        "the raw input reached the sink:\n{jsonl}"
    );

    // ...and the hash is stable across runs and across processes.
    let again = Digest::of_json(&serde_json::json!({ "cmd": "curl", "header": SECRET }));
    assert!(
        jsonl.contains(again.as_str()),
        "the digest is not in the record:\n{jsonl}"
    );
    assert_eq!(
        again.as_str(),
        Digest::of_json(&serde_json::json!({ "header": SECRET, "cmd": "curl" })).as_str(),
        "the digest must not depend on map ordering"
    );
}

#[test]
fn a_secret_in_a_nested_input_is_hashed_too() {
    let sink = MemorySink::new();
    sink.append(AuditEvent::tool_call(
        CallId::new(),
        "http.get",
        &serde_json::json!({ "auth": { "bearer": [SECRET] } }),
        CallOutcome::Ok,
    ));
    assert!(!sink.to_jsonl().contains(SECRET));
}

#[test]
fn append_only() {
    let sink = MemorySink::new();
    for i in 0..3u64 {
        sink.append(AuditEvent::ModelRequest {
            model: format!("m{i}"),
            input_tokens: i,
            output_tokens: 0,
        });
    }
    let first = sink.records();
    assert_eq!(first.len(), 3);
    assert_eq!(first[0].seq, 0);
    assert_eq!(first[2].seq, 2);

    sink.append(AuditEvent::ModelRequest {
        model: "m3".into(),
        input_tokens: 3,
        output_tokens: 0,
    });
    let second = sink.records();
    assert_eq!(second.len(), 4);
    assert_eq!(&second[..3], &first[..], "an earlier record changed");
}

#[test]
fn file_sink_only_ever_grows() {
    let dir = tempfile::tempdir().unwrap();
    let path = dir.path().join("audit.jsonl");
    let sink = FileSink::open(&path).unwrap();
    sink.append(AuditEvent::ModelRequest {
        model: "a".into(),
        input_tokens: 1,
        output_tokens: 2,
    });
    let after_one = std::fs::read_to_string(&path).unwrap();
    sink.append(call_with_secret());
    let after_two = std::fs::read_to_string(&path).unwrap();

    assert!(after_two.starts_with(&after_one), "a line was rewritten");
    assert_eq!(after_two.lines().count(), 2);
    assert!(!after_two.contains(SECRET));
}

#[test]
fn every_stream_has_a_target() {
    assert_eq!(Stream::Load.target(), "orrery.load");
    assert_eq!(Stream::Audit.target(), "orrery.audit");
    assert_eq!(Stream::Telemetry.target(), "orrery.telemetry");
    assert_eq!(Stream::of_target("orrery.audit.policy"), Some(Stream::Audit));
    assert_eq!(Stream::of_target("tokio::task"), None);
}
