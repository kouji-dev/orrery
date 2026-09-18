//! Task 8 · role binding. One winner, named losers, and never a widening.
//!
//! `roles::phases_fire_in_every_step` is **not** here: phases fire where steps
//! run, and this crate deliberately cannot reach the loop. It lives in
//! `orrery-orchestrator/tests/roles.rs`.

use orrery_audit::AuditEvent;
use orrery_proto::{
    AgentScope, Aspect, BranchId, Capability, Consent, Grant, GrantSpec, Layer, Role,
};
use orrery_router::{Bindings, RoleAgentDef, RoleOffer, bind_scope, shadowed_line};

fn agents(names: &[&str]) -> Vec<String> {
    names.iter().map(|s| (*s).to_owned()).collect()
}

#[test]
fn missing_binding_fails_at_session_start() {
    let err = Bindings::resolve(
        [RoleOffer::new(
            Role::Planner,
            "architect",
            Layer::Workspace,
            "config.toml",
            14,
        )],
        &agents(&["executor"]),
    )
    .expect_err("it fails at startup, not mid-turn");

    assert_eq!(err.file, "config.toml");
    assert_eq!(err.line, 14);
    assert_eq!(err.agent, "architect");
    assert_eq!(err.role, "planner");
    assert!(err.to_string().starts_with("config.toml:14:"), "{err}");

    // The same binding with the agent defined loads.
    Bindings::resolve(
        [RoleOffer::new(
            Role::Planner,
            "architect",
            Layer::Workspace,
            "config.toml",
            14,
        )],
        &agents(&["architect"]),
    )
    .expect("it loads");

    // A namespaced name resolves against the extension that provides it.
    Bindings::resolve(
        [RoleOffer::new(
            Role::Planner,
            "agents-default.planner",
            Layer::Managed,
            "managed.toml",
            2,
        )],
        &agents(&["planner"]),
    )
    .expect("`ext.agent` resolves to `agent`");
}

#[test]
fn exactly_one_wins() {
    // Two extensions offering a planner, from two layers.
    let from_org = RoleOffer::new(Role::Planner, "acme.planner", Layer::Org, "org.toml", 3);
    let from_project = RoleOffer::new(
        Role::Planner,
        "myteam.planner",
        Layer::Project,
        "project.toml",
        7,
    );
    let bindings = Bindings::resolve(
        [from_org.clone(), from_project.clone()],
        &agents(&["planner"]),
    )
    .expect("both agents exist");

    let winner = bindings.of(Role::Planner).expect("one binding wins");
    assert_eq!(winner.agent, "myteam.planner", "the closest layer wins");
    assert_eq!(winner.layer, Layer::Project);

    // And the ledger names the loser.
    assert_eq!(bindings.shadowed().len(), 1);
    let loser = &bindings.shadowed()[0];
    assert_eq!(loser.agent, "acme.planner");
    let line = shadowed_line(winner, loser);
    assert!(line.contains("org.toml:3"), "{line}");
    assert!(line.contains("does not take effect"), "{line}");

    // The audit records which one ran.
    let events = bindings.audit_events();
    assert_eq!(events.len(), 1);
    let AuditEvent::RoutingDecision { chose, signals } = &events[0] else {
        panic!("{:?}", events[0]);
    };
    assert_eq!(chose, "role:planner=myteam.planner");
    assert_eq!(signals.get("layer"), Some(&4.0));

    // Order does not change the answer: resolution is by layer, not by luck.
    let other_way =
        Bindings::resolve([from_project, from_org], &agents(&["planner"])).expect("resolves");
    assert_eq!(
        other_way.of(Role::Planner).expect("bound").agent,
        "myteam.planner"
    );
}

#[test]
fn binding_never_widens() {
    // The step may read and write, and see two tools.
    let step = AgentScope {
        agent: "main".to_owned(),
        branch: BranchId::new(),
        tools: vec!["fs.read".to_owned(), "fs.write".to_owned()],
        grant: Grant {
            capabilities: vec![
                Capability::all(Aspect::Read),
                Capability::scoped(Aspect::Tool, ["fs.read", "fs.write"]),
            ],
            consent: Consent::Once,
        },
    };

    // The bound agent asks for more than the step has: a write aspect the step
    // never carried, a third tool, and consent it was not given.
    let greedy = RoleAgentDef::named("planner")
        .asking(GrantSpec {
            capabilities: Some(vec![
                Capability::all(Aspect::Read),
                Capability::all(Aspect::Spawn),
                Capability::scoped(Aspect::Tool, ["fs.read", "fs.write", "shell.exec"]),
            ]),
            consent: Some(Consent::Always),
        })
        .seeing(["fs.read", "shell.exec"]);

    let bound = bind_scope(&step, &greedy);

    assert_eq!(bound.agent, "planner");
    assert_eq!(bound.branch, step.branch, "same step, same branch");
    assert!(
        !bound
            .grant
            .capabilities
            .iter()
            .any(|c| c.aspect == Aspect::Spawn),
        "an aspect the step never had cannot appear: {:?}",
        bound.grant
    );
    let tools_cap = bound
        .grant
        .capabilities
        .iter()
        .find(|c| c.aspect == Aspect::Tool)
        .expect("the tool aspect survives, narrowed");
    assert_eq!(
        tools_cap.scope,
        vec!["fs.read".to_owned(), "fs.write".to_owned()],
        "the intersection, not the union"
    );
    assert_eq!(
        bound.grant.consent,
        Consent::Once,
        "consent takes the minimum: a child cannot promote itself to never asking"
    );
    assert_eq!(
        bound.tools,
        vec!["fs.read".to_owned()],
        "a tool the step could not see is not added by a binding"
    );

    // And an agent that asks for nothing inherits the step's own ceiling.
    let quiet = bind_scope(&step, &RoleAgentDef::named("executor"));
    assert_eq!(quiet.grant, step.grant);
    assert_eq!(quiet.tools, step.tools);
}

#[test]
fn router_is_unbound_by_default() {
    let bindings = Bindings::none();
    assert!(
        bindings.is_declarative(Role::Router),
        "unbound stays the default, because eval comparison needs reproducibility"
    );

    // Binding it is the same mechanism, not a new one.
    let bound = Bindings::resolve(
        [RoleOffer::new(
            Role::Router,
            "chooser",
            Layer::User,
            "user.toml",
            9,
        )],
        &agents(&["chooser"]),
    )
    .expect("resolves");
    assert!(!bound.is_declarative(Role::Router));
    assert_eq!(bound.of(Role::Router).expect("bound").agent, "chooser");
}
