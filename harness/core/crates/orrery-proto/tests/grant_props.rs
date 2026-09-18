//! `Grant` narrowing is the one operation ever applied to a grant, so it is
//! property-tested rather than example-tested.

use orrery_proto::{Aspect, Budget, Capability, Consent, Grant, GrantSpec, TokenBudget, Usage};
use proptest::prelude::*;

const ASPECTS: [Aspect; 13] = [
    Aspect::Tool,
    Aspect::Mcp,
    Aspect::Skill,
    Aspect::Ext,
    Aspect::Mode,
    Aspect::Read,
    Aspect::Write,
    Aspect::Spawn,
    Aspect::Net,
    Aspect::Creds,
    Aspect::Ui,
    Aspect::Render,
    Aspect::MemRead,
];

fn any_aspect() -> impl Strategy<Value = Aspect> {
    prop::sample::select(ASPECTS.as_slice())
}

fn any_scope() -> impl Strategy<Value = Vec<String>> {
    prop::collection::vec(
        prop::sample::select(vec!["a", "b", "c", "d"]).prop_map(str::to_owned),
        0..4,
    )
}

fn any_capability() -> impl Strategy<Value = Capability> {
    (any_aspect(), any_scope()).prop_map(|(aspect, scope)| Capability { aspect, scope })
}

fn any_consent() -> impl Strategy<Value = Consent> {
    prop::sample::select(vec![Consent::Always, Consent::Once, Consent::Never])
}

fn any_grant() -> impl Strategy<Value = Grant> {
    (prop::collection::vec(any_capability(), 0..5), any_consent()).prop_map(
        |(capabilities, consent)| Grant {
            capabilities,
            consent,
        },
    )
}

/// `empty` scope means "unqualified", i.e. the whole aspect.
fn covers(grant: &Grant, aspect: Aspect, scope: &str) -> bool {
    grant
        .capabilities
        .iter()
        .filter(|c| c.aspect == aspect)
        .any(|c| c.scope.is_empty() || c.scope.iter().any(|s| s == scope))
}

proptest! {
    #[test]
    fn intersect_never_widens(a in any_grant(), b in any_grant()) {
        let got = a.intersect_exact(&b);
        for cap in &got.capabilities {
            prop_assert!(
                a.capabilities.iter().any(|c| c.aspect == cap.aspect)
                    && b.capabilities.iter().any(|c| c.aspect == cap.aspect),
                "aspect {:?} appeared out of nowhere", cap.aspect
            );
            for scope in &cap.scope {
                prop_assert!(covers(&a, cap.aspect, scope), "scope widened past a");
                prop_assert!(covers(&b, cap.aspect, scope), "scope widened past b");
            }
        }
        prop_assert_eq!(got.consent, a.consent.min(b.consent));
    }

    #[test]
    fn intersect_is_commutative(a in any_grant(), b in any_grant()) {
        prop_assert_eq!(a.intersect_exact(&b), b.intersect_exact(&a));
    }

    #[test]
    fn intersect_is_idempotent(a in any_grant()) {
        let once = a.intersect_exact(&a);
        prop_assert_eq!(once.intersect_exact(&once.clone()), once);
    }

    #[test]
    fn spec_apply_never_widens(parent in any_grant(), spec_consent in any_consent()) {
        let spec = GrantSpec { capabilities: None, consent: Some(spec_consent) };
        let got = spec.apply_to(&parent);
        prop_assert_eq!(got.consent, parent.consent.min(spec_consent));
        prop_assert_eq!(got.capabilities, parent.capabilities);
    }
}

#[test]
fn consent_orders_never_below_always() {
    assert_eq!(Consent::Never.min(Consent::Always), Consent::Never);
    assert_eq!(Consent::Once.min(Consent::Always), Consent::Once);
    assert_eq!(Consent::Always.min(Consent::Always), Consent::Always);
}

#[test]
fn aspect_renames_are_pinned() {
    let cases = [
        (Aspect::Tool, "tool"),
        (Aspect::Mcp, "mcp"),
        (Aspect::Skill, "skill"),
        (Aspect::Ext, "ext"),
        (Aspect::Mode, "mode"),
        (Aspect::Read, "read"),
        (Aspect::Write, "write"),
        (Aspect::Spawn, "spawn"),
        (Aspect::Net, "net"),
        (Aspect::Creds, "creds"),
        (Aspect::Ui, "ui"),
        (Aspect::Render, "render"),
        (Aspect::MemRead, "mem.read"),
        (Aspect::MemWrite, "mem.write"),
    ];
    for (aspect, wire) in cases {
        assert_eq!(
            serde_json::to_value(aspect).unwrap(),
            serde_json::json!(wire)
        );
        assert_eq!(
            serde_json::from_value::<Aspect>(serde_json::json!(wire)).unwrap(),
            aspect
        );
    }
}

#[test]
fn unqualified_is_wider_than_scoped() {
    let wide = Grant {
        capabilities: vec![Capability {
            aspect: Aspect::Read,
            scope: vec![],
        }],
        consent: Consent::Always,
    };
    let narrow = Grant {
        capabilities: vec![Capability {
            aspect: Aspect::Read,
            scope: vec!["src".into()],
        }],
        consent: Consent::Once,
    };
    let got = wide.intersect_exact(&narrow);
    assert_eq!(got.capabilities, narrow.capabilities);
    assert_eq!(got.consent, Consent::Once);
}

#[test]
fn disjoint_scopes_drop_the_capability() {
    let a = Grant {
        capabilities: vec![Capability {
            aspect: Aspect::Read,
            scope: vec!["src".into()],
        }],
        consent: Consent::Always,
    };
    let b = Grant {
        capabilities: vec![Capability {
            aspect: Aspect::Read,
            scope: vec!["docs".into()],
        }],
        consent: Consent::Always,
    };
    assert!(a.intersect_exact(&b).capabilities.is_empty());
}

#[test]
fn usage_accumulates_saturating() {
    let mut a = Usage {
        input_tokens: u64::MAX,
        output_tokens: 1,
        cache_hits: 0,
        micro_usd: Some(u64::MAX),
    };
    a += Usage {
        input_tokens: 10,
        output_tokens: 1,
        cache_hits: 3,
        micro_usd: Some(10),
    };
    assert_eq!(
        a.input_tokens,
        u64::MAX,
        "accumulation must saturate, not wrap"
    );
    assert_eq!(a.output_tokens, 2);
    assert_eq!(a.cache_hits, 3);
    assert_eq!(a.micro_usd, Some(u64::MAX));

    let mut none = Usage::default();
    none += Usage {
        micro_usd: Some(5),
        ..Usage::default()
    };
    assert_eq!(none.micro_usd, Some(5));
    none += Usage::default();
    assert_eq!(none.micro_usd, Some(5));
}

#[test]
fn budget_has_no_floats() {
    let budget = Budget {
        max_turns: 8,
        max_tokens: 100_000,
        wall_clock_ms: 60_000,
        max_micro_usd: Some(250_000),
    };
    let json = serde_json::to_value(budget).unwrap();
    assert_eq!(json["max_micro_usd"], serde_json::json!(250_000));
    assert!(json["max_micro_usd"].is_u64());

    let clamp = TokenBudget {
        max: 1000,
        reserve: 200,
    };
    assert_eq!(clamp.available(), 800);
    assert_eq!(
        TokenBudget {
            max: 100,
            reserve: 500
        }
        .available(),
        0
    );
}
