//! The shipped roles load like any other bundle.
//!
//! Through [`orrery_ext_api::testing::load_for_test`] — the published
//! mock-broker harness `orrery ext test` runs, and the one plan 18 points a
//! community author at. An extension's own tests never reach for the host:
//! `cargo xtask deps-check` rule 3 forbids it, and the point of the exercise is
//! that the door a third party is given actually works on the case that most
//! tempts a shortcut.

use orrery_ext_agents_default::{DefaultAgents, MANIFEST, ModeWord, ModelClass, agents, promised};
use orrery_ext_api::testing::load_for_test;
use orrery_proto::{Aspect, Consent, Contribution, ContributionKind, ExtId, LoadOutcome, Role};

fn id() -> ExtId {
    ExtId::new("agents-default").expect("a valid ext id")
}

#[test]
fn ships_as_an_extension() {
    // It asks for nothing of its own — a role agent runs turns through the
    // kernel — so no grants at all are enough and the load is clean.
    let harness = load_for_test(MANIFEST, &[])
        .expect("its own orrery.toml parses with the parser a third party is held to");
    assert_eq!(harness.manifest().name, id());

    let outcome = harness.load_outcome();
    let LoadOutcome::Ok { contributions, .. } = &outcome else {
        panic!("a role bundle needs no capabilities, so it loads clean: {outcome:?}");
    };

    // The ledger names all five, as agents.
    for name in promised() {
        assert!(
            contributions.contains(&Contribution {
                kind: ContributionKind::Agent,
                name: name.clone(),
            }),
            "`{name}` is contributed: {contributions:?}"
        );
    }
    let listed: Vec<String> = contributions
        .iter()
        .filter(|c| c.kind == ContributionKind::Agent)
        .map(|c| c.name.clone())
        .collect();
    assert_eq!(
        listed,
        promised(),
        "in the order the manifest promises them"
    );

    // And nothing was asked of the broker.
    assert!(harness.recorded().is_empty());
}

#[test]
fn the_loaded_bundle_has_agents_behind_the_manifest() {
    let shipped = DefaultAgents.agents();
    assert_eq!(shipped.len(), 5, "not a manifest with nothing behind it");

    let planner = shipped
        .iter()
        .find(|a| a.role == Role::Planner)
        .expect("a planner");
    assert_eq!(planner.mode, ModeWord::Plan, "a planner plans");
    assert_eq!(planner.model, ModelClass::Large);
    let asked = planner.grant.capabilities.as_ref().expect("it narrows");
    assert!(
        !asked
            .iter()
            .any(|c| matches!(c.aspect, Aspect::Write | Aspect::Spawn | Aspect::MemWrite)),
        "read-only: {asked:?}"
    );

    let executor = shipped
        .iter()
        .find(|a| a.role == Role::Executor)
        .expect("an executor");
    assert_eq!(executor.mode, ModeWord::Execute);
    assert!(
        executor.grant.capabilities.is_none(),
        "the granted set is the step's; an omitted field inherits rather than narrowing"
    );

    let verifier = shipped
        .iter()
        .find(|a| a.role == Role::Verifier)
        .expect("a verifier");
    assert_eq!(verifier.mode, ModeWord::Review, "review never writes");
    let asked = verifier.grant.capabilities.as_ref().expect("it narrows");
    assert!(
        asked.iter().any(|c| c.aspect == Aspect::Spawn),
        "a verifier that cannot run the tests is not a verifier: {asked:?}"
    );
    assert!(!asked.iter().any(|c| c.aspect == Aspect::Write));

    // Compaction and summarising are mechanical, so they ask for the cheap
    // model — which is the whole "plan with a large model, compact with a cheap
    // one" claim, as configuration rather than as a branch in the loop.
    for role in [Role::Compactor, Role::Summariser] {
        let agent = shipped.iter().find(|a| a.role == role).expect("shipped");
        assert_eq!(agent.model, ModelClass::Cheap, "{}", agent.name);
        assert_eq!(agent.budget.max_turns, 1, "one pass is all it takes");
    }
}

#[test]
fn nothing_grants_itself_silence() {
    for agent in agents() {
        if let Some(consent) = agent.grant.consent {
            assert_eq!(
                consent,
                Consent::Once,
                "`{}` asks rather than helping itself",
                agent.name
            );
        }
    }
}

#[test]
fn no_model_id_is_hard_coded() {
    // A class, not a name: what `cheap` means is a profile's business and it
    // changes every few months.
    for agent in agents() {
        assert!(
            ["default", "large", "cheap"].contains(&agent.model.as_str()),
            "`{}` names a class",
            agent.name
        );
        assert!(
            !agent.prompt.contains("claude") && !agent.prompt.contains("gpt"),
            "`{}` does not name a model either",
            agent.name
        );
    }
}
