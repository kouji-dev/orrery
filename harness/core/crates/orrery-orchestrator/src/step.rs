//! What a workflow is made of.
//!
//! # A step carries a name
//!
//! §4.6 writes the five variants without one. A `ref` has to address something,
//! though, so the name lives on the wrapper: [`NamedStep`] is a [`Step`] plus
//! the name earlier steps are referred to by and the shape it returns. Nested
//! bodies — a `Parallel`'s branches, a `Loop`'s body — are `NamedStep`s too, for
//! the same reason.

use serde::{Deserialize, Serialize};

use orrery_proto::{Budget, Expr, GrantSpec, Predicate};

use crate::join::Join;

/// What a failing [`Step::Gate`] does.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum OnFail {
    /// End the workflow here, with the gate named.
    Stop,
    /// Run the preceding step again, once.
    Retry,
    /// Hand the decision up: the router is asked for the next rung.
    Escalate,
}

/// One step of a workflow.
///
/// `Tool` is the interesting one: **no model call at all**. Deterministic steps
/// between model calls are where cost comes out, since they cost nothing.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "kebab-case")]
pub enum Step {
    /// Run a sub-agent.
    Agent {
        /// Which one.
        subagent: String,
        /// What to give it.
        input: Expr,
    },
    /// Call a tool. No model call at all.
    Tool {
        /// Which tool.
        r#ref: String,
        /// What to give it.
        input: Expr,
    },
    /// Run several steps at once and join them.
    Parallel {
        /// The branches.
        steps: Vec<NamedStep>,
        /// How to join them.
        join: Join,
    },
    /// Run a body until a predicate holds, or until the cap.
    ///
    /// **Both** `until` and `max_iterations` are mandatory fields with no serde
    /// default: a loop that forgot to say when to stop does not load.
    Loop {
        /// What to run each time round.
        body: Vec<NamedStep>,
        /// When to stop.
        until: Predicate,
        /// The hard cap. Enforced whether or not `until` ever holds.
        max_iterations: u32,
    },
    /// Check a predicate and decide what a failure does.
    Gate {
        /// What must hold.
        check: Predicate,
        /// What a failure does.
        on_fail: OnFail,
    },
}

impl Step {
    /// The word this step is written with.
    #[must_use]
    pub const fn kind(&self) -> &'static str {
        match self {
            Step::Agent { .. } => "agent",
            Step::Tool { .. } => "tool",
            Step::Parallel { .. } => "parallel",
            Step::Loop { .. } => "loop",
            Step::Gate { .. } => "gate",
        }
    }

    /// Whether running this step can cost a model call.
    ///
    /// A `Tool` step cannot, which is the whole reason it exists.
    #[must_use]
    pub fn can_call_a_model(&self) -> bool {
        match self {
            Step::Agent { .. } => true,
            Step::Tool { .. } | Step::Gate { .. } => false,
            Step::Parallel { steps, .. } => steps.iter().any(|s| s.step.can_call_a_model()),
            Step::Loop { body, .. } => body.iter().any(|s| s.step.can_call_a_model()),
        }
    }
}

/// A step, its name, and what it declares it returns.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct NamedStep {
    /// What earlier-than-me steps are referred to by.
    pub name: String,
    /// The step itself.
    #[serde(flatten)]
    pub step: Step,
    /// What it returns, when the step says so itself rather than inheriting the
    /// declaration from the agent or tool it runs.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub returns: Option<TypeShape>,
}

/// The shape a step returns, as far as the load-time typecheck is concerned.
///
/// Deliberately small. This is not a type system; it is exactly enough to say
/// whether `.count` is a thing the target has.
#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum TypeShape {
    /// Nothing is declared, so nothing can be checked. A `ref` into it is
    /// accepted and reported as unchecked.
    #[default]
    Any,
    /// Text.
    Text,
    /// A number.
    Number,
    /// True or false.
    Bool,
    /// A map of known keys.
    Object(std::collections::BTreeMap<String, TypeShape>),
    /// A list of one shape.
    Array(Box<TypeShape>),
}

impl TypeShape {
    /// An object shape from pairs.
    #[must_use]
    pub fn object(fields: impl IntoIterator<Item = (impl Into<String>, TypeShape)>) -> Self {
        TypeShape::Object(fields.into_iter().map(|(k, v)| (k.into(), v)).collect())
    }

    /// Follow one key. `None` means the shape has no such key; `Some(Any)`
    /// means it could not be checked.
    #[must_use]
    pub fn field(&self, key: &str) -> Option<&TypeShape> {
        match self {
            TypeShape::Any => Some(&ANY_SHAPE),
            TypeShape::Object(fields) => fields.get(key),
            // An array is addressed by index, and a `path` is keys.
            _ => None,
        }
    }

