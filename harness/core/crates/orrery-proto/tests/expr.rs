//! `Expr`, `Predicate`, `Verdict` and `LoadOutcome`.

mod expr {
    use orrery_proto::{CmpOp, Expr, Predicate};

    #[test]
    fn ref_and_literal_are_distinguishable() {
        let as_ref: Expr = serde_json::from_value(serde_json::json!({ "ref": "step1" })).unwrap();
        assert_eq!(
            as_ref,
            Expr::Ref {
                r#ref: "step1".into(),
                path: None
            }
        );

        let with_path: Expr =
            serde_json::from_value(serde_json::json!({ "ref": "step1", "path": ["out", "0"] }))
                .unwrap();
        assert_eq!(
            with_path,
            Expr::Ref {
                r#ref: "step1".into(),
                path: Some(vec!["out".into(), "0".into()])
            }
        );

        let literal: Expr = serde_json::from_value(serde_json::json!({ "a": 1 })).unwrap();
        assert_eq!(literal, Expr::Literal(serde_json::json!({ "a": 1 })));

        let scalar: Expr = serde_json::from_value(serde_json::json!(7)).unwrap();
        assert_eq!(scalar, Expr::Literal(serde_json::json!(7)));

        for value in [
            serde_json::json!({ "ref": "step1" }),
            serde_json::json!({ "a": 1 }),
            serde_json::json!("plain"),
        ] {
            let parsed: Expr = serde_json::from_value(value.clone()).unwrap();
            assert_eq!(
                serde_json::to_value(&parsed).unwrap(),
                value,
                "untagged must round-trip"
            );
        }
    }

    #[test]
    fn predicates_nest() {
        let raw = serde_json::json!({
            "all": [
                { "cmp": { "lhs": { "ref": "step1" }, "op": "eq", "rhs": true } },
                { "not": { "any": [] } }
            ]
        });
        let predicate: Predicate = serde_json::from_value(raw.clone()).unwrap();
        assert_eq!(serde_json::to_value(&predicate).unwrap(), raw);

        let Predicate::All(inner) = &predicate else {
            panic!("expected `all`")
        };
        assert_eq!(inner.len(), 2);
        let Predicate::Cmp { op, .. } = &inner[0] else {
            panic!("expected `cmp`")
        };
        assert_eq!(*op, CmpOp::Eq);
    }
}

mod verdict {
    use orrery_proto::{Outcome, Verdict};

    #[test]
    fn is_generic_over_its_payload() {
        // The point of the type parameter: a tool input cannot be rewritten
        // into a model request, because they are different `P`s.
        let rewrite: Verdict<serde_json::Value> =
            Verdict::Rewrite(serde_json::json!({ "path": "safe.rs" }));
        assert!(matches!(rewrite, Verdict::Rewrite(_)));

        let handled: Verdict<serde_json::Value> = Verdict::Handled {
            result: Outcome::ok(),
        };
        assert!(matches!(handled, Verdict::Handled { .. }));

        let denied: Verdict<()> = Verdict::Deny {
            reason: "no".into(),
        };
        assert!(
            format!("{denied:?}").contains("Deny"),
            "Verdict must be Debug"
        );

        let carry_on: Verdict<()> = Verdict::Continue;
        assert!(matches!(carry_on, Verdict::Continue));
    }
}

mod load {
    use orrery_proto::{Contribution, ContributionKind, LoadOutcome, LoadStage, SkipReason};

    #[test]
    fn outcome_variants() {
        let degraded: LoadOutcome = serde_json::from_value(serde_json::json!({
            "status": "degraded",
            "ext": "buildgraph",
            "ms": 42,
            "contributions": [{ "kind": "tool", "name": "graph" }],
            "problems": ["the lsp binary is missing; symbol search is off"]
        }))
        .unwrap();
        let LoadOutcome::Degraded {
            contributions,
            problems,
            ..
        } = &degraded
        else {
            panic!("expected `degraded`");
        };
        assert_eq!(
            contributions[0],
            Contribution {
                kind: ContributionKind::Tool,
                name: "graph".into()
            }
        );
        assert_eq!(problems.len(), 1);

        let ok: LoadOutcome = serde_json::from_value(serde_json::json!({
            "status": "ok", "ext": "builtin", "ms": 1, "contributions": []
        }))
        .unwrap();
        assert!(matches!(ok, LoadOutcome::Ok { .. }));
    }

    #[test]
    fn skipped_requires_a_reason_from_the_closed_set() {
        let skipped: LoadOutcome = serde_json::from_value(serde_json::json!({
            "status": "skipped", "ext": "buildgraph", "reason": "disabled"
        }))
        .unwrap();
        let LoadOutcome::Skipped { reason, .. } = &skipped else {
            panic!("expected `skipped`")
        };
        assert_eq!(*reason, SkipReason::Disabled);

        assert!(
            serde_json::from_value::<LoadOutcome>(serde_json::json!({
                "status": "skipped", "ext": "buildgraph"
            }))
            .is_err(),
            "`skipped` without a reason must not parse"
        );
        assert!(
            serde_json::from_value::<LoadOutcome>(serde_json::json!({
                "status": "skipped", "ext": "buildgraph", "reason": "felt like it"
            }))
            .is_err(),
            "the reason set is closed"
        );
    }

    #[test]
    fn failed_names_the_stage() {
        let failed: LoadOutcome = serde_json::from_value(serde_json::json!({
            "status": "failed", "ext": "buildgraph", "stage": "activate",
            "message": "panicked while starting"
        }))
        .unwrap();
        let LoadOutcome::Failed { stage, .. } = &failed else {
            panic!("expected `failed`")
        };
        assert_eq!(*stage, LoadStage::Activate);
    }
}

mod contribution_kinds {
    use orrery_proto::ContributionKind;

    /// Every kind an extension manifest can declare has a wire name, and the
    /// six that plan 05 needs — an agent, a workflow, an interceptor, a
    /// lifecycle handler, a permission handler, an MCP server — are among
    /// them. Without these the kernel can register an interceptor and the
    /// ledger cannot say so.
    #[test]
    fn every_provides_field_has_a_kind() {
        let expected = [
            (ContributionKind::Tool, "tool"),
            (ContributionKind::Provider, "provider"),
            (ContributionKind::Renderer, "renderer"),
            (ContributionKind::Command, "command"),
            (ContributionKind::Skill, "skill"),
            (ContributionKind::Memory, "memory"),
            (ContributionKind::Grader, "grader"),
            (ContributionKind::Router, "router"),
            (ContributionKind::SessionStore, "session-store"),
            (ContributionKind::Mode, "mode"),
            (ContributionKind::View, "view"),
            (ContributionKind::Agent, "agent"),
            (ContributionKind::Workflow, "workflow"),
            (ContributionKind::Interceptor, "interceptor"),
            (ContributionKind::Lifecycle, "lifecycle"),
            (ContributionKind::Permissions, "permissions"),
            (ContributionKind::Mcp, "mcp"),
        ];
        for (kind, wire) in expected {
            assert_eq!(
                serde_json::to_value(kind).unwrap(),
                serde_json::Value::String(wire.to_owned()),
                "{kind:?} must serialise as `{wire}`"
            );
            assert_eq!(
                serde_json::from_value::<ContributionKind>(serde_json::json!(wire)).unwrap(),
                kind
            );
        }
    }
}
