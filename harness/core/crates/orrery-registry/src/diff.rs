//! The grant diff: what an extension is asking for, and what is new about it.
//!
//! §4.7's install prompt, as a [`Surface`], so every client draws it and none
//! of them owns it:
//!
//! ```text
//!  buildgraph 1.2.0 requests:
//!    read     $WORKSPACE/**        [allow] [allow once] [deny]
//!    spawn    java                 [allow] [allow once] [deny]
//! ```
//!
//! On an **upgrade** the diff is against what was already approved, and the new
//! row is the whole point of this plan:
//!
//! ```text
//!  buildgraph 1.2.0 → 1.3.0 changes:
//!  + net      api.buildgraph.io    [allow] [deny]        ← NEW
//!    read     $WORKSPACE/**        (already allowed)
//! ```
//!
//! Two rules, both asserted rather than described: a new capability on an
//! upgrade is **visually distinct** and **defaults to deny**; and denying does
//! not fail the install — the tools that needed it are disabled, everything else
//! works, and the ledger says `degraded`.

use orrery_ext_api::ExtensionManifest;
use orrery_proto::{
    Capability, Choice, ExtId, LoadOutcome, StackDir, Surface, SurfaceKind, TextStyle,
};

use crate::index::render_requirement;

/// What is being asked about one capability.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum RowState {
    /// A first install: everything is a request, and nothing is a surprise.
    Requested,
    /// An upgrade wants something the previous version did not.
    New,
    /// The previous version already had it and it was approved.
    AlreadyAllowed,
}

impl RowState {
    /// Whether this row must be marked out from the others.
    #[must_use]
    pub fn is_new(self) -> bool {
        matches!(self, RowState::New)
    }
}

/// One line of the diff.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantRow {
    /// What is being asked for.
    pub capability: Capability,
    /// Whether it is new.
    pub state: RowState,
}

impl GrantRow {
    /// The capability as an index or a manifest writes it.
    #[must_use]
    pub fn text(&self) -> String {
        render_requirement(&self.capability)
    }

    /// The key an answer comes back under.
    #[must_use]
    pub fn field(&self) -> String {
        self.text()
    }
}

/// What to do with one row.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq)]
pub enum Decision {
    /// Grant it for good.
    Allow,
    /// Grant it for this session.
    AllowOnce,
    /// Refuse it. The default for anything new.
    Deny,
}

impl Decision {
    /// The value a surface sends back.
    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Decision::Allow => "allow",
            Decision::AllowOnce => "allow-once",
            Decision::Deny => "deny",
        }
    }

    /// Parse one back.
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "allow" => Decision::Allow,
            "allow-once" => Decision::AllowOnce,
            "deny" => Decision::Deny,
            _ => return None,
        })
    }

    /// Whether this decision hands the capability over.
    #[must_use]
    pub fn grants(self) -> bool {
        matches!(self, Decision::Allow | Decision::AllowOnce)
    }
}

/// The whole prompt.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct GrantDiff {
    /// Which extension.
    pub ext: ExtId,
    /// The version already installed, on an upgrade.
    pub from: Option<semver::Version>,
    /// The version being installed.
    pub to: semver::Version,
    /// One row per capability asked for.
    pub rows: Vec<GrantRow>,
}

impl GrantDiff {
    /// A first install: every capability is a request.
    #[must_use]
    pub fn fresh(ext: ExtId, to: semver::Version, wants: &[Capability]) -> Self {
        Self {
            ext,
            from: None,
            to,
            rows: wants
                .iter()
                .map(|c| GrantRow {
                    capability: c.clone(),
                    state: RowState::Requested,
                })
                .collect(),
        }
    }

    /// An upgrade, against what was already approved.
    ///
    /// A capability is `New` when its **aspect and scope together** are not
    /// already covered. A version that keeps `read` but widens its scope is a
    /// new ask, not an unchanged one — that is the case a looser comparison
    /// would wave through.
    #[must_use]
    pub fn upgrade(
        ext: ExtId,
        from: semver::Version,
        to: semver::Version,
        wants: &[Capability],
        already: &[Capability],
    ) -> Self {
        Self {
            ext,
            from: Some(from),
            to,
            rows: wants
                .iter()
                .map(|c| GrantRow {
                    capability: c.clone(),
                    state: if approved(c, already) {
                        RowState::AlreadyAllowed
                    } else {
                        RowState::New
                    },
                })
                .collect(),
        }
    }

    /// Whether anything here is new.
    #[must_use]
    pub fn has_new(&self) -> bool {
        self.rows.iter().any(|r| r.state.is_new())
    }

    /// The rows that are new.
    #[must_use]
    pub fn new_rows(&self) -> Vec<&GrantRow> {
        self.rows.iter().filter(|r| r.state.is_new()).collect()
    }

