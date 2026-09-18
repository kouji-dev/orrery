//! The offline half of the drift check.
//!
//! `cargo xtask agui-drift` needs the network and is therefore CI-only. This
//! runs everywhere, against the same pinned lists, so the two can never say
//! different things about the same enum.

use orrery_agui::drift::{EMITTED, KNOWN_UNEMITTED, UPSTREAM};

/// Every name we say we emit, we can actually emit.
#[test]
fn emitted_names_are_real_variants() {
    // The list and the enum are written by hand in two places, which is exactly
    // the kind of pair that rots. This is the guard.
    let produced = [
        orrery_agui::AguiEvent::RunStarted {
            thread_id: String::new(),
            run_id: String::new(),
        },
        orrery_agui::AguiEvent::RunFinished {
            thread_id: String::new(),
            run_id: String::new(),
            result: None,
        },
        orrery_agui::AguiEvent::RunError {
            message: String::new(),
            code: None,
        },
        orrery_agui::AguiEvent::StepStarted {
            step_name: String::new(),
        },
        orrery_agui::AguiEvent::StepFinished {
            step_name: String::new(),
        },
        orrery_agui::AguiEvent::TextMessageStart {
            message_id: String::new(),
            role: String::new(),
        },
        orrery_agui::AguiEvent::TextMessageContent {
            message_id: String::new(),
            delta: String::new(),
        },
        orrery_agui::AguiEvent::TextMessageEnd {
            message_id: String::new(),
        },
        orrery_agui::AguiEvent::ToolCallStart {
            tool_call_id: String::new(),
            tool_call_name: String::new(),
            parent_message_id: None,
        },
        orrery_agui::AguiEvent::ToolCallArgs {
            tool_call_id: String::new(),
            delta: String::new(),
        },
        orrery_agui::AguiEvent::ToolCallEnd {
            tool_call_id: String::new(),
        },
        orrery_agui::AguiEvent::ToolCallResult {
            message_id: String::new(),
            tool_call_id: String::new(),
            content: String::new(),
            outcome: None,
        },
        orrery_agui::AguiEvent::StateSnapshot {
            snapshot: serde_json::Value::Null,
        },
        orrery_agui::AguiEvent::StateDelta { delta: Vec::new() },
        orrery_agui::AguiEvent::Custom {
            name: String::new(),
            value: serde_json::Value::Null,
        },
    ];
    let mut names: Vec<&str> = produced.iter().map(|e| e.type_name()).collect();
    names.sort_unstable();
    let mut declared: Vec<&str> = EMITTED.to_vec();
    declared.sort_unstable();
    assert_eq!(names, declared);
}

/// Nothing we emit is a name we invented, and nothing upstream has is
/// unaccounted for.
///
/// The second half is the one that matters: an upstream name that is neither
/// emitted nor deliberately skipped is a mapping somebody forgot, and it would
/// otherwise stay forgotten until a client asked for it.
#[test]
fn every_upstream_name_is_accounted_for() {
    let skipped: Vec<&str> = KNOWN_UNEMITTED.iter().map(|(name, _)| *name).collect();

    let invented: Vec<&&str> = EMITTED.iter().filter(|n| !UPSTREAM.contains(n)).collect();
    assert!(
        invented.is_empty(),
        "these are not AG-UI events: {invented:?}"
    );

    let stale: Vec<&&str> = skipped.iter().filter(|n| !UPSTREAM.contains(n)).collect();
    assert!(
        stale.is_empty(),
        "these are skipped but upstream no longer has them: {stale:?}"
    );

    let unaccounted: Vec<&&str> = UPSTREAM
        .iter()
        .filter(|n| !EMITTED.contains(n) && !skipped.contains(n))
        .collect();
    assert!(
        unaccounted.is_empty(),
        "upstream has these and we neither emit nor skip them: {unaccounted:?}"
    );

    for (name, reason) in KNOWN_UNEMITTED {
        assert!(!reason.is_empty(), "{name} is skipped for no stated reason");
    }
}