    /// The keys a person could have meant, for the error message.
    #[must_use]
    pub fn keys(&self) -> Vec<String> {
        match self {
            TypeShape::Object(fields) => fields.keys().cloned().collect(),
            _ => Vec::new(),
        }
    }

    /// The word it is described with.
    #[must_use]
    pub fn word(&self) -> &'static str {
        match self {
            TypeShape::Any => "anything",
            TypeShape::Text => "text",
            TypeShape::Number => "a number",
            TypeShape::Bool => "a boolean",
            TypeShape::Object(_) => "an object",
            TypeShape::Array(_) => "a list",
        }
    }
}

/// The unchecked shape, as a place a `&TypeShape` can point at.
static ANY_SHAPE: TypeShape = TypeShape::Any;

/// A sub-agent, as declared.
///
/// # `budget` is mandatory
///
/// No `#[serde(default)]`, on purpose: an agent that cannot terminate is a cost
/// incident, so one that does not say what it may spend does not deserialize at
/// all. That is `subagent::budget_is_mandatory`.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct AgentDefinition {
    /// Its name.
    pub name: String,
    /// What it may spend. **Mandatory.**
    pub budget: Budget,
    /// Its prompt.
    #[serde(default)]
    pub prompt: String,
    /// Its model, when it names one.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub model: Option<String>,
    /// The tools it asks to see. Intersected with the parent's, never added to.
    #[serde(default)]
    pub tools: Vec<String>,
    /// What it asks for. A declaration narrows; it never substitutes.
    #[serde(default)]
    pub grant: GrantSpec,
    /// What it returns, for the load-time typecheck.
    #[serde(default)]
    pub returns: TypeShape,
    /// Whether it is allowed to ask a person anything.
    ///
    /// A sub-agent on a branch usually is not: there is nobody attached to its
    /// branch. See [`SubAgentError::CannotPrompt`](crate::SubAgentError).
    #[serde(default)]
    pub can_prompt: bool,
}

impl AgentDefinition {
    /// An agent with a name and a budget, which are the two things it cannot
    /// do without.
    #[must_use]
    pub fn new(name: impl Into<String>, budget: Budget) -> Self {
        Self {
            name: name.into(),
            budget,
            prompt: String::new(),
            model: None,
            tools: Vec::new(),
            grant: GrantSpec::default(),
            returns: TypeShape::Any,
            can_prompt: false,
        }
    }

    /// Declare what it returns.
    #[must_use]
    pub fn returning(mut self, returns: TypeShape) -> Self {
        self.returns = returns;
        self
    }

    /// Declare what it asks for.
    #[must_use]
    pub fn asking(mut self, grant: GrantSpec) -> Self {
        self.grant = grant;
        self
    }

    /// Declare the tools it asks to see.
    #[must_use]
    pub fn seeing(mut self, tools: impl IntoIterator<Item = impl Into<String>>) -> Self {
        self.tools = tools.into_iter().map(Into::into).collect();
        self
    }

    /// Let it ask a person.
    #[must_use]
    pub fn prompting(mut self, can_prompt: bool) -> Self {
        self.can_prompt = can_prompt;
        self
    }
}

/// A tool, as far as the typecheck is concerned: a name and a declared return.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct ToolDefinition {
    /// Its fully-qualified name.
    pub name: String,
    /// What it returns.
    #[serde(default)]
    pub returns: TypeShape,
}

impl ToolDefinition {
    /// A tool and what it returns.
    #[must_use]
    pub fn new(name: impl Into<String>, returns: TypeShape) -> Self {
        Self {
            name: name.into(),
            returns,
        }
    }
}

/// Everything a workflow may name, and what each of them returns.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Catalogue {
    /// The sub-agents in force.
    pub agents: std::collections::BTreeMap<String, AgentDefinition>,
    /// The tools in force.
    pub tools: std::collections::BTreeMap<String, ToolDefinition>,
}

impl Catalogue {
    /// An empty catalogue: every `ref` into a step is unchecked.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// Add an agent.
    #[must_use]
    pub fn with_agent(mut self, def: AgentDefinition) -> Self {
        self.agents.insert(def.name.clone(), def);
        self
    }

    /// Add a tool.
    #[must_use]
    pub fn with_tool(mut self, def: ToolDefinition) -> Self {
        self.tools.insert(def.name.clone(), def);
        self
    }

    /// What a step returns: its own declaration first, then the agent's or the
    /// tool's, then nothing.
    #[must_use]
    pub fn returns_of(&self, step: &NamedStep) -> TypeShape {
        if let Some(declared) = &step.returns {
            return declared.clone();
        }
        match &step.step {
            Step::Agent { subagent, .. } => self
                .agents
                .get(subagent)
                .map_or(TypeShape::Any, |a| a.returns.clone()),
            Step::Tool { r#ref, .. } => self
                .tools
                .get(r#ref)
                .map_or(TypeShape::Any, |t| t.returns.clone()),
            _ => TypeShape::Any,
        }
    }
}
