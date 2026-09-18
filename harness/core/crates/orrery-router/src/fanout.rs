//! Fan-out is computed, not chosen.
//!
//! ```text
//! N = min(disjoint units the parent can name,
//!         the profile's fan-out cap,
//!         remaining budget ÷ fanout.child_cost)
//! ```
//!
//! `child_cost` is a **declared profile value**. It is not a model guess and it
//! is not measured after the fact: a number that only exists once the children
//! have run cannot bound how many of them to start. Where a profile declares
//! none, the other two bounds hold on their own — which is
//! `no_child_cost_still_bounded`.
//!
//! Independence is demonstrated by giving each child a **disjoint input set**.
//! Overlapping sets mean one problem, so they collapse to one agent. This is
//! bounded delegation, not racing five agents at one problem.

use std::collections::BTreeSet;

use serde::{Deserialize, Serialize};

/// What a profile declares about fanning out.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
pub struct FanOutProfile {
    /// The hard ceiling on children, whatever the arithmetic says.
    pub cap: u32,
    /// What one child is declared to cost, in tokens. `None` means the profile
    /// does not say, and the budget bound simply does not apply.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub child_cost: Option<u64>,
}

impl Default for FanOutProfile {
    /// One child. A profile that has not thought about fan-out does not fan out.
    fn default() -> Self {
        Self {
            cap: 1,
            child_cost: None,
        }
    }
}

impl FanOutProfile {
    /// A profile with a cap and no declared child cost.
    #[must_use]
    pub const fn capped(cap: u32) -> Self {
        Self {
            cap,
            child_cost: None,
        }
    }

    /// A profile with a cap and a declared child cost.
    #[must_use]
    pub const fn priced(cap: u32, child_cost: u64) -> Self {
        Self {
            cap,
            child_cost: Some(child_cost),
        }
    }
}

/// Which of the three bounds produced N.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Bound {
    /// The parent could not name more disjoint units than this.
    Units,
    /// The profile's cap.
    Cap,
    /// What the remaining budget pays for.
    Budget,
    /// The inputs overlap, so there is one problem and one agent.
    Overlap,
}

/// The arithmetic, with the bound that produced it.
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub struct FanOutPlan {
    /// How many children.
    pub n: u32,
    /// Which bound decided.
    pub bound: Bound,
}

/// `N = min(units, cap, remaining ÷ child_cost)`.
///
/// A `child_cost` of `None` — or of zero, which would divide by zero — drops
/// the budget bound rather than collapsing N to nothing.
#[must_use]
pub fn n_of(units: u32, profile: &FanOutProfile, remaining_tokens: u64) -> FanOutPlan {
    let mut n = units;
    let mut bound = Bound::Units;
    if profile.cap < n {
        n = profile.cap;
        bound = Bound::Cap;
    }
    if let Some(cost) = profile.child_cost.filter(|c| *c > 0) {
        let affordable = u32::try_from(remaining_tokens / cost).unwrap_or(u32::MAX);
        if affordable < n {
            n = affordable;
            bound = Bound::Budget;
        }
    }
    FanOutPlan { n, bound }
}

/// Whether every input set is disjoint from every other.
///
/// A unit is read as a set of names: a JSON array of strings is that set, a
/// string is the one-element set, and anything else — an object, a number — is
/// treated as its serialised form, so two identical objects overlap and two
/// different ones do not.
#[must_use]
pub fn are_disjoint(units: &[serde_json::Value]) -> bool {
    let mut seen: BTreeSet<String> = BTreeSet::new();
    for unit in units {
        let members = members_of(unit);
        // An empty unit names nothing, so it demonstrates nothing: it cannot be
        // shown disjoint from its siblings.
        if members.is_empty() {
            return false;
        }
        for member in members {
            if !seen.insert(member) {
                return false;
            }
        }
    }
    true
}

/// The plan for a set of proposed inputs: disjointness first, then arithmetic.
#[must_use]
pub fn plan(
    units: &[serde_json::Value],
    profile: &FanOutProfile,
    remaining_tokens: u64,
) -> FanOutPlan {
    if units.len() > 1 && !are_disjoint(units) {
        return FanOutPlan {
            n: 1,
            bound: Bound::Overlap,
        };
    }
    let units = u32::try_from(units.len()).unwrap_or(u32::MAX);
    n_of(units, profile, remaining_tokens)
}

fn members_of(unit: &serde_json::Value) -> BTreeSet<String> {
    match unit {
        serde_json::Value::Array(items) => items
            .iter()
            .map(|i| match i {
                serde_json::Value::String(s) => s.clone(),
                other => other.to_string(),
            })
            .collect(),
        serde_json::Value::String(s) => BTreeSet::from([s.clone()]),
        serde_json::Value::Null => BTreeSet::new(),
        other => BTreeSet::from([other.to_string()]),
    }
}
