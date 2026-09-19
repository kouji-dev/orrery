//! What the host and a guest say to each other.
//!
//! Four methods host→guest, five guest→host, and `$/cancel` in both directions.
//! The guest half is what `@orrery/ext` implements; the host half is the broker,
//! and it is the *only* thing a guest can reach.

use base64::Engine as _;
use base64::engine::general_purpose::STANDARD as B64;
use orrery_ext_api::{ToolBudget, ToolDef};
use orrery_proto::{Aspect, Outcome};
use serde::{Deserialize, Serialize};
use serde_json::Value;

/// Host → guest: "what do you contribute?"
pub const LOAD: &str = "ext/load";
/// Host → guest: "run this tool".
pub const CALL: &str = "tool/call";
/// Host → guest: "you are going away".
pub const SHUTDOWN: &str = "ext/shutdown";

/// Guest → host: list what is under a directory.
pub const BROKER_LIST: &str = "broker/list";
/// Guest → host: read a bounded slice of a file.
pub const BROKER_READ: &str = "broker/read";
/// Guest → host: write a file.
pub const BROKER_WRITE: &str = "broker/write";
/// Guest → host: run a program.
pub const BROKER_SPAWN: &str = "broker/spawn";
/// Guest → host: make one HTTP request.
pub const BROKER_FETCH: &str = "broker/fetch";
/// Guest → host: read a named credential.
pub const BROKER_CREDENTIAL: &str = "broker/credential";

/// One tool, as a guest declares it.
///
/// The Rust side is [`ToolDef`]; this is its wire shape, and the conversion is
/// [`ToolWire::into_def`]. Two types rather than `Serialize` on `ToolDef`,
/// because what a guest may say and what the host holds are not the same thing:
/// a guest cannot, for instance, hand back a compiled schema validator.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ToolWire {
    /// The name inside the extension.
    pub name: String,
    /// What it does.
    #[serde(default)]
    pub description: String,
    /// JSON Schema for the input.
    #[serde(default = "open_object")]
    pub input_schema: Value,
    /// Whether the effect is all-or-nothing.
    #[serde(default)]
    pub atomic: bool,
    /// The aspects it cannot work without.
    #[serde(default)]
    pub requires: Vec<Aspect>,
    /// A ceiling of its own, in milliseconds and bytes.
    #[serde(default)]
    pub ceiling: Option<CeilingWire>,
}

fn open_object() -> Value {
    serde_json::json!({ "type": "object" })
}

/// A ceiling, as a guest declares it.
#[derive(Copy, Clone, Debug, Serialize, Deserialize)]
pub struct CeilingWire {
    /// How long.
    pub wall_clock_ms: u64,
    /// How much output.
    pub output_bytes: u64,
    /// How much memory, for anything it spawns.
    #[serde(default)]
    pub memory_bytes: Option<u64>,
}

impl ToolWire {
    /// The host-side tool definition.
    #[must_use]
    pub fn into_def(self) -> ToolDef {
        let mut def = ToolDef::new(self.name)
            .described(self.description)
            .with_schema(self.input_schema)
            .atomic(self.atomic)
            .requiring(self.requires);
        if let Some(c) = self.ceiling {
            def = def.with_ceiling(ToolBudget {
                wall_clock_ms: c.wall_clock_ms,
                output_bytes: c.output_bytes,
                memory_bytes: c.memory_bytes,
            });
        }
        def
    }
}

/// What `ext/load` answers.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
pub struct LoadReply {
    /// What the guest actually contributes.
    #[serde(default)]
    pub tools: Vec<ToolWire>,
    /// Anything it could not bring up, in words a person can act on. A guest
    /// that reports its own problems here degrades rather than failing.
    #[serde(default)]
    pub problems: Vec<String>,
}

/// What `tool/call` is given.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CallParams {
    /// Which call, so the guest can put it in its own logs.
    pub call: String,
    /// Which tool.
    pub tool: String,
    /// Its input, already validated against the schema by the registry.
    pub input: Value,
    /// The ceiling this call runs under.
    pub budget: CeilingWire,
}

/// What `tool/call` answers.
///
/// An [`Outcome`], and nothing else: a guest cannot invent a third category
/// between "it worked" and "it was refused", because the client would have
/// nowhere to draw it.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CallReply {
    /// How it went.
    pub outcome: Outcome,
}

/// What `broker/read` is given.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadParams {
    /// Which file.
    pub path: String,
    /// Where to start.
    #[serde(default)]
    pub offset: u64,
    /// How much, at most. Not optional: see [`orrery_ext_api::ReadRequest`].
    pub limit: u64,
}

