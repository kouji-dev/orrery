//! The per-call ceiling that rides on every dispatch.

use serde::{Deserialize, Serialize};

/// What one tool call is allowed to consume.
///
/// The registry carries it and makes it impossible to dispatch without one; the
/// broker (plan 07) enforces it. `output_bytes` is applied **while** reading,
/// never after: a limit checked after the fact has already let the bytes into
/// memory.
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct ToolBudget {
    /// How long, in milliseconds of wall clock.
    pub wall_clock_ms: u64,
    /// How many bytes the tool may emit.
    pub output_bytes: u64,
    /// How much memory a spawned process may take. `None` for in-process tools.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub memory_bytes: Option<u64>,
}

impl ToolBudget {
    /// A ceiling on time and output, with no memory limit.
    #[must_use]
    pub const fn new(wall_clock_ms: u64, output_bytes: u64) -> Self {
        Self {
            wall_clock_ms,
            output_bytes,
            memory_bytes: None,
        }
    }

    /// Also cap the memory of a spawned process.
    #[must_use]
    pub const fn with_memory(mut self, memory_bytes: u64) -> Self {
        self.memory_bytes = Some(memory_bytes);
        self
    }

    /// The tighter of two ceilings, field by field.
    ///
    /// A `None` memory limit is *no* limit, so the present one always wins:
    /// narrowing never loosens.
    #[must_use]
    pub fn narrow(self, other: Self) -> Self {
        Self {
            wall_clock_ms: self.wall_clock_ms.min(other.wall_clock_ms),
            output_bytes: self.output_bytes.min(other.output_bytes),
            memory_bytes: match (self.memory_bytes, other.memory_bytes) {
                (Some(a), Some(b)) => Some(a.min(b)),
                (Some(a), None) => Some(a),
                (None, b) => b,
            },
        }
    }
}
