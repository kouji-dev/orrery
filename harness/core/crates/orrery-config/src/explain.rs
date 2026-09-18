//! `config explain <key>` — where did this value come from?
//!
//! The answer is only worth anything if it also names what it beat. A key set
//! in two layers has a winner and a shadowed value, and printing the winner
//! alone is how somebody spends an afternoon editing a file that is not in
//! force.
//!
//! The CLI surface is plan 17's; this is the answer it prints.

use std::fmt;

use crate::provenance::{Fold, Origin, Provenanced, Slot};

/// One layer's contribution to a key.
#[derive(Clone, Debug, PartialEq)]
pub struct Contribution {
    /// The value, as written.
    pub value: toml::Value,
    /// Where it was written.
    pub origin: Origin,
    /// Whether it is in force.
    pub in_force: bool,
}

/// Where a key's value came from, and what it beat.
#[derive(Clone, Debug, PartialEq)]
pub struct Explanation {
    /// The key asked about.
    pub key: String,
    /// The key actually read.
    ///
    /// The same as [`key`](Self::key) unless a profile overlay answered: with
    /// `--profile review` and `model` set under `[profile.review]`, this is
    /// `profile.review.model`. Naming it is the difference between an answer
    /// and a riddle — the person asked about `model` and the value they are
    /// looking at is not written next to that word anywhere.
    pub resolved_from: String,
    /// How it folds: closest-wins, or a union.
    pub fold: Fold,
    /// Every layer that set it, winner first.
    pub contributions: Vec<Contribution>,
}

impl Explanation {
    /// The contribution in force, if anything set the key.
    #[must_use]
    pub fn winner(&self) -> Option<&Contribution> {
        self.contributions.iter().find(|c| c.in_force)
    }

    /// The contributions a closer layer shadowed.
    #[must_use]
    pub fn shadowed(&self) -> Vec<&Contribution> {
        self.contributions.iter().filter(|c| !c.in_force).collect()
    }

    /// Whether anything set it at all.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.contributions.is_empty()
    }
}

impl fmt::Display for Explanation {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        if self.contributions.is_empty() {
            return write!(f, "{}: not set in any layer", self.key);
        }
        for (i, c) in self.contributions.iter().enumerate() {
            if i > 0 {
                writeln!(f)?;
            }
            let layer = format!("{:?}", c.origin.layer).to_lowercase();
            let key = &self.resolved_from;
            if c.in_force {
                let word = match self.fold {
                    Fold::Union => "in force (union)",
                    Fold::Override => "in force",
                };
                write!(f, "{key} = {} — {layer}, {} — {word}", c.value, c.origin)?;
            } else {
                write!(f, "{key} = {} — {layer}, {} — shadowed", c.value, c.origin)?;
            }
        }
        Ok(())
    }
}

/// Explain one key.
#[must_use]
pub fn explain(values: &Provenanced, key: &str) -> Explanation {
    let fold = values.fold(key).unwrap_or(Fold::Override);
    Explanation {
        key: key.to_owned(),
        resolved_from: key.to_owned(),
        fold,
        contributions: contributions(values, key, fold),
    }
}

/// Explain one key **as a profile sees it**.
///
/// A profile is an overlay, so `--profile review` asking about `model` is
/// asking about `profile.review.model` *and* about `model`, in that order. A
/// bare key that a profile overrode is still printed, as shadowed: naming only
/// the winner is how somebody spends an afternoon editing a file that is not in
/// force, and that reasoning does not stop being true one key deeper.
///
/// A key that already names a profile explicitly — `profile.review.model` — is
/// left exactly as it was typed, so the raw dotted path keeps working.
#[must_use]
pub fn explain_in(values: &Provenanced, key: &str, profile: Option<&str>) -> Explanation {
    let Some(profile) = profile.filter(|p| !p.is_empty()) else {
        return explain(values, key);
    };
    if key.starts_with("profile.") {
        return explain(values, key);
    }
    let overlaid = format!("profile.{profile}.{key}");
    let fold = values
        .fold(&overlaid)
        .or_else(|| values.fold(key))
        .unwrap_or(Fold::Override);
    let mut from_profile = contributions(values, &overlaid, fold);
    if from_profile.is_empty() {
        // Nothing under the profile: the bare key is the whole answer, and the
        // key it was read from is the one that was typed.
        return explain(values, key);
    }
    // Everything the overlay beat, which under an override is all of it.
    let mut beaten = contributions(values, key, fold);
    if fold == Fold::Override {
        for c in &mut beaten {
            c.in_force = false;
        }
    }
    from_profile.extend(beaten);
    Explanation {
        key: key.to_owned(),
        resolved_from: overlaid,
        fold,
        contributions: from_profile,
    }
}

