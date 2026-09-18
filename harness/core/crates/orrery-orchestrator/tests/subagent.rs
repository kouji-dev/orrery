//! Task 6 · sub-agents on branches, and the parent merging at the join.

mod common;

use std::sync::Arc;
use std::time::Duration;

use async_trait::async_trait;
use common::store::MemoryStore;
use orrery_orchestrator::step::AgentDefinition;
use orrery_orchestrator::subagent::{SubAgentError, TurnReport, TurnRunner, spawn};
use orrery_proto::{
    AgentScope, Aspect, Budget, Capability, Consent, ContentBlock, Grant, GrantSpec, SessionId,
    TurnId, Usage, UserInput,
};
use orrery_session::{BranchLease, BranchOutcome, NewTurn, SessionStore, TurnKind};

fn budget() -> Budget {
    Budget {
        max_turns: 4,
        max_tokens: 20_000,
        wall_clock_ms: 60_000,
        max_micro_usd: None,
    }
}

/// A parent that may read and write, and see two tools.
fn parent_scope(branch: orrery_proto::BranchId) -> AgentScope {
    AgentScope {
        agent: "main".to_owned(),
        branch,
        tools: vec!["fs.read".to_owned(), "fs.write".to_owned()],
        grant: Grant {
            capabilities: vec![
                Capability::all(Aspect::Read),
                Capability::scoped(Aspect::Tool, ["fs.read", "fs.write"]),
            ],
            consent: Consent::Once,
        },
    }
}

/// A runner that appends ordinary turns to the child's branch.
struct Chatty {
    turns: u32,
}

#[async_trait]
impl TurnRunner for Chatty {
    async fn run(
        &self,
        lease: &BranchLease,
        scope: &AgentScope,
        input: UserInput,
    ) -> Result<TurnReport, SubAgentError> {
        // This is the point of running on a branch: the child's work is turns,
        // not one opaque tool result.
        let store = STORE.with(Arc::clone);
        store
            .append(lease, NewTurn::new(TurnKind::User { input }))
            .await?;
        for i in 0..self.turns {
            store
                .append(
                    lease,
                    NewTurn::new(TurnKind::Assistant {
                        content: vec![ContentBlock::Text {
                            text: format!("{} pass {i}", scope.agent),
                        }],
                        usage: Usage {
                            output_tokens: 10,
                            ..Usage::default()
                        },
                    }),
                )
                .await?;
        }
        Ok(TurnReport {
            text: format!("{} is done", scope.agent),
            usage: Usage {
                output_tokens: u64::from(self.turns) * 10,
                ..Usage::default()
            },
            turns: self.turns,
        })
    }
}

/// A runner that needs to ask a person something and has nobody to ask.
struct NeedsAnAnswer;

#[async_trait]
impl TurnRunner for NeedsAnAnswer {
    async fn run(
        &self,
        _lease: &BranchLease,
        scope: &AgentScope,
        _input: UserInput,
    ) -> Result<TurnReport, SubAgentError> {
        Err(SubAgentError::CannotPrompt {
            agent: scope.agent.clone(),
            what: "write(./src/main.rs)".to_owned(),
        })
    }
}

// The runner needs the store, and `TurnRunner` deliberately does not hand it
// one: only the lease. A task-local keeps the test honest about that.
tokio::task_local! {
    static STORE: Arc<MemoryStore>;
}

struct World {
    store: Arc<MemoryStore>,
    session: SessionId,
    root: orrery_proto::BranchId,
    at: TurnId,
}

async fn world() -> World {
    let store = Arc::new(MemoryStore::default());
    let session = store.create("/ws", "default").await.expect("create");
    let root = store.open(session).await.expect("open").root;
    let lease = store.lease(root).await.expect("the root lease");
    let at = store
        .append(
            &lease,
            NewTurn::new(TurnKind::User {
                input: UserInput::text("refactor the store"),
            }),
        )
        .await
        .expect("a first turn");
    drop(lease);
    World {
        store,
        session,
        root,
        at,
    }
}

