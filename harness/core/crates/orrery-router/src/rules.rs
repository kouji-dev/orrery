//! The rungs, and the declarative rules that allow or forbid climbing one.
//!
//! A rule is a conjunction of comparisons over [`Signals`] plus one effect.
//! That is the whole grammar: no functions, no expressions over expressions,
//! nothing to evaluate that could fail at run time. It is written like this:
//!
//! ```toml
//! [[route]]
//! name   = "one problem is one agent"
//! when   = { diff_lines = "< 200" }
//! deny   = "fan-out"
//! reason = "a diff this small does not decompose into disjoint sets"
//!
//! [[route]]
//! name = "a failing gate justifies a loop"
//! when = { last_gate = "== 0" }
//! allow = "bounded-loop"
//! ```

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

use crate::signals::Signals;

/// A rung of the escalation ladder.
///
/// Ordered. [`Router::decide`](crate::Router::decide) climbs **one at a time**,
/// so the ordering is load-bearing rather than cosmetic.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Rung {
    /// The default: run another pass under the turn budget.
    OnePass,
    /// A loop with a predicate and a hard cap.
    BoundedLoop,
    /// One child on its own branch, with a slice of the parent's budget.
    SubAgent,
    /// Several children over demonstrably disjoint inputs.
    FanOut,
    /// A declared sequence, typechecked at load.
    Workflow,
}

impl Rung {
    /// Every rung, lowest first.
    #[must_use]
    pub const fn all() -> [Rung; 5] {
        [
            Rung::OnePass,
            Rung::BoundedLoop,
            Rung::SubAgent,
            Rung::FanOut,
            Rung::Workflow,
        ]
    }

    /// Its height on the ladder, from zero.
    #[must_use]
    pub const fn level(self) -> u8 {
        match self {
            Rung::OnePass => 0,
            Rung::BoundedLoop => 1,
            Rung::SubAgent => 2,
            Rung::FanOut => 3,
            Rung::Workflow => 4,
        }
    }

    /// The next rung up, or itself at the top.
    #[must_use]
    pub const fn next(self) -> Rung {
        match self {
            Rung::OnePass => Rung::BoundedLoop,
            Rung::BoundedLoop => Rung::SubAgent,
            Rung::SubAgent => Rung::FanOut,
            Rung::FanOut | Rung::Workflow => Rung::Workflow,
        }
    }

    /// The word it is written with in config and in the audit.
    #[must_use]
    pub const fn as_str(self) -> &'static str {
        match self {
            Rung::OnePass => "one-pass",
            Rung::BoundedLoop => "bounded-loop",
            Rung::SubAgent => "sub-agent",
            Rung::FanOut => "fan-out",
            Rung::Workflow => "workflow",
        }
    }

    /// Read one back.
    #[must_use]
    pub fn parse(word: &str) -> Option<Rung> {
        Rung::all().into_iter().find(|r| r.as_str() == word)
    }
}

impl std::fmt::Display for Rung {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(self.as_str())
    }
}

/// The signals a rule may name, by the word it is written with.
pub const SIGNAL_NAMES: [&str; 9] = [
    "budget_fraction",
    "tokens_spent",
    "tokens_remaining",
    "turns_in_mode",
    "reads",
    "writes",
    "diff_lines",
    "repeated_identical_calls",
    "last_gate",
];

/// How a condition compares.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Op {
    /// `<`
    Lt,
    /// `<=`
    Le,
    /// `>`
    Gt,
    /// `>=`
    Ge,
    /// `==`
    Eq,
    /// `!=`
    Ne,
}

impl Op {
    /// Read one of the six operators.
    #[must_use]
    pub fn parse(word: &str) -> Option<Op> {
        Some(match word {
            "<" => Op::Lt,
            "<=" => Op::Le,
            ">" => Op::Gt,
            ">=" => Op::Ge,
            "==" | "=" => Op::Eq,
            "!=" => Op::Ne,
            _ => return None,
        })
    }

