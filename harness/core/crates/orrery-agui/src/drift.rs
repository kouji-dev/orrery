//! What upstream says AG-UI is, pinned so a drift can be detected.
//!
//! `cargo xtask agui-drift` fetches upstream's published schema and diffs the
//! variant names below against it. That job needs the network, so it is CI-only
//! and skips itself offline — this file is the offline half of the check, and
//! `orrery-agui`'s own tests assert that every name here is a variant the enum
//! actually produces.

/// The AG-UI protocol version this encoder was written against.
pub const AGUI_PROTOCOL_VERSION: &str = "1.0";

/// Where the drift job fetches upstream's event list from.
pub const AGUI_SCHEMA_URL: &str = "https://raw.githubusercontent.com/ag-ui-protocol/ag-ui/main/typescript-sdk/packages/core/src/events.ts";

/// The offline half of the check: upstream's list as of
/// [`AGUI_PROTOCOL_VERSION`], read out of the `@ag-ui/core` package that
/// `clients/sdk-ts` already installs.
///
/// `cargo xtask agui-drift` compares [`EMITTED`] and [`KNOWN_UNEMITTED`] against
/// *this* when it cannot reach the network, and against the fetched schema when
/// it can. Neither run needs a model, a key or a paid API.
pub const UPSTREAM: &[&str] = &[
    "TEXT_MESSAGE_START",
    "TEXT_MESSAGE_CONTENT",
    "TEXT_MESSAGE_END",
    "TEXT_MESSAGE_CHUNK",
    "TOOL_CALL_START",
    "TOOL_CALL_ARGS",
    "TOOL_CALL_END",
    "TOOL_CALL_CHUNK",
    "TOOL_CALL_RESULT",
    "STATE_SNAPSHOT",
    "STATE_DELTA",
    "MESSAGES_SNAPSHOT",
    "ACTIVITY_SNAPSHOT",
    "ACTIVITY_DELTA",
    "RAW",
    "CUSTOM",
    "RUN_STARTED",
    "RUN_FINISHED",
    "RUN_ERROR",
    "STEP_STARTED",
    "STEP_FINISHED",
    "REASONING_START",
    "REASONING_MESSAGE_START",
    "REASONING_MESSAGE_CONTENT",
    "REASONING_MESSAGE_END",
    "REASONING_MESSAGE_CHUNK",
    "REASONING_END",
    "REASONING_ENCRYPTED_VALUE",
    "SUBAGENT_STARTED",
    "SUBAGENT_FINISHED",
    "SUBAGENT_ERROR",
];

/// Every `type` tag this encoder can emit.
///
/// A name upstream has that is missing here is a mapping we have not written; a
/// name here that upstream does not have is a name we invented. The drift job
/// reports both, and fails only on the second.
pub const EMITTED: &[&str] = &[
    "RUN_STARTED",
    "RUN_FINISHED",
    "RUN_ERROR",
    "STEP_STARTED",
    "STEP_FINISHED",
    "TEXT_MESSAGE_START",
    "TEXT_MESSAGE_CONTENT",
    "TEXT_MESSAGE_END",
    "TOOL_CALL_START",
    "TOOL_CALL_ARGS",
    "TOOL_CALL_END",
    "TOOL_CALL_RESULT",
    "STATE_SNAPSHOT",
    "STATE_DELTA",
    "CUSTOM",
];

/// Upstream names we deliberately do not emit, and why.
///
/// Listed so the drift job does not report them as a gap every run. Each is a
/// phase-2-or-later mapping, not an oversight.
pub const KNOWN_UNEMITTED: &[(&str, &str)] = &[
    (
        "MESSAGES_SNAPSHOT",
        "replay is ours: `session.attach(since)`",
    ),
    ("RAW", "we never forward a provider's own frames verbatim"),
    (
        "TEXT_MESSAGE_CHUNK",
        "a convenience form of START/CONTENT/END; we always send the three",
    ),
    (
        "TOOL_CALL_CHUNK",
        "likewise: our differ knows where a call begins",
    ),
    (
        "REASONING_START",
        "§4.6 reasoning surfaces land with plan 09",
    ),
    ("REASONING_END", "§4.6 reasoning surfaces land with plan 09"),
    (
        "REASONING_MESSAGE_START",
        "§4.6 reasoning surfaces land with plan 09",
    ),
    (
        "REASONING_MESSAGE_CONTENT",
        "§4.6 reasoning surfaces land with plan 09",
    ),
    (
        "REASONING_MESSAGE_END",
        "§4.6 reasoning surfaces land with plan 09",
    ),
    (
        "REASONING_MESSAGE_CHUNK",
        "§4.6 reasoning surfaces land with plan 09",
    ),
    (
        "REASONING_ENCRYPTED_VALUE",
        "we do not carry a provider's opaque reasoning blob across the wire",
    ),
    ("ACTIVITY_SNAPSHOT", "§4.6 mode changes land with plan 11"),
    ("ACTIVITY_DELTA", "§4.6 mode changes land with plan 11"),
    ("SUBAGENT_STARTED", "§4.10 sub-agents land with plan 11"),
    ("SUBAGENT_FINISHED", "§4.10 sub-agents land with plan 11"),
    ("SUBAGENT_ERROR", "§4.10 sub-agents land with plan 11"),
];