#[tokio::test]
async fn grant_is_intersected() {
    let w = world().await;
    // The child asks for more than the parent has: a spawn aspect the parent
    // never carried, a third tool, and consent it was not given.
    let greedy = AgentDefinition::new("worker", budget())
        .asking(GrantSpec {
            capabilities: Some(vec![
                Capability::all(Aspect::Read),
                Capability::all(Aspect::Spawn),
                Capability::scoped(Aspect::Tool, ["fs.read", "fs.write", "shell.exec"]),
            ]),
            consent: Some(Consent::Always),
        })
        .seeing(["fs.read", "shell.exec"]);

    let parent_lease = w.store.lease(w.root).await.expect("the parent's lease");
    let result = STORE
        .scope(
            Arc::clone(&w.store),
            spawn(
                &*w.store,
                &parent_lease,
                w.at,
                &greedy,
                &parent_scope(w.root),
                UserInput::text("go"),
                &Chatty { turns: 2 },
            ),
        )
        .await
        .expect("it runs");

    let got = result.scope;
    assert!(
        !got.grant
            .capabilities
            .iter()
            .any(|c| c.aspect == Aspect::Spawn),
        "it cannot do what the parent could not: {:?}",
        got.grant
    );
    let tools = got
        .grant
        .capabilities
        .iter()
        .find(|c| c.aspect == Aspect::Tool)
        .expect("narrowed, not dropped");
    assert_eq!(
        tools.scope,
        vec!["fs.read".to_owned(), "fs.write".to_owned()],
        "the intersection, not the union"
    );
    assert_eq!(
        got.grant.consent,
        Consent::Once,
        "consent takes the minimum"
    );
    assert_eq!(
        got.tools,
        vec!["fs.read".to_owned()],
        "a tool the parent could not see is not added by a child"
    );
}

#[test]
fn budget_is_mandatory() {
    // An `AgentDefinition` without a budget fails to deserialize. An agent that
    // cannot terminate is a cost incident, so there is no default to fall back
    // on.
    let err = serde_json::from_value::<AgentDefinition>(serde_json::json!({
        "name": "worker",
        "prompt": "do the thing"
    }))
    .expect_err("no budget, no agent");
    assert!(err.to_string().contains("budget"), "{err}");

    // The same definition with one loads.
    let ok: AgentDefinition = serde_json::from_value(serde_json::json!({
        "name": "worker",
        "budget": { "max_turns": 4, "max_tokens": 20000, "wall_clock_ms": 60000 }
    }))
    .expect("with a budget it loads");
    assert_eq!(ok.budget.max_turns, 4);

    // And from TOML, which is how a person writes one.
    let err = toml::from_str::<AgentDefinition>("name = \"worker\"\nprompt = \"go\"\n")
        .expect_err("no budget, no agent");
    assert!(err.to_string().contains("budget"), "{err}");
}

#[tokio::test]
async fn runs_on_a_branch() {
    let w = world().await;
    let def = AgentDefinition::new("worker", budget());
    let parent_lease = w.store.lease(w.root).await.expect("the parent's lease");

    let result = STORE
        .scope(
            Arc::clone(&w.store),
            spawn(
                &*w.store,
                &parent_lease,
                w.at,
                &def,
                &parent_scope(w.root),
                UserInput::text("go"),
                &Chatty { turns: 3 },
            ),
        )
        .await
        .expect("it runs");

    // The child's turns are ordinary rows on its own branch — inspectable, and
    // not collapsed into one tool result.
    let rows = w.store.rows(result.branch);
    assert_eq!(rows.len(), 4, "one user turn and three assistant turns");
    assert!(matches!(rows[0].kind, TurnKind::User { .. }));
    assert!(
        rows[1..]
            .iter()
            .all(|r| matches!(r.kind, TurnKind::Assistant { .. })),
        "ordinary turns in the tree"
    );
    assert_eq!(rows[1].seq, orrery_proto::Seq(2), "contiguous, from one");
    assert!(w.store.is_closed(result.branch), "and the branch is closed");

    // The session knows about the branch, so a client can walk to it.
    let handle = w.store.open(w.session).await.expect("open");
    assert!(handle.branches.contains(&result.branch));
}

