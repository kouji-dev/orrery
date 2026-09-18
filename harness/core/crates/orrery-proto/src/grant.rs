//! Capabilities, grants and consent.
//!
//! # Where narrowing lives
//!
//! A child's grant is never a superset of its parent's, and narrowing is the
//! *only* operation ever applied to a grant. The narrowing that the kernel
//! actually enforces is **glob-aware** — `read(./src/**)` narrowed by
//! `read(./**)` is `./src/**`, because the second pattern contains the first —
//! and a glob matcher is a dependency this crate does not have and will not
//! take.
//!
//! So the operation lives in `orrery-policy` (plan 07) and the data lives here.
//! What this module offers is [`Grant::intersect_exact`]: string-set
//! intersection, correct for [`Consent`] and for exact-match aspects
//! (`tool`, `skill`, `ext`, `mode`, `mcp`), and **not** correct for the path and
//! host aspects (`read`, `write`, `net`). It is named for what it does so that
//! nothing reaches for it expecting policy semantics. See the decision recorded
//! under "Open questions" in `harness/docs/plans/01-proto-shared-types.md`.

use serde::{Deserialize, Serialize};

/// What a capability is *about*.
///
/// The wire names are pinned per variant rather than derived, because two of
/// them (`mem.read`, `mem.write`) are dotted and no `rename_all` rule produces
/// a dot.
#[non_exhaustive]
#[derive(
    Copy,
    Clone,
    Debug,
    PartialEq,
    Eq,
    PartialOrd,
    Ord,
    Hash,
    Serialize,
    Deserialize,
    schemars::JsonSchema,
)]
#[serde(rename_all = "kebab-case")]
pub enum Aspect {
    /// Calling a named tool.
    Tool,
    /// Talking to an MCP server.
    Mcp,
    /// Running a skill.
    Skill,
    /// Loading an extension.
    Ext,
    /// Entering a mode.
    Mode,
    /// Reading a path.
    Read,
    /// Writing a path.
    Write,
    /// Spawning a sub-agent or a process.
    Spawn,
    /// Reaching a host over the network.
    Net,
    /// Reading a credential.
    Creds,
    /// Driving the user interface.
    Ui,
    /// Contributing a renderer for a surface kind.
    Render,
    /// Reading memory.
    #[serde(rename = "mem.read")]
    MemRead,
    /// Writing memory.
    #[serde(rename = "mem.write")]
    MemWrite,
}

impl Aspect {
    /// True when this aspect's scope entries are exact names rather than
    /// patterns, and [`Grant::intersect_exact`] is therefore the right
    /// operation for it.
    #[must_use]
    pub const fn is_exact_match(self) -> bool {
        matches!(
            self,
            Aspect::Tool
                | Aspect::Mcp
                | Aspect::Skill
                | Aspect::Ext
                | Aspect::Mode
                | Aspect::Creds
                | Aspect::Ui
                | Aspect::Render
                | Aspect::MemRead
                | Aspect::MemWrite
        )
    }
}

/// One aspect and the scope it is granted over.
///
/// An **empty** `scope` means unqualified: the whole aspect, not nothing. That
/// is what makes an unscoped parent wider than a scoped child, which is the
/// direction narrowing needs.
#[derive(
    Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Hash, Serialize, Deserialize, schemars::JsonSchema,
)]
pub struct Capability {
    /// What the capability is about.
    pub aspect: Aspect,
    /// The names or patterns it covers. Empty means unqualified.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub scope: Vec<String>,
}

impl Capability {
    /// An unqualified capability over a whole aspect.
    #[must_use]
    pub fn all(aspect: Aspect) -> Self {
        Self {
            aspect,
            scope: Vec::new(),
        }
    }

    /// A capability over a named set.
    #[must_use]
    pub fn scoped(aspect: Aspect, scope: impl IntoIterator<Item = impl Into<String>>) -> Self {
        Self {
            aspect,
            scope: scope.into_iter().map(Into::into).collect(),
        }
    }
}

/// How often the user has to be asked.
///
/// Ordered `Never < Once < Always`, and narrowing takes the minimum: a child
/// that asks every time cannot be promoted to a child that never asks.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash, Serialize, Deserialize, schemars::JsonSchema)]
#[serde(rename_all = "kebab-case")]
pub enum Consent {
    /// Allowed without asking.
    Always,
    /// Allowed once the user says so, each time.
    Once,
    /// Not allowed at all.
    Never,
}

impl Consent {
    /// `Never` = 0, `Once` = 1, `Always` = 2.
    ///
    /// Spelled out rather than derived, because the declaration order above is
    /// the *readable* one (widest first) and the lattice order is the opposite.
    #[must_use]
    pub const fn rank(self) -> u8 {
        match self {
            Consent::Never => 0,
            Consent::Once => 1,
            Consent::Always => 2,
        }
    }

    /// The narrower of two consents.
    #[must_use]
    pub const fn min(self, other: Self) -> Self {
        if other.rank() < self.rank() {
            other
        } else {
            self
        }
    }
}

impl PartialOrd for Consent {
    fn partial_cmp(&self, other: &Self) -> Option<std::cmp::Ordering> {
        Some(self.cmp(other))
    }
}