/// Every layer that set one key, winner first.
fn contributions(values: &Provenanced, key: &str, fold: Fold) -> Vec<Contribution> {
    values
        .all(key)
        .iter()
        .enumerate()
        .map(|(i, slot): (usize, &Slot)| Contribution {
            value: slot.value.clone(),
            origin: slot.origin.clone(),
            // Under a union every layer stays in force; under an override only
            // the closest does.
            in_force: fold == Fold::Union || i == 0,
        })
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;
    use orrery_proto::Layer;

    use crate::layer::LayerFile;

    fn values(files: &[LayerFile]) -> Provenanced {
        crate::merge::merge(files).expect("the fixture parses").values
    }

    #[test]
    fn prints_value_layer_and_file() {
        let vals = values(&[
            LayerFile::new(Layer::User, "user.toml", "model = \"sonnet\"\n"),
            LayerFile::new(
                Layer::Project,
                "project.toml",
                "# a comment\nmodel = \"qwen\"\n",
            ),
        ]);
        let explained = explain(&vals, "model");

        let winner = explained.winner().expect("something is in force");
        assert_eq!(winner.value.as_str(), Some("qwen"));
        assert_eq!(winner.origin.layer, Layer::Project);
        assert_eq!(winner.origin.line, 2);

        let shadowed = explained.shadowed();
        assert_eq!(shadowed.len(), 1, "the loser is named too");
        assert_eq!(shadowed[0].value.as_str(), Some("sonnet"));
        assert_eq!(shadowed[0].origin.layer, Layer::User);

        let printed = explained.to_string();
        assert!(printed.contains("project.toml:2"), "{printed}");
        assert!(printed.contains("in force"), "{printed}");
        assert!(printed.contains("user.toml:1"), "{printed}");
        assert!(printed.contains("shadowed"), "{printed}");
    }

    #[test]
    fn a_union_key_has_no_loser() {
        let vals = values(&[
            LayerFile::new(Layer::Managed, "managed.toml", "[permissions]\ndeny = [\"creds(*)\"]\n"),
            LayerFile::new(Layer::User, "user.toml", "[permissions]\ndeny = [\"net(domain: *)\"]\n"),
        ]);
        let explained = explain(&vals, "permissions.deny");
        assert_eq!(explained.fold, Fold::Union);
        assert!(explained.shadowed().is_empty(), "deny is a union: {explained}");
        assert_eq!(explained.contributions.len(), 2);
        assert!(explained.to_string().contains("union"));
    }

    /// Round 5: a profile is an overlay, and `explain_in` reads it.
    #[test]
    fn a_profile_overlay_wins_and_names_the_key_it_read() {
        let vals = values(&[LayerFile::new(
            Layer::User,
            "user.toml",
            "model = \"sonnet\"
[profile.review]
model = \"review-model\"
",
        )]);

        let explained = explain_in(&vals, "model", Some("review"));
        assert_eq!(explained.resolved_from, "profile.review.model");
        let winner = explained.winner().expect("the overlay is in force");
        assert_eq!(winner.value.as_str(), Some("review-model"));
        let shadowed = explained.shadowed();
        assert_eq!(shadowed.len(), 1, "the bare key is still named: {explained}");
        assert_eq!(shadowed[0].value.as_str(), Some("sonnet"));

        // A profile that sets nothing falls through to the bare key.
        let plain = explain_in(&vals, "model", Some("other"));
        assert_eq!(plain.resolved_from, "model");
        assert_eq!(
            plain.winner().and_then(|c| c.value.as_str()),
            Some("sonnet")
        );

        // And an explicit dotted path is left exactly as typed.
        let raw = explain_in(&vals, "profile.review.model", Some("review"));
        assert_eq!(raw.resolved_from, "profile.review.model");
        assert_eq!(raw.contributions.len(), 1);
    }

    #[test]
    fn a_key_nobody_set_says_so() {
        let explained = explain(&Provenanced::new(), "model");
        assert!(explained.is_empty());
        assert_eq!(explained.to_string(), "model: not set in any layer");
    }
}
