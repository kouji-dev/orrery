//! Task 3 · the fan-out arithmetic. N is a minimum, never a choice.

use orrery_router::fanout::{Bound, FanOutProfile, are_disjoint, n_of, plan};
use proptest::prelude::*;

proptest! {
    /// For arbitrary units, caps and budgets, N never exceeds any of the three
    /// bounds.
    #[test]
    fn n_is_the_minimum(
        units in 0u32..64,
        cap in 0u32..64,
        remaining in 0u64..1_000_000,
        cost in prop::option::of(1u64..50_000),
    ) {
        let profile = FanOutProfile { cap, child_cost: cost };
        let got = n_of(units, &profile, remaining);

        prop_assert!(got.n <= units, "never more children than disjoint units");
        prop_assert!(got.n <= cap, "never over the profile's cap");
        if let Some(cost) = cost {
            let affordable = remaining / cost;
            prop_assert!(u64::from(got.n) <= affordable, "never more than the budget pays for");
        }

        // And it is the minimum, not merely under it.
        let mut expected = units.min(cap);
        if let Some(cost) = cost {
            expected = expected.min(u32::try_from(remaining / cost).unwrap_or(u32::MAX));
        }
        prop_assert_eq!(got.n, expected);
    }
}

#[test]
fn overlapping_inputs_collapse_to_one() {
    let profile = FanOutProfile::capped(8);
    let overlapping = vec![
        serde_json::json!(["crates/a", "crates/shared"]),
        serde_json::json!(["crates/b", "crates/shared"]),
        serde_json::json!(["crates/c"]),
    ];
    assert!(!are_disjoint(&overlapping));
    let got = plan(&overlapping, &profile, u64::MAX);
    assert_eq!(got.n, 1, "overlapping sets mean one problem and one agent");
    assert_eq!(got.bound, Bound::Overlap);

    let disjoint = vec![
        serde_json::json!(["crates/a"]),
        serde_json::json!(["crates/b"]),
        serde_json::json!(["crates/c"]),
    ];
    assert!(are_disjoint(&disjoint));
    assert_eq!(plan(&disjoint, &profile, u64::MAX).n, 3);

    // A unit that names nothing demonstrates nothing.
    assert!(!are_disjoint(&[
        serde_json::json!([]),
        serde_json::json!(["a"])
    ]));
}

#[test]
fn no_child_cost_still_bounded() {
    // No declared `child_cost`, and no budget left at all: the cap and the
    // units still bound N, and the budget bound simply does not apply.
    let profile = FanOutProfile::capped(2);
    assert!(profile.child_cost.is_none());
    let units = vec![
        serde_json::json!(["a"]),
        serde_json::json!(["b"]),
        serde_json::json!(["c"]),
        serde_json::json!(["d"]),
    ];
    let got = plan(&units, &profile, 0);
    assert_eq!(got.n, 2);
    assert_eq!(got.bound, Bound::Cap);

    // And with fewer units than the cap, the units bound it.
    let got = plan(&units[..1], &profile, 0);
    assert_eq!(got.n, 1);
    assert_eq!(got.bound, Bound::Units);
}

#[test]
fn a_declared_child_cost_bounds_n() {
    let profile = FanOutProfile::priced(8, 25_000);
    let units: Vec<serde_json::Value> = (0..8)
        .map(|i| serde_json::json!([format!("u{i}")]))
        .collect();
    let got = plan(&units, &profile, 70_000);
    assert_eq!(got.n, 2, "70k buys two children at 25k each");
    assert_eq!(got.bound, Bound::Budget);
}

#[test]
fn a_zero_child_cost_does_not_divide_by_zero() {
    let profile = FanOutProfile {
        cap: 4,
        child_cost: Some(0),
    };
    assert_eq!(n_of(3, &profile, 1_000).n, 3);
}

#[test]
fn the_default_profile_does_not_fan_out() {
    let profile = FanOutProfile::default();
    assert_eq!(profile.cap, 1);
    let units = vec![serde_json::json!(["a"]), serde_json::json!(["b"])];
    assert_eq!(plan(&units, &profile, u64::MAX).n, 1);
}
