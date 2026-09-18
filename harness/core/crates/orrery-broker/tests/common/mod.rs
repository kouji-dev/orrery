//! Shared fixtures. A token can only come from the engine, so every test that
//! needs one builds a real engine and a broker over the engine's ledger.
#![allow(dead_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use orrery_broker::LocalBroker;
use orrery_policy::{
    CapabilityToken, Decision, PendingCall, PolicyBuilder, PolicyEngine, TokenLedger, TokenMinter,
};
use orrery_proto::{AgentScope, BranchId, Grant, Layer, Subject};

pub struct Fixture {
    pub dir: tempfile::TempDir,
    pub root: PathBuf,
    pub engine: PolicyEngine,
    pub broker: LocalBroker,
    pub audit: Arc<orrery_audit::MemorySink>,
}

/// Everything allowed, so a test can isolate the broker's own enforcement.
pub const ALLOW_ALL: &str = r#"
[permissions]
allow = [
  "read(./**)", "write(./**)", "spawn(*)", "net(domain: *)", "creds(*)",
  "tool(*)",
]
"#;

impl Fixture {
    pub fn new() -> Self {
        Self::with_rules(ALLOW_ALL)
    }

    pub fn with_rules(toml: &str) -> Self {
        let dir = tempfile::tempdir().expect("a temp dir");
        let root = dunce::canonicalize(dir.path()).expect("canonical root");
        let ledger = Arc::new(TokenLedger::new());
        let audit = orrery_audit::memory();
        let rules = PolicyBuilder::new(&root)
            .layer_toml(toml, "test.toml", Layer::Project, false)
            .expect("the fixture parses")
            .build()
            .expect("it compiles");
        let engine = PolicyEngine::new(rules)
            .with_minter(TokenMinter::new(Arc::clone(&ledger)))
            .with_audit(Arc::clone(&audit) as orrery_audit::Audit);
        let broker = LocalBroker::new(Arc::clone(&ledger))
            .with_audit(Arc::clone(&audit) as orrery_audit::Audit);
        Self {
            dir,
            root,
            engine,
            broker,
            audit,
        }
    }

    pub fn path(&self, rel: &str) -> PathBuf {
        self.root.join(rel)
    }

    pub fn scope() -> AgentScope {
        AgentScope {
            agent: "test".to_owned(),
            branch: BranchId::new(),
            tools: vec!["*".to_owned()],
            grant: Grant::nothing(),
        }
    }

    /// A token for one call, or a panic naming what was refused.
    pub fn token(&self, call: &PendingCall) -> CapabilityToken {
        match self.engine.check(call, &Subject::Agent, &Self::scope()) {
            Decision::Allow { token, .. } => token,
            other => panic!("expected Allow for {call}, got {other:?}"),
        }
    }

    pub fn read_token(&self, path: &Path) -> CapabilityToken {
        self.token(&PendingCall::read(path.display().to_string()))
    }

    pub fn write_token(&self, path: &Path) -> CapabilityToken {
        self.token(&PendingCall::write(path.display().to_string()))
    }
}
