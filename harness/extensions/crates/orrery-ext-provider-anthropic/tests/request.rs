//! Task 6: what goes on the wire.

use std::sync::Arc;

use orrery_ext_provider_anthropic::request::build_body;
use orrery_proto::{ContentBlock, Message, MessageRole, Outcome};
use orrery_provider::{Capabilities, ModelRequest, ToolDescriptor};

fn caps(tools: bool, images: bool, cache: bool) -> Capabilities {
    Capabilities {
        tools,
        images,
        cache,
        max_context: 200_000,
        max_output: 64_000,
    }
}

fn tool() -> ToolDescriptor {
    ToolDescriptor {
        name: "builtin.read".to_owned(),
        description: "Read a file".to_owned(),
        input_schema: serde_json::json!({ "type": "object" }),
    }
}

fn three_messages() -> Arc<[Message]> {
    Arc::from([
        Message::text(MessageRole::User, "one"),
        Message::text(MessageRole::Assistant, "two"),
        Message::text(MessageRole::User, "three"),
    ])
}

#[test]
fn tool_descriptors_omitted_when_unsupported() {
    let mut req = ModelRequest::new("claude-sonnet-4-5", three_messages(), 1024);
    req.tools = Arc::from([tool()]);

    let with = build_body(&req, &caps(true, true, true)).expect("builds");
    assert!(with.get("tools").is_some(), "{with}");

    let without = build_body(&req, &caps(false, true, true)).expect("builds");
    assert!(
        without.get("tools").is_none(),
        "a provider without tools must not be sent a `tools` key: {without}"
    );
}

#[test]
fn an_empty_tool_list_is_no_key_at_all() {
    let req = ModelRequest::new("claude-sonnet-4-5", three_messages(), 1024);
    let body = build_body(&req, &caps(true, true, true)).expect("builds");
    assert!(body.get("tools").is_none(), "{body}");
}

#[test]
fn cache_control_lands_on_the_breakpoint() {
    let mut req = ModelRequest::new("claude-sonnet-4-5", three_messages(), 1024);
    req.cache_breakpoint = Some(2);
    let body = build_body(&req, &caps(true, true, true)).expect("builds");
    let messages = body["messages"].as_array().expect("array");
    assert_eq!(messages.len(), 3);
    for (i, m) in messages.iter().enumerate() {
        let last = m["content"]
            .as_array()
            .expect("blocks")
            .last()
            .expect("one");
        let marked = last.get("cache_control").is_some();
        assert_eq!(marked, i == 2, "message {i}: {m}");
    }
    assert_eq!(
        messages[2]["content"][0]["cache_control"],
        serde_json::json!({ "type": "ephemeral" })
    );
}

#[test]
fn cache_breakpoint_degrades_to_a_no_op_without_the_capability() {
    // Open question 4: the field must degrade, never force a bad request.
    let mut req = ModelRequest::new("claude-sonnet-4-5", three_messages(), 1024);
    req.cache_breakpoint = Some(2);
    let body = build_body(&req, &caps(true, true, false)).expect("builds");
    let s = body.to_string();
    assert!(!s.contains("cache_control"), "{s}");
}

#[test]
fn an_out_of_range_breakpoint_is_ignored_not_an_error() {
    let mut req = ModelRequest::new("claude-sonnet-4-5", three_messages(), 1024);
    req.cache_breakpoint = Some(99);
    let body = build_body(&req, &caps(true, true, true)).expect("builds");
    assert!(!body.to_string().contains("cache_control"));
}

#[test]
fn system_messages_are_hoisted_and_the_stream_flag_is_set() {
    let mut req = ModelRequest::new(
        "claude-sonnet-4-5",
        Arc::from([
            Message::text(MessageRole::System, "in-band system"),
            Message::text(MessageRole::User, "hi"),
        ]),
        1024,
    );
    req.system = Some(Arc::from("out-of-band system"));
    let body = build_body(&req, &caps(true, true, true)).expect("builds");
    assert_eq!(body["stream"], serde_json::json!(true));
    assert_eq!(body["max_tokens"], serde_json::json!(1024));
    let system = body["system"].as_str().expect("string");
    assert!(system.contains("out-of-band system"), "{system}");
    assert!(system.contains("in-band system"), "{system}");
    // Anthropic has no system role inside `messages`.
    assert_eq!(body["messages"].as_array().expect("array").len(), 1);
}

#[test]
fn an_image_without_the_capability_is_a_bad_request_not_a_silent_drop() {
    let req = ModelRequest::new(
        "claude-sonnet-4-5",
        Arc::from([Message {
            role: MessageRole::User,
            content: vec![ContentBlock::Image {
                media_type: "image/png".to_owned(),
                data: "aGk=".to_owned(),
            }],
        }]),
        1024,
    );
    let e = build_body(&req, &caps(true, false, true)).expect_err("rejected");
    assert!(!e.is_retryable());
    assert_eq!(e.code(), "bad_request");

    let ok = build_body(&req, &caps(true, true, true)).expect("builds");
    assert_eq!(ok["messages"][0]["content"][0]["source"]["type"], "base64");
}

#[test]
fn tool_use_and_tool_result_ids_match_within_the_request() {
    let call = orrery_proto::CallId::new();
    let req = ModelRequest::new(
        "claude-sonnet-4-5",
        Arc::from([
            Message {
                role: MessageRole::Assistant,
                content: vec![ContentBlock::ToolUse {
                    call,
                    name: "builtin.read".to_owned(),
                    input: serde_json::json!({ "path": "Cargo.toml" }),
                }],
            },
            Message {
                role: MessageRole::User,
                content: vec![ContentBlock::ToolResult {
                    call,
                    outcome: Outcome::Failed {
                        code: "enoent".to_owned(),
                        message: "no such file".to_owned(),
                    },
                }],
            },
        ]),
        1024,
    );
    let body = build_body(&req, &caps(true, true, true)).expect("builds");
    let use_id = body["messages"][0]["content"][0]["id"]
        .as_str()
        .expect("id")
        .to_owned();
    assert_eq!(body["messages"][1]["content"][0]["tool_use_id"], use_id);
    // A failed tool has to arrive at the model *as* a failure.
    assert_eq!(body["messages"][1]["content"][0]["is_error"], true);
}

#[test]
fn optional_knobs_are_absent_when_unset() {
    let mut req = ModelRequest::new("claude-sonnet-4-5", three_messages(), 1024);
    let bare = build_body(&req, &caps(true, true, true)).expect("builds");
    assert!(bare.get("temperature").is_none(), "{bare}");
    assert!(bare.get("stop_sequences").is_none(), "{bare}");

    req.temperature = Some(0.2);
    req.stop = vec!["\n\nHuman:".to_owned()];
    let set = build_body(&req, &caps(true, true, true)).expect("builds");
    assert!(set["temperature"].as_f64().expect("number") > 0.19);
    assert_eq!(set["stop_sequences"][0], "\n\nHuman:");
}
