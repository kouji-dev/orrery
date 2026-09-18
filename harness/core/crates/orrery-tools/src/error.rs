//! What can go wrong that is *not* a refusal.
//!
//! # There is no `Denied` variant here, and there never will be
//!
//! A denial is a value: [`dispatch`](crate::Registry::dispatch) returns
//! `Result<Outcome, ToolError>` and a refused call comes back as
//! `Ok(Outcome::Denied { .. })`. An error would have to be rendered by whoever
//! caught it, and then two layers would be describing the same refusal in two
//! different ways. `ToolError` is for the cases where the harness itself broke:
//! the host was unreachable, a manifest carried a schema that is not a schema.
//!
//! `tests/dispatch.rs` asserts the absence of a `Denied` variant, so this is
//! checked rather than merely promised.

/// A tool call that could not be carried out at all.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum ToolError {
    /// The registry was asked to dispatch a reference it does not hold.
    ///
    /// Not reachable through `resolve`, which answers `Unknown` as a value;
    /// this is a caller that built a [`ToolRef`](orrery_proto::ToolRef) by hand.
    #[error("no such tool: `{name}`")]
    NoSuchTool {
        /// The reference that was asked for.
        name: String,
    },
    /// The tool's declared `input_schema` is not a valid JSON Schema.
    ///
    /// A registration bug, not a call bug: the call never happened.
    #[error("tool `{name}` declares an invalid input schema: {message}")]
    InvalidSchema {
        /// Which tool.
        name: String,
        /// What the schema compiler said.
        message: String,
    },
    /// The extension host could not be reached or answered nonsense.
    #[error("host error calling `{name}`: {message}")]
    Host {
        /// Which tool.
        name: String,
        /// What went wrong.
        message: String,
    },
}