    /// The answers a client that is asked nothing must use.
    ///
    /// Anything new is `Deny`; anything already allowed stays allowed. A
    /// non-interactive install therefore never *acquires* a capability by
    /// timing out, which is the property `consent = "never"` needs.
    #[must_use]
    pub fn defaults(&self) -> Vec<(String, Decision)> {
        self.rows
            .iter()
            .map(|r| {
                let d = match r.state {
                    RowState::AlreadyAllowed => Decision::Allow,
                    RowState::New | RowState::Requested => Decision::Deny,
                };
                (r.field(), d)
            })
            .collect()
    }

    /// The heading, in the words the plan writes.
    #[must_use]
    pub fn title(&self) -> String {
        match &self.from {
            None => format!("{} {} requests:", self.ext.as_str(), self.to),
            Some(from) => format!("{} {from} → {} changes:", self.ext.as_str(), self.to),
        }
    }

    /// The prompt as a surface: a stack of questions, one per row.
    ///
    /// A `Stack` of `Question`s rather than a `Form`, because each row is
    /// answered from a different fixed set — an already-allowed row has nothing
    /// to ask — and because a client that can only render one question at a time
    /// still gets every row rather than an unanswerable composite.
    #[must_use]
    pub fn surface(&self) -> Surface {
        let mut children = Vec::with_capacity(self.rows.len() + 1);
        children.push(Surface::new(SurfaceKind::Text {
            value: self.title(),
            style: Some(TextStyle::Emphasis),
        }));
        for row in &self.rows {
            children.push(match row.state {
                RowState::AlreadyAllowed => Surface::new(SurfaceKind::Text {
                    value: format!("  {}   (already allowed)", row.text()),
                    style: None,
                }),
                RowState::Requested => Surface::new(SurfaceKind::Question {
                    prompt: format!("  {}", row.text()),
                    choices: vec![
                        choice(Decision::Allow, "allow"),
                        choice(Decision::AllowOnce, "allow once"),
                        choice(Decision::Deny, "deny"),
                    ],
                    multi: false,
                    free: false,
                    default: Some(Decision::Deny.as_str().to_owned()),
                    deadline_ms: None,
                }),
                RowState::New => Surface::new(SurfaceKind::Question {
                    // The marker is in the prompt, not in a style a renderer
                    // might drop: `+` and `NEW` survive a plain-text client.
                    prompt: format!("+ {}   ← NEW", row.text()),
                    choices: vec![
                        choice(Decision::Allow, "allow"),
                        choice(Decision::Deny, "deny"),
                    ],
                    multi: false,
                    free: false,
                    default: Some(Decision::Deny.as_str().to_owned()),
                    deadline_ms: None,
                }),
            });
        }
        Surface::new(SurfaceKind::Stack {
            dir: StackDir::Column,
            title: Some(self.title()),
            collapsed: false,
            children,
        })
    }

    /// Apply a set of answers, filling in [`Self::defaults`] for anything
    /// unanswered.
    #[must_use]
    pub fn apply(&self, answers: &[(String, Decision)]) -> Approval {
        let mut granted = Vec::new();
        let mut denied = Vec::new();
        for row in &self.rows {
            let field = row.field();
            let decision = answers
                .iter()
                .find(|(k, _)| k == &field)
                .map(|(_, d)| *d)
                .unwrap_or_else(|| {
                    self.defaults()
                        .into_iter()
                        .find(|(k, _)| k == &field)
                        .map_or(Decision::Deny, |(_, d)| d)
                });
            if decision.grants() {
                granted.push(row.capability.clone());
            } else {
                denied.push(row.capability.clone());
            }
        }
        Approval { granted, denied }
    }
}

fn choice(decision: Decision, label: &str) -> Choice {
    Choice {
        value: decision.as_str().to_owned(),
        label: label.to_owned(),
    }
}

/// Whether a wanted capability is covered by what was already approved.
fn approved(want: &Capability, already: &[Capability]) -> bool {
    already.iter().any(|have| {
        have.aspect == want.aspect
            && (have.scope.is_empty() || want.scope.iter().all(|s| have.scope.contains(s)))
    })
}

/// What the diff came to.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Approval {
    /// What was allowed.
    pub granted: Vec<Capability>,
    /// What was refused.
    pub denied: Vec<Capability>,
}

impl Approval {
    /// The load outcome a denial produces.
    ///
    /// **Denying does not fail the install.** The tools that needed the refused
    /// capability are disabled, everything else works, and the ledger says
    /// `degraded` with the problems in the words `orrery-ext-api` already uses —
    /// so a person reads the same sentence here and in a running session,
    /// because it is the same function.
    #[must_use]
    pub fn outcome(&self, manifest: &ExtensionManifest, ms: u64) -> LoadOutcome {
        let problems = manifest.unmet(&self.granted);
        let contributions = manifest.contributions();
        if problems.is_empty() {
            LoadOutcome::Ok {
                ext: manifest.name.clone(),
                contributions,
                ms,
            }
        } else {
            LoadOutcome::Degraded {
                ext: manifest.name.clone(),
                contributions,
                ms,
                problems,
            }
        }
    }
}