#[tokio::test]
async fn parent_merges_at_the_join() {
    // Cross-reference plan 02 task 8. The parent holds its own lease for the
    // whole call; the child is forked, leased, run and closed underneath it.
    // If closing the child took the parent's lease, this would hang — so it
    // runs under a timeout rather than on trust.
    let w = world().await;
    let def = AgentDefinition::new("worker", budget());
    let parent_lease = w.store.lease(w.root).await.expect("the parent's lease");

    let result = tokio::time::timeout(
        Duration::from_secs(5),
        STORE.scope(
            Arc::clone(&w.store),
            spawn(
                &*w.store,
                &parent_lease,
                w.at,
                &def,
                &parent_scope(w.root),
                UserInput::text("go"),
                &Chatty { turns: 1 },
            ),
        ),
    )
    .await
    .expect("no deadlock: the child never reaches for the parent's lease")
    .expect("it runs");

    // The join row is on the **parent's** branch, written by the parent.
    let parent_rows = w.store.rows(w.root);
    let join = parent_rows
        .iter()
        .find(|r| r.id == result.join)
        .expect("a BranchResult row on the parent");
    assert_eq!(join.branch, w.root);
    match &join.kind {
        TurnKind::BranchResult { child, outcome } => {
            assert_eq!(*child, result.branch);
            assert!(matches!(outcome, BranchOutcome::Completed { .. }));
        }
        other => panic!("a branch result: {other:?}"),
    }
    // And nothing the child wrote landed on the parent.
    assert!(
        !w.store
            .rows(result.branch)
            .iter()
            .any(|r| r.branch == w.root),
        "a child never appends to its parent"
    );
    // The parent still holds its lease afterwards and can carry on.
    assert_eq!(parent_lease.branch(), w.root);
    w.store
        .append(
            &parent_lease,
            NewTurn::new(TurnKind::User {
                input: UserInput::text("and now this"),
            }),
        )
        .await
        .expect("the parent is still mid-turn and still holds its lease");
}

#[tokio::test]
async fn cannot_prompt_fails_typed() {
    let w = world().await;
    let def = AgentDefinition::new("worker", budget());
    let parent_lease = w.store.lease(w.root).await.expect("the parent's lease");

    let err = STORE
        .scope(
            Arc::clone(&w.store),
            spawn(
                &*w.store,
                &parent_lease,
                w.at,
                &def,
                &parent_scope(w.root),
                UserInput::text("go"),
                &NeedsAnAnswer,
            ),
        )
        .await
        .expect_err("it cannot ask, and says so");

    // A typed value the parent can handle — not an auto-denial that stalls.
    match &err {
        SubAgentError::CannotPrompt { agent, what } => {
            assert_eq!(agent, "worker");
            assert_eq!(what, "write(./src/main.rs)");
        }
        other => panic!("a typed refusal: {other:?}"),
    }
    assert!(err.to_string().contains("nobody attached"), "{err}");

    // And the tree is in the same shape a success leaves it in: the child is
    // closed and the parent's join row says why.
    let parent_rows = w.store.rows(w.root);
    let join = parent_rows
        .iter()
        .find_map(|r| match &r.kind {
            TurnKind::BranchResult { child, outcome } => Some((*child, outcome.clone())),
            _ => None,
        })
        .expect("the parent wrote a join row anyway");
    assert!(w.store.is_closed(join.0));
    match join.1 {
        BranchOutcome::Failed { code, .. } => assert_eq!(code, "cannot-prompt"),
        other => panic!("{other:?}"),
    }
}
