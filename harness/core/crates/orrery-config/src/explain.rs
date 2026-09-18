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
            if c.in_force {
                let word = match self.fold {
                    Fold::Union => "in force (union)",
                    Fold::Override => "in force",
                };
                write!(f, "{} = {} — {layer}, {} — {word}", self.key, c.value, c.origin)?;
            } else {
                write!(
                    f,
                    "{} = {} — {layer}, {} — shadowed",
                    self.key, c.value, c.origin
                )?;
            }
        }
        Ok(())
    }
}

/// Explain one key.
#[must_use]
pub fn explain(values: &Provenanced, key: &str) -> Explanation {
    let fold = values.fold(key).unwrap_or(Fold::Override);
    let contributions = values
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
        .collect();
    Explanation {
        key: key.to_owned(),
        fold,
        contributions,
    }
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

    #[test]
    fn a_key_nobody_set_says_so() {
        let explained = explain(&Provenanced::new(), "model");
        assert!(explained.is_empty());
        assert_eq!(explained.to_string(), "model: not set in any layer");
    }
}
