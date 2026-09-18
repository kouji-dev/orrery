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

impl ToolBudget {
    /// The budget a call actually runs under.
    ///
    /// The **minimum**, field by field, of the profile's ceiling, the agent's
    /// own and whatever the tool's manifest declared. Three sources, one
    /// direction: every one of them may tighten and none may loosen, so a tool
    /// that asks for an hour inside an agent given a second gets a second.
    ///
    /// The manifest ceiling is optional because most tools do not declare one;
    /// absent is "no opinion", never "no limit".
    #[must_use]
    pub fn effective(profile: Self, agent: Self, tool: Option<Self>) -> Self {
        let base = profile.narrow(agent);
        match tool {
            Some(tool) => base.narrow(tool),
            None => base,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::ToolBudget;

    #[test]
    fn derives_from_scope_and_manifest() {
        let profile = ToolBudget::new(60_000, 1 << 20).with_memory(1 << 30);
        let agent = ToolBudget::new(10_000, 4 << 20);
        let tool = ToolBudget::new(30_000, 1 << 10).with_memory(1 << 20);

        assert_eq!(
            ToolBudget::effective(profile, agent, Some(tool)),
            ToolBudget {
                wall_clock_ms: 10_000, // the agent's
                output_bytes: 1 << 10, // the tool's
                memory_bytes: Some(1 << 20),
            }
        );
    }

    #[test]
    fn a_tool_without_a_ceiling_has_no_opinion() {
        let profile = ToolBudget::new(60_000, 1 << 20);
        let agent = ToolBudget::new(10_000, 4 << 20);
        assert_eq!(
            ToolBudget::effective(profile, agent, None),
            ToolBudget::new(10_000, 1 << 20)
        );
    }

    #[test]
    fn narrowing_never_loosens() {
        let tight = ToolBudget::new(1, 1).with_memory(1);
        let loose = ToolBudget::new(u64::MAX, u64::MAX);
        assert_eq!(tight.narrow(loose), tight);
        assert_eq!(loose.narrow(tight), tight);
    }
}