    /// Apply it.
    #[must_use]
    pub fn apply(self, lhs: f64, rhs: f64) -> bool {
        match self {
            Op::Lt => lhs < rhs,
            Op::Le => lhs <= rhs,
            Op::Gt => lhs > rhs,
            Op::Ge => lhs >= rhs,
            #[allow(clippy::float_cmp)]
            Op::Eq => lhs == rhs,
            #[allow(clippy::float_cmp)]
            Op::Ne => lhs != rhs,
        }
    }
}

/// One comparison against one signal.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct Condition {
    /// Which signal, by the name in [`SIGNAL_NAMES`].
    pub signal: String,
    /// How it is compared.
    pub op: Op,
    /// What it is compared against.
    pub value: f64,
}

impl Condition {
    /// Whether these signals satisfy it.
    ///
    /// A signal that is absent — `last_gate` before any gate has run — makes
    /// the condition **false**, never true: a rule fires on evidence, not on
    /// the lack of it.
    #[must_use]
    pub fn holds(&self, values: &BTreeMap<String, f64>) -> bool {
        values
            .get(&self.signal)
            .is_some_and(|v| self.op.apply(*v, self.value))
    }
}

/// What a rule does when it fires.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum Effect {
    /// This rung may be climbed.
    Allow {
        /// Which rung.
        rung: Rung,
    },
    /// This rung may not be climbed, and why.
    Deny {
        /// Which rung.
        rung: Rung,
        /// What to tell the caller — and the model.
        reason: String,
    },
}

impl Effect {
    /// The rung it is about.
    #[must_use]
    pub const fn rung(&self) -> Rung {
        match self {
            Effect::Allow { rung } | Effect::Deny { rung, .. } => *rung,
        }
    }
}

/// One declared rule.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct RoutingRule {
    /// What it is called, for the audit and for the error message.
    pub name: String,
    /// Every condition, all of which must hold.
    pub when: Vec<Condition>,
    /// What it does.
    pub effect: Effect,
}

impl RoutingRule {
    /// Whether every condition holds.
    #[must_use]
    pub fn fires(&self, values: &BTreeMap<String, f64>) -> bool {
        self.when.iter().all(|c| c.holds(values))
    }
}

/// The rules in force, in declaration order. **First match wins**, exactly as
/// in `orrery-policy`: two rule systems that disagree about precedence are two
/// rule systems nobody can reason about.
#[derive(Clone, Debug, Default, PartialEq, Serialize, Deserialize)]
pub struct RuleSet {
    /// The rules, in the order they were written.
    pub rules: Vec<RoutingRule>,
}

/// What the rules had to say about one rung.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq)]
pub enum RuleVerdict {
    /// No rule mentioned this rung. The router decides on its own.
    NoRule,
    /// A rule allows it.
    Allowed {
        /// Which rule.
        rule: String,
    },
    /// A rule forbids it.
    Denied {
        /// Which rule.
        rule: String,
        /// Why.
        reason: String,
    },
}

impl RuleSet {
    /// No rules at all.
    #[must_use]
    pub fn empty() -> Self {
        Self::default()
    }

    /// A set from rules already in hand.
    #[must_use]
    pub fn new(rules: Vec<RoutingRule>) -> Self {
        Self { rules }
    }

    /// What the first matching rule says about this rung.
    #[must_use]
    pub fn verdict(&self, signals: &Signals, rung: Rung) -> RuleVerdict {
        let values = signals.values();
        for rule in &self.rules {
            if rule.effect.rung() != rung || !rule.fires(&values) {
                continue;
            }
            return match &rule.effect {
                Effect::Allow { .. } => RuleVerdict::Allowed {
                    rule: rule.name.clone(),
                },
                Effect::Deny { reason, .. } => RuleVerdict::Denied {
                    rule: rule.name.clone(),
                    reason: reason.clone(),
                },
            };
        }
        RuleVerdict::NoRule
    }

