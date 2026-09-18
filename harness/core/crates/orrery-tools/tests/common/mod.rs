//! Shared fixtures for the registry's integration tests.
#![allow(dead_code)]

use orrery_proto::{AgentScope, BranchId, ExtId, Grant, Layer};
use orrery_tools::{Registry, ToolSpec};

/// A scope that can see the named globs.
pub fn scope(tools: &[&str]) -> AgentScope {
    AgentScope {
        agent: "test".to_owned(),
        branch: BranchId::new(),
        tools: tools.iter().map(|t| (*t).to_owned()).collect(),
        grant: Grant::nothing(),
    }
}

/// Everything.
pub fn wide_scope() -> AgentScope {
    scope(&["*"])
}

/// An extension id, or a panic: test input is not user input.
pub fn ext(id: &str) -> ExtId {
    ExtId::new(id).expect("valid ext id")
}

/// Register `ext.name` at `layer` with a permissive object schema.
pub fn register(reg: &mut Registry, ext_id: &str, name: &str, layer: Layer) {
    reg.register(&ext(ext_id), layer, ToolSpec::new(name));
}
