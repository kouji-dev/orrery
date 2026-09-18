//! Task 1 · signals are cheap, and rules parse from configuration.

use orrery_proto::{Budget, Usage};
use orrery_router::rules::{Effect, Op, RuleSet, RuleVerdict, Rung};
use orrery_router::{ABSENT, GateOutcome, Mode, Signals};

/// A **compile-level** check that `Signals` contains no handle, no future and
/// nothing requiring a model call.
///
/// `Copy` is the whole assertion: `Arc`, `Box<dyn …>`, a channel, a `String`,
/// any `Future` and any store handle are all not `Copy`, so a field that would
/// let a rule reach out cannot be added without this failing to build. The
/// runtime half only checks that the documented exclusion list has not been
/// quietly emptied.
#[test]
fn signals_are_cheap() {
    const fn assert_cheap<T: Copy + Send + Sync + 'static>() {}
    assert_cheap::<Signals>();
    // And every field of it, named one by one, so that widening one of them
    // fails here and not somewhere downstream.
    assert_cheap::<Usage>();
    assert_cheap::<Budget>();
    assert_cheap::<Mode>();
    assert_cheap::<Option<GateOutcome>>();
    assert_cheap::<u32>();

    assert_eq!(ABSENT.len(), 5, "the list of what is deliberately absent");
    assert!(
        ABSENT.iter().any(|(what, _)| *what == "the transcript"),
        "reading the transcript is the temptation this list exists for"
    );
    assert!(
        ABSENT.iter().all(|(_, why)| !why.is_empty()),
        "every exclusion says why"
    );

    // The signals a rule may read are exactly the numbers `values()` produces.
    let values = Signals::default().values();
    for name in orrery_router::rules::SIGNAL_NAMES {
        if name == "last_gate" {
            continue; // absent until a gate has run.
        }
        assert!(values.contains_key(name), "`{name}` is produced");
    }
}

#[test]
fn parse_from_config() {
    let text = r#"
[[route]]
name   = "one problem is one agent"
when   = { diff_lines = "< 200", writes = ">= 1" }
deny   = "fan-out"
reason = "a diff this small does not decompose into disjoint sets"

[[route]]
name  = "a failing gate justifies a loop"
when  = { last_gate = "== 0" }
allow = "bounded-loop"
"#;
    let set = RuleSet::parse_toml(text, "config.toml").expect("it parses");
    assert_eq!(set.rules.len(), 2);

    let first = &set.rules[0];
    assert_eq!(first.name, "one problem is one agent");
    assert_eq!(first.when.len(), 2);
    // Sorted by signal name, so a TOML table's arbitrary order cannot leak out.
    assert_eq!(first.when[0].signal, "diff_lines");
    assert_eq!(first.when[0].op, Op::Lt);
    assert!((first.when[0].value - 200.0).abs() < f64::EPSILON);
    assert_eq!(first.when[1].signal, "writes");
    assert_eq!(first.when[1].op, Op::Ge);
    assert!(matches!(
        &first.effect,
        Effect::Deny { rung: Rung::FanOut, reason } if reason.contains("disjoint")
    ));
    assert!(matches!(
        set.rules[1].effect,
        Effect::Allow {
            rung: Rung::BoundedLoop
        }
    ));

    // And it fires on the signals it names.
    let signals = Signals {
        diff_lines: 12,
        writes: 3,
        ..Signals::default()
    };
    assert!(matches!(
        set.verdict(&signals, Rung::FanOut),
        RuleVerdict::Denied { .. }
    ));
    // A signal that is absent makes the condition false, never true.
    assert_eq!(
        set.verdict(&signals, Rung::BoundedLoop),
        RuleVerdict::NoRule,
        "no gate has run, so the gate rule cannot fire"
    );
    let after_a_failed_gate = Signals {
        last_gate: Some(GateOutcome::Failed),
        ..signals
    };
    assert!(matches!(
        set.verdict(&after_a_failed_gate, Rung::BoundedLoop),
        RuleVerdict::Allowed { .. }
    ));
}

#[test]
fn an_unknown_signal_names_the_file() {
    let text =
        "[[route]]\nname = \"x\"\nwhen = { model_confidence = \"> 0.8\" }\nallow = \"sub-agent\"\n";
    let err = RuleSet::parse_toml(text, "config.toml").expect_err("it fails at load");
    assert_eq!(err.file, "config.toml");
    assert!(err.to_string().contains("model_confidence"), "{err}");
    assert!(
        err.to_string().contains("diff_lines"),
        "and it says what was expected: {err}"
    );
}

#[test]
fn a_rule_that_decides_nothing_is_a_load_error() {
    let text = "[[route]]\nname = \"x\"\nwhen = { reads = \"> 1\" }\n";
    let err = RuleSet::parse_toml(text, "config.toml").expect_err("it fails at load");
    assert!(err.to_string().contains("allow"), "{err}");
    assert_eq!(err.rule.as_deref(), Some("x"));
}

#[test]
fn no_route_table_is_no_rules() {
    let set = RuleSet::parse_toml("model = \"claude-sonnet-5\"\n", "config.toml").expect("parses");
    assert!(set.rules.is_empty());
}