    /// Read a `[[route]]` list out of a configuration layer.
    ///
    /// # Errors
    ///
    /// [`RuleError`] when the TOML does not parse, a signal is not one of
    /// [`SIGNAL_NAMES`], a comparison is not `<op> <number>`, a rung is not one
    /// of the five, or a rule declares neither `allow` nor `deny`. Every one of
    /// them names the file.
    pub fn parse_toml(text: &str, file: &str) -> Result<RuleSet, RuleError> {
        let doc: toml::Value = text.parse().map_err(|e: toml::de::Error| RuleError {
            file: file.to_owned(),
            rule: None,
            message: e.to_string(),
        })?;
        let bad = |rule: Option<&str>, message: String| RuleError {
            file: file.to_owned(),
            rule: rule.map(str::to_owned),
            message,
        };

        let mut rules = Vec::new();
        let Some(list) = doc.get("route").and_then(toml::Value::as_array) else {
            return Ok(RuleSet::empty());
        };
        for (index, entry) in list.iter().enumerate() {
            let name = entry
                .get("name")
                .and_then(toml::Value::as_str)
                .map_or_else(|| format!("route[{index}]"), str::to_owned);
            let here = Some(name.as_str());

            let mut when = Vec::new();
            if let Some(table) = entry.get("when") {
                let table = table.as_table().ok_or_else(|| {
                    bad(
                        here,
                        "`when` is a table of signal = \"<op> <number>\"".to_owned(),
                    )
                })?;
                for (signal, raw) in table {
                    if !SIGNAL_NAMES.contains(&signal.as_str()) {
                        return Err(bad(
                            here,
                            format!(
                                "`{signal}` is not a signal a rule may read: expected one of {}",
                                SIGNAL_NAMES.join(", ")
                            ),
                        ));
                    }
                    let text = raw.as_str().ok_or_else(|| {
                        bad(
                            here,
                            format!("`{signal}` takes a comparison, written as \"< 100\""),
                        )
                    })?;
                    let (op, value) = text.trim().split_once(' ').ok_or_else(|| {
                        bad(
                            here,
                            format!("`{signal} = \"{text}\"` is not `<op> <number>`"),
                        )
                    })?;
                    let op = Op::parse(op.trim()).ok_or_else(|| {
                        bad(
                            here,
                            format!("`{op}` is not a comparison: expected <, <=, >, >=, == or !="),
                        )
                    })?;
                    let value: f64 = value
                        .trim()
                        .parse()
                        .map_err(|_| bad(here, format!("`{value}` is not a number")))?;
                    when.push(Condition {
                        signal: signal.clone(),
                        op,
                        value,
                    });
                }
            }
            // A table has no order of its own, so sort by name: the rule has to
            // compare equal whichever way TOML handed it over.
            when.sort_by(|a, b| a.signal.cmp(&b.signal));

            let rung = |key: &str| -> Result<Option<Rung>, RuleError> {
                match entry.get(key).and_then(toml::Value::as_str) {
                    None => Ok(None),
                    Some(word) => Rung::parse(word).map(Some).ok_or_else(|| {
                        bad(
                            here,
                            format!(
                                "`{word}` is not a rung: expected one of {}",
                                Rung::all().map(Rung::as_str).join(", ")
                            ),
                        )
                    }),
                }
            };

            let effect = match (rung("allow")?, rung("deny")?) {
                (Some(_), Some(_)) => {
                    return Err(bad(
                        here,
                        "a rule either allows or denies, not both".to_owned(),
                    ));
                }
                (Some(rung), None) => Effect::Allow { rung },
                (None, Some(rung)) => Effect::Deny {
                    rung,
                    reason: entry
                        .get("reason")
                        .and_then(toml::Value::as_str)
                        .unwrap_or("a declared rule forbids this rung")
                        .to_owned(),
                },
                (None, None) => {
                    return Err(bad(
                        here,
                        "a rule says `allow = \"<rung>\"` or `deny = \"<rung>\"`".to_owned(),
                    ));
                }
            };

            rules.push(RoutingRule { name, when, effect });
        }
        Ok(RuleSet::new(rules))
    }
}

/// A `[[route]]` entry that is not one.
#[derive(Clone, Debug, PartialEq, Eq, thiserror::Error)]
#[error("{file}{}: {message}", match rule { Some(r) => format!(" (`{r}`)"), None => String::new() })]
pub struct RuleError {
    /// Which file it was written in.
    pub file: String,
    /// Which rule, when the problem is inside one.
    pub rule: Option<String>,
    /// What is wrong.
    pub message: String,
}
