//! What an extension's tool declares, and what a compiled-in one implements.

use async_trait::async_trait;
use orrery_proto::{Aspect, Outcome, ToolDescriptor};
use serde_json::Value;

use crate::ctx::{CallCtx, ToolBudget};
use crate::error::HostError;

/// One tool, as its extension declares it.
///
/// The `requires` list is the half that makes
/// [`LoadOutcome::Degraded`](orrery_proto::LoadOutcome) mean something: without
/// it, a missing `spawn` grant either fails the whole extension or is only
/// discovered at the first call. With it, the loader knows at load time which
/// tools cannot work and says so in the ledger.
#[derive(Clone, Debug, PartialEq)]
pub struct ToolDef {
    /// The name inside its extension, unqualified.
    pub name: String,
    /// What it does, in the model's context window.
    pub description: String,
    /// JSON Schema for the input. Validated at the dispatch boundary.
    pub input_schema: Value,
    /// Whether the effect is all-or-nothing, and therefore whether the broker
    /// must revert it on cancel.
    pub atomic: bool,
    /// The ceiling this tool declares. Narrows, never widens.
    pub ceiling: Option<ToolBudget>,
    /// The aspects this tool cannot work without.
    pub requires: Vec<Aspect>,
}

impl ToolDef {
    /// A tool with an open object schema, no ceiling and no requirements.
    #[must_use]
    pub fn new(name: impl Into<String>) -> Self {
        Self {
            name: name.into(),
            description: String::new(),
            input_schema: serde_json::json!({ "type": "object" }),
            atomic: false,
            ceiling: None,
            requires: Vec::new(),
        }
    }

    /// Set the description the model is shown.
    #[must_use]
    pub fn described(mut self, description: impl Into<String>) -> Self {
        self.description = description.into();
        self
    }

    /// Set the schema validated at the boundary.
    #[must_use]
    pub fn with_schema(mut self, schema: Value) -> Self {
        self.input_schema = schema;
        self
    }

    /// Declare the effect all-or-nothing.
    #[must_use]
    pub fn atomic(mut self, atomic: bool) -> Self {
        self.atomic = atomic;
        self
    }

    /// Declare a ceiling of this tool's own.
    #[must_use]
    pub fn with_ceiling(mut self, ceiling: ToolBudget) -> Self {
        self.ceiling = Some(ceiling);
        self
    }

    /// Declare what this tool cannot work without.
    #[must_use]
    pub fn requiring(mut self, aspects: impl IntoIterator<Item = Aspect>) -> Self {
        self.requires = aspects.into_iter().collect();
        self
    }

    /// What the model is shown, under a fully-qualified name.
    #[must_use]
    pub fn descriptor(&self, qualified: impl Into<String>) -> ToolDescriptor {
        ToolDescriptor {
            name: qualified.into(),
            description: self.description.clone(),
            input_schema: self.input_schema.clone(),
            atomic: self.atomic,
        }
    }
}

/// An extension compiled into the harness behind a cargo feature.
///
/// Translation #14: this is not a privileged path. A `native` extension is
/// reached through the same registry, the same policy check and the same
/// ledger as a child-process one; the only difference is where the code was
/// compiled. Its `manifest` is the same TOML a third party would ship, parsed
/// by the same parser, which is what `native::manifest_is_the_same_shape`
/// asserts.
#[async_trait]
pub trait NativeExtension: Send + Sync {
    /// The extension's `orrery.toml`, usually `include_str!`.
    fn manifest(&self) -> &str;

    /// Where that manifest lives, for error messages.
    fn manifest_path(&self) -> &str {
        "orrery.toml"
    }

    /// What it contributes, in registration order.
    fn tools(&self) -> Vec<ToolDef>;

    /// Anything that has to happen before the first call. The default is
    /// nothing, which is what most bundles need.
    async fn activate(&self) -> Result<(), HostError> {
        Ok(())
    }

    /// Run one of its tools.
    ///
    /// A refusal is `Ok(Outcome::Denied)`. An `Err` means the harness itself
    /// could not carry the call.
    async fn call(&self, tool: &str, input: Value, ctx: &CallCtx) -> Result<Outcome, HostError>;

    /// Anything that has to happen before the instance is dropped.
    async fn deactivate(&self) -> Result<(), HostError> {
        Ok(())
    }
}