impl Ord for Consent {
    fn cmp(&self, other: &Self) -> std::cmp::Ordering {
        self.rank().cmp(&other.rank())
    }
}

/// What a subject is allowed to do.
#[derive(Clone, Debug, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct Grant {
    /// The capabilities, one entry per aspect-and-scope.
    #[serde(default)]
    pub capabilities: Vec<Capability>,
    /// How often to ask.
    pub consent: Consent,
}

impl Default for Grant {
    /// The empty grant: nothing allowed, and nothing may be asked for.
    fn default() -> Self {
        Self {
            capabilities: Vec::new(),
            consent: Consent::Never,
        }
    }
}

impl Grant {
    /// A grant of nothing.
    #[must_use]
    pub fn nothing() -> Self {
        Self::default()
    }

    /// Set-based narrowing: correct for [`Consent`] and for exact-match
    /// aspects, wrong for pattern aspects.
    ///
    /// For each aspect present in **both** grants, the resulting scope is the
    /// set intersection of the two scopes, with an empty scope on either side
    /// read as "unqualified" and therefore as the wider of the pair. An aspect
    /// missing from either side, or whose two non-empty scopes are disjoint, is
    /// dropped. The consent is the minimum. The result is canonical — one
    /// capability per aspect, scopes sorted and deduplicated — so that equality
    /// is meaningful and the operation is commutative and idempotent.
    ///
    /// # This is not the policy operation
    ///
    /// `read(./src/**)` intersected with `read(./**)` is **empty** here and
    /// `./src/**` under the glob semantics the broker enforces. Use
    /// `orrery-policy`'s narrowing for anything where
    /// [`Aspect::is_exact_match`] is false.
    #[must_use]
    pub fn intersect_exact(&self, other: &Grant) -> Grant {
        let mut capabilities: Vec<Capability> = Vec::new();

        for aspect in self.aspects() {
            if !other.aspects().any(|a| a == aspect) {
                continue;
            }
            let mine = self.scope_of(aspect);
            let theirs = other.scope_of(aspect);

            let scope = match (mine.is_empty(), theirs.is_empty()) {
                // Both unqualified: still unqualified.
                (true, true) => Vec::new(),
                // One unqualified: the other is the narrower of the pair.
                (true, false) => theirs,
                (false, true) => mine,
                (false, false) => {
                    let narrowed: Vec<String> =
                        mine.into_iter().filter(|s| theirs.contains(s)).collect();
                    if narrowed.is_empty() {
                        // No overlap at all: the capability is gone, not
                        // promoted to unqualified.
                        continue;
                    }
                    narrowed
                }
            };
            capabilities.push(Capability { aspect, scope });
        }

        capabilities.sort();
        Grant {
            capabilities,
            consent: self.consent.min(other.consent),
        }
    }

    /// Every aspect this grant mentions, in ascending order, without repeats.
    fn aspects(&self) -> impl Iterator<Item = Aspect> + '_ {
        let mut seen: Vec<Aspect> = self.capabilities.iter().map(|c| c.aspect).collect();
        seen.sort();
        seen.dedup();
        seen.into_iter()
    }

    /// The union of every scope written for `aspect`, sorted and deduplicated.
    /// Empty when the aspect is unqualified, or absent.
    fn scope_of(&self, aspect: Aspect) -> Vec<String> {
        let entries: Vec<&Capability> = self
            .capabilities
            .iter()
            .filter(|c| c.aspect == aspect)
            .collect();
        if entries.iter().any(|c| c.scope.is_empty()) {
            return Vec::new();
        }
        let mut scope: Vec<String> = entries
            .into_iter()
            .flat_map(|c| c.scope.iter().cloned())
            .collect();
        scope.sort();
        scope.dedup();
        scope
    }
}

/// A grant declaration that may narrow but is allowed to leave fields out.
///
/// The `Partial<Grant>` of the TypeScript sketch, spelled as a distinct type so
/// that "omitted" and "empty" cannot be confused: `capabilities: None` means
/// *inherit the parent's*, `capabilities: Some(vec![])` means *none*.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize, schemars::JsonSchema)]
pub struct GrantSpec {
    /// The capabilities asked for, or `None` to inherit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub capabilities: Option<Vec<Capability>>,
    /// The consent asked for, or `None` to inherit.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub consent: Option<Consent>,
}

impl GrantSpec {
    /// Resolve this declaration against the grant it is declared under.
    ///
    /// An omitted field inherits. A present field narrows — it is intersected
    /// with the parent, never substituted for it — so a declaration can never
    /// widen what it was given. Uses [`Grant::intersect_exact`], with the same
    /// caveat about pattern aspects.
    #[must_use]
    pub fn apply_to(&self, parent: &Grant) -> Grant {
        let consent = self
            .consent
            .map_or(parent.consent, |c| c.min(parent.consent));
        match &self.capabilities {
            None => Grant {
                capabilities: parent.capabilities.clone(),
                consent,
            },
            Some(asked) => {
                let asked = Grant {
                    capabilities: asked.clone(),
                    consent,
                };
                let mut narrowed = parent.intersect_exact(&asked);
                narrowed.consent = consent;
                narrowed
            }
        }
    }
}
