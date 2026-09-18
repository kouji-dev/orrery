//! Shared fixtures for the policy crate's integration tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use orrery_policy::{PolicyBuilder, PolicyEngine, ResolvedRules};
use orrery_proto::{AgentScope, BranchId, Grant, Layer};

/// A scope that narrows nothing: the rules are what is under test.
pub fn wide_scope() -> AgentScope {
    AgentScope {
        agent: "test".to_owned(),
        branch: BranchId::new(),
        tools: vec!["*".to_owned()],
        grant: Grant::nothing(),
    }
}

/// A workspace with a `src/main.rs` in it, so paths have something real to
/// resolve against.
pub struct Workspace {
    dir: tempfile::TempDir,
    root: PathBuf,
}

impl Workspace {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("a temp dir");
        let root = dunce::canonicalize(dir.path()).expect("canonical root");
        std::fs::create_dir_all(root.join("src")).unwrap();
        std::fs::write(root.join("src/main.rs"), "fn main() {}").unwrap();
        std::fs::create_dir_all(root.join("secrets")).unwrap();
        std::fs::write(root.join("secrets/key.txt"), "shh").unwrap();
        Self { dir, root }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    /// Keep the directory alive for as long as the caller wants it.
    pub fn keep(&self) -> &tempfile::TempDir {
        &self.dir
    }
}

/// Build rules from one TOML layer.
pub fn rules(ws: &Workspace, layer: Layer, toml: &str) -> ResolvedRules {
    PolicyBuilder::new(ws.root())
        .layer_toml(toml, "test.toml", layer, false)
        .expect("the fixture parses")
        .build()
        .expect("the fixture compiles")
}

/// An engine over one layer, recording into a memory audit.
pub fn engine(ws: &Workspace, toml: &str) -> (PolicyEngine, Arc<orrery_audit::MemorySink>) {
    let audit = orrery_audit::memory();
    let engine = PolicyEngine::new(rules(ws, Layer::Project, toml))
        .with_audit(Arc::clone(&audit) as orrery_audit::Audit);
    (engine, audit)
}
