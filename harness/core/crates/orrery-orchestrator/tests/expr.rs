//! Task 5 · the evaluator. Data in, data out, and no way to call out.

use orrery_orchestrator::expr::{Env, ExprError, eval, holds};
use orrery_proto::{CmpOp, Expr, Predicate};
use serde_json::{Value, json};

fn env() -> Env {
    let mut env = Env::new();
    env.bind("plan", json!({ "files": ["a.rs", "b.rs"], "count": 2 }));
    env.bind("gate", json!({ "passed": false }));
    env
}

/// A **review-level assertion, pinned as a test**: the evaluator has no I/O in
/// its signature.
///
/// `eval` coerces to a plain `fn` pointer, which an `async fn` does not and a
/// closure capturing a handle cannot. Its only argument besides the expression
/// is an [`Env`], and an `Env` round-trips through JSON — so there is no socket,
/// no store, no `&dyn` anything in there either. A step input therefore cannot
/// read a file, reach a host or ask a model.
#[test]
fn cannot_call_out() {
    const EVAL: fn(&Expr, &Env) -> Result<Value, ExprError> = eval;
    const HOLDS: fn(&Predicate, &Env) -> Result<bool, ExprError> = holds;
    let value = EVAL(&Expr::Literal(json!(1)), &Env::new()).expect("a literal");
    assert_eq!(value, json!(1));
    assert!(!HOLDS(&Predicate::Any(Vec::new()), &Env::new()).expect("an empty any is false"));

    // The environment is data: it serialises, so nothing live can live in it.
    let round_trip: Env =
        serde_json::from_str(&serde_json::to_string(&env()).expect("serialises")).expect("parses");
    assert_eq!(round_trip, env());

    // And it is Send + Sync + 'static, so it cannot be hiding a borrow of one.
    const fn assert_data<T: Send + Sync + 'static>() {}
    assert_data::<Env>();
}

#[test]
fn a_ref_reads_an_earlier_step() {
    let env = env();
    assert_eq!(
        eval(
            &Expr::Ref {
                r#ref: "plan".to_owned(),
                path: Some(vec!["count".to_owned()]),
            },
            &env
        )
        .expect("a declared path"),
        json!(2)
    );
    assert_eq!(
        eval(
            &Expr::Ref {
                r#ref: "plan".to_owned(),
                path: None,
            },
            &env
        )
        .expect("the whole value"),
        json!({ "files": ["a.rs", "b.rs"], "count": 2 })
    );
}

#[test]
fn a_missing_step_or_key_is_a_typed_error() {
    let env = env();
    let err = eval(
        &Expr::Ref {
            r#ref: "nowhere".to_owned(),
            path: None,
        },
        &env,
    )
    .expect_err("it has not run");
    assert!(matches!(err, ExprError::NoSuchStep { .. }), "{err:?}");
    assert!(
        err.to_string().contains("plan"),
        "it says what has run: {err}"
    );

    let err = eval(
        &Expr::Ref {
            r#ref: "plan".to_owned(),
            path: Some(vec!["nope".to_owned()]),
        },
        &env,
    )
    .expect_err("no such key");
    assert!(matches!(err, ExprError::NoSuchKey { .. }), "{err:?}");
}

#[test]
fn predicates_compare_declared_values() {
    let env = env();
    let count_is_two = Predicate::Cmp {
        lhs: Expr::Ref {
            r#ref: "plan".to_owned(),
            path: Some(vec!["count".to_owned()]),
        },
        op: CmpOp::Eq,
        rhs: Expr::Literal(json!(2)),
    };
    assert!(holds(&count_is_two, &env).expect("comparable"));
    assert!(!holds(&Predicate::Not(Box::new(count_is_two.clone())), &env).expect("negated"));
    assert!(holds(&Predicate::All(vec![count_is_two.clone()]), &env).expect("all"));
    assert!(holds(&Predicate::All(Vec::new()), &env).expect("an empty all is true"));
    assert!(!holds(&Predicate::Any(Vec::new()), &env).expect("an empty any is false"));

    let bigger = Predicate::Cmp {
        lhs: Expr::Ref {
            r#ref: "plan".to_owned(),
            path: Some(vec!["count".to_owned()]),
        },
        op: CmpOp::Gt,
        rhs: Expr::Literal(json!(5)),
    };
    assert!(!holds(&bigger, &env).expect("comparable"));

    let contains = Predicate::Cmp {
        lhs: Expr::Ref {
            r#ref: "plan".to_owned(),
            path: Some(vec!["files".to_owned()]),
        },
        op: CmpOp::Contains,
        rhs: Expr::Literal(json!("a.rs")),
    };
    assert!(holds(&contains, &env).expect("comparable"));
}

#[test]
fn two_things_that_do_not_compare_are_an_error_not_a_false() {
    let err = holds(
        &Predicate::Cmp {
            lhs: Expr::Literal(json!("text")),
            op: CmpOp::Lt,
            rhs: Expr::Literal(json!(2)),
        },
        &Env::new(),
    )
    .expect_err("text is not less than a number");
    assert!(matches!(err, ExprError::NotComparable { .. }), "{err:?}");
}
