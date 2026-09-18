//! Profiles: a named composition of everything the runtime assembles.
//!
//! Grown in task 6.

use crate::error::ConfigError;
use crate::provenance::Provenanced;

/// A named composition: model, extensions, interceptors, sub-agents, skills,
/// permissions, consent and audit.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Profile {
    /// Its name. Empty when no profile was asked for.
    pub name: String,
}

/// The profile a context asked for, or the empty one.
///
/// # Errors
///
/// When the named profile is not defined.
pub fn select(values: &Provenanced, name: Option<&str>) -> Result<Profile, ConfigError> {
    let _ = values;
    Ok(Profile {
        name: name.unwrap_or_default().to_owned(),
    })
}
