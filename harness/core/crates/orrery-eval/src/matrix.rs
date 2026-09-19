//! profile × model × seed, expanded in one deterministic order.
//!
//! The order is profile-major, then model, then seed, and it is part of the
//! contract: two runs of the same matrix have to produce results in the same
//! order or a diff of two reports is noise.

use serde::{Deserialize, Serialize};

/// What to run the suite against.
#[derive(Clone, Debug, Default, PartialEq, Eq, Serialize, Deserialize)]
pub struct Matrix {
    /// The profiles. Empty means the one profile called `default`.
    #[serde(default)]
    pub profiles: Vec<String>,
    /// The models. Empty means whatever the profile binds.
    #[serde(default)]
    pub models: Vec<String>,
    /// The seeds.
    ///
    /// Empty means one unseeded point. Most hosted providers offer no
    /// deterministic sampling at all, so a seed here is a *label* on the run
    /// for those and a real setting only for local models — see plan 16, open
    /// question 2. Nothing in this crate claims a seeded run is reproducible.
    #[serde(default)]
    pub seeds: Vec<u64>,
}

/// The profile a point runs under when the matrix names none.
pub const DEFAULT_PROFILE: &str = "default";

/// The model id a point carries when the matrix names none.
pub const DEFAULT_MODEL: &str = "profile-default";

/// One cell of the matrix.
#[derive(Clone, Debug, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
pub struct MatrixPoint {
    /// The profile.
    pub profile: String,
    /// The model.
    pub model: String,
    /// The seed, when one was asked for.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub seed: Option<u64>,
}

impl MatrixPoint {
    /// How a report names this point.
    #[must_use]
    pub fn label(&self) -> String {
        match self.seed {
            Some(seed) => format!("{}/{}@{seed}", self.profile, self.model),
            None => format!("{}/{}", self.profile, self.model),
        }
    }
}

impl Matrix {
    /// A matrix over one axis each.
    #[must_use]
    pub fn new(
        profiles: impl IntoIterator<Item = impl Into<String>>,
        models: impl IntoIterator<Item = impl Into<String>>,
    ) -> Self {
        Self {
            profiles: profiles.into_iter().map(Into::into).collect(),
            models: models.into_iter().map(Into::into).collect(),
            seeds: Vec::new(),
        }
    }

    /// Add the seeds.
    #[must_use]
    pub fn with_seeds(mut self, seeds: impl IntoIterator<Item = u64>) -> Self {
        self.seeds = seeds.into_iter().collect();
        self
    }

    /// Every point, profile-major then model then seed.
    ///
    /// An empty axis contributes exactly one point rather than none: a matrix
    /// with no models named is one run per profile, not zero runs.
    #[must_use]
    pub fn expand(&self) -> Vec<MatrixPoint> {
        let profiles: Vec<&str> = if self.profiles.is_empty() {
            vec![DEFAULT_PROFILE]
        } else {
            self.profiles.iter().map(String::as_str).collect()
        };
        let models: Vec<&str> = if self.models.is_empty() {
            vec![DEFAULT_MODEL]
        } else {
            self.models.iter().map(String::as_str).collect()
        };
        let seeds: Vec<Option<u64>> = if self.seeds.is_empty() {
            vec![None]
        } else {
            self.seeds.iter().copied().map(Some).collect()
        };

        let mut out = Vec::with_capacity(profiles.len() * models.len() * seeds.len());
        for profile in &profiles {
            for model in &models {
                for seed in &seeds {
                    out.push(MatrixPoint {
                        profile: (*profile).to_owned(),
                        model: (*model).to_owned(),
                        seed: *seed,
                    });
                }
            }
        }
        out
    }

    /// Every point of the matrix, then one per competing harness.
    ///
    /// A competitor is **one** point, not one per model and seed: our model
    /// axis is a setting we impose on our own runner, and imposing it on
    /// another harness would put a label on their run that we did not set. So
    /// an adapter point carries [`DEFAULT_MODEL`] and no seed, and is named by
    /// its adapter id — which is also the profile name its runner is bound to.
    ///
    /// Ours come first and the order within each half is the order above, so
    /// two runs of one suite still produce results in the same sequence.
    #[must_use]
    pub fn expand_with_adapters<'a>(
        &self,
        adapters: impl IntoIterator<Item = &'a str>,
    ) -> Vec<MatrixPoint> {
        let mut out = self.expand();
        for id in adapters {
            // A matrix that already names the adapter as a profile has said the
            // same thing twice; running it twice would report agreement it did
            // not measure.
            if out.iter().any(|p| p.profile == id) {
                continue;
            }
            out.push(MatrixPoint {
                profile: id.to_owned(),
                model: DEFAULT_MODEL.to_owned(),
                seed: None,
            });
        }
        out
    }
}