/// What `broker/read` answers.
///
/// Text first. A JSON string cannot carry arbitrary bytes, and base64 in every
/// read would cost a third of the bandwidth on the hot path — reading source
/// files — to buy a case most calls do not have.
///
/// So [`ReadReply::bytes_b64`] is **conditional**, not additional: it is filled
/// in only when the lossy decode actually lost something, which is exactly the
/// case where `text` alone is a lie. A guest that wants the real bytes reads
/// `bytes_b64` when it is there and falls back to `text` when it is not; a
/// guest written before the field existed still parses the reply, because the
/// field is `Option` and skipped when empty.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ReadReply {
    /// What was read, lossily decoded.
    pub text: String,
    /// Whether the file ended inside this chunk.
    pub eof: bool,
    /// The whole file's size, when known.
    #[serde(default)]
    pub total: Option<u64>,
    /// The chunk's exact bytes, base64, when `text` could not carry them.
    ///
    /// `None` on a read whose bytes were valid UTF-8 — the common case, where
    /// `text` is already lossless.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bytes_b64: Option<String>,
}

impl ReadReply {
    /// Build a reply from a chunk, encoding only when the decode would lose.
    #[must_use]
    pub fn from_bytes(bytes: &[u8], eof: bool, total: Option<u64>) -> Self {
        match std::str::from_utf8(bytes) {
            // Lossless already: no base64, no extra third of the bandwidth.
            Ok(text) => Self {
                text: text.to_owned(),
                eof,
                total,
                bytes_b64: None,
            },
            Err(_) => Self {
                text: String::from_utf8_lossy(bytes).into_owned(),
                eof,
                total,
                bytes_b64: Some(B64.encode(bytes)),
            },
        }
    }

    /// The exact bytes this reply carries: the base64 when there is some, and
    /// the text's own bytes when there is not.
    ///
    /// # Errors
    ///
    /// [`base64::DecodeError`] when `bytes_b64` is not valid base64, which can
    /// only happen if something other than a host wrote the frame.
    pub fn bytes(&self) -> Result<Vec<u8>, base64::DecodeError> {
        match &self.bytes_b64 {
            Some(encoded) => Self::decode_b64(encoded),
            None => Ok(self.text.clone().into_bytes()),
        }
    }

    /// Decode one `bytes_b64` value.
    ///
    /// # Errors
    ///
    /// [`base64::DecodeError`] when the value is not valid base64.
    pub fn decode_b64(encoded: &str) -> Result<Vec<u8>, base64::DecodeError> {
        B64.decode(encoded)
    }
}

/// What `broker/list` is given.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListParams {
    /// Where to look.
    pub path: String,
    /// Whether to descend.
    #[serde(default)]
    pub recursive: bool,
    /// How many entries at most. Not optional: a walk has a ceiling like
    /// everything else.
    pub limit: u64,
}

/// One thing a listing found.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListEntryWire {
    /// Where it is.
    pub path: String,
    /// Whether it is a directory.
    pub is_dir: bool,
    /// Its size, when known.
    #[serde(default)]
    pub size: Option<u64>,
}

/// What `broker/list` answers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ListReply {
    /// What this call may see. What it may not read is not named.
    pub entries: Vec<ListEntryWire>,
    /// Whether the ceiling cut the walk short.
    pub truncated: bool,
}

/// What `broker/write` is given.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct WriteParams {
    /// Which file.
    pub path: String,
    /// The new contents.
    pub text: String,
    /// Whether the write must be all-or-nothing.
    #[serde(default = "yes")]
    pub atomic: bool,
}

fn yes() -> bool {
    true
}

/// What `broker/spawn` is given.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpawnParams {
    /// The program.
    pub program: String,
    /// Its arguments.
    #[serde(default)]
    pub args: Vec<String>,
    /// Where to run it.
    #[serde(default)]
    pub cwd: Option<String>,
    /// How long to wait, overriding nothing: the call's ceiling still applies.
    #[serde(default)]
    pub timeout_ms: Option<u64>,
}

/// What `broker/spawn` answers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct SpawnReply {
    /// Its exit code.
    #[serde(default)]
    pub status: Option<i32>,
    /// What it printed.
    pub stdout: String,
    /// What it complained about.
    pub stderr: String,
    /// Whether the ceiling cut it short.
    pub truncated: bool,
}

/// What `broker/credential` is given.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CredentialParams {
    /// Which credential.
    pub name: String,
}

/// What `broker/credential` answers.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CredentialReply {
    /// The value. It never reaches the transcript.
    pub value: String,
}

#[cfg(test)]
mod tests {
    use super::{CallReply, ToolWire};
    use orrery_proto::Outcome;

    #[test]
    fn a_guest_declares_a_tool_with_almost_nothing() {
        let wire: ToolWire = serde_json::from_str(r#"{"name":"say"}"#).unwrap();
        let def = wire.into_def();
        assert_eq!(def.name, "say");
        assert!(!def.atomic);
        assert_eq!(def.input_schema["type"], "object");
    }

    #[test]
    fn an_outcome_round_trips_over_the_wire() {
        let reply = CallReply {
            outcome: Outcome::Failed {
                code: "boom".into(),
                message: "it broke".into(),
            },
        };
        let json = serde_json::to_string(&reply).unwrap();
        let back: CallReply = serde_json::from_str(&json).unwrap();
        assert_eq!(back.outcome, reply.outcome);
    }
}
