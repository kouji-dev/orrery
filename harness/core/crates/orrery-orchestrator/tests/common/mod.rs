//! A recording executor.
//!
//! **No provider, no network, no model.** The orchestrator reaches the rest of
//! the system through `StepExecutor`, so a test implements that over counters
//! and canned values — which is also what lets
//! `workflow::tool_step_makes_no_model_call` assert that the provider was never
//! invoked at all.
#![allow(dead_code)]

pub mod store;

use std::collections::{HashMap, HashSet};
use std::sync::Mutex;
use std::time::Duration;

use async_trait::async_trait;
use orrery_orchestrator::workflow::{StepExecutor, StepFailure, StepOutput};
use orrery_proto::{Budget, Usage};
use serde_json::{Value, json};

/// One call that was made.
#[derive(Clone, Debug, PartialEq)]
pub struct Call {
    /// Which agent or tool.
    pub name: String,
    /// What it was given.
    pub input: Value,
    /// The budget it was given, for an agent call.
    pub budget: Option<Budget>,
}

/// An executor that records everything and calls nothing.
#[derive(Default)]
pub struct Fake {
    agent_calls: Mutex<Vec<Call>>,
    tool_calls: Mutex<Vec<Call>>,
    replies: HashMap<String, Value>,
    delays_ms: HashMap<String, u64>,
    failing: HashSet<String>,
    /// Tokens charged per agent call.
    cost: u64,
}

impl Fake {
    /// An executor that answers `{}` to everything and costs nothing.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }

    /// Answer this agent or tool with this value.
    #[must_use]
    pub fn answering(mut self, name: &str, value: Value) -> Self {
        self.replies.insert(name.to_owned(), value);
        self
    }

    /// Take this long to answer, so a join has something to cancel.
    #[must_use]
    pub fn slow(mut self, name: &str, ms: u64) -> Self {
        self.delays_ms.insert(name.to_owned(), ms);
        self
    }

    /// Fail this one.
    #[must_use]
    pub fn failing(mut self, name: &str) -> Self {
        self.failing.insert(name.to_owned());
        self
    }

    /// Charge this many tokens per agent call.
    #[must_use]
    pub fn costing(mut self, tokens: u64) -> Self {
        self.cost = tokens;
        self
    }

    /// Every agent call, in the order they were made.
    #[must_use]
    pub fn agent_calls(&self) -> Vec<Call> {
        self.agent_calls.lock().expect("not poisoned").clone()
    }

    /// Every tool call.
    #[must_use]
    pub fn tool_calls(&self) -> Vec<Call> {
        self.tool_calls.lock().expect("not poisoned").clone()
    }

    /// How many model calls were made. **The provider count.**
    #[must_use]
    pub fn model_calls(&self) -> usize {
        self.agent_calls.lock().expect("not poisoned").len()
    }

    fn reply(&self, name: &str) -> Value {
        self.replies
            .get(name)
            .cloned()
            .unwrap_or_else(|| json!({ "from": name }))
    }

    async fn pause(&self, name: &str) {
        if let Some(ms) = self.delays_ms.get(name) {
            tokio::time::sleep(Duration::from_millis(*ms)).await;
        }
    }
}

#[async_trait]
impl StepExecutor for Fake {
    async fn agent(
        &self,
        subagent: &str,
        input: Value,
        budget: Budget,
    ) -> Result<StepOutput, StepFailure> {
        self.agent_calls.lock().expect("not poisoned").push(Call {
            name: subagent.to_owned(),
            input,
            budget: Some(budget),
        });
        self.pause(subagent).await;
        if self.failing.contains(subagent) {
            return Err(StepFailure::new(
                "agent-failed",
                format!("{subagent} said no"),
            ));
        }
        Ok(StepOutput {
            value: self.reply(subagent),
            usage: Usage {
                input_tokens: self.cost,
                ..Usage::default()
            },
        })
    }

    async fn tool(&self, r#ref: &str, input: Value) -> Result<StepOutput, StepFailure> {
        self.tool_calls.lock().expect("not poisoned").push(Call {
            name: r#ref.to_owned(),
            input,
            budget: None,
        });
        self.pause(r#ref).await;
        if self.failing.contains(r#ref) {
            return Err(StepFailure::new(
                "tool-failed",
                format!("{} said no", r#ref),
            ));
        }
        // A tool costs nothing. That is the whole point of a tool step.
        Ok(StepOutput::free(self.reply(r#ref)))
    }
}
