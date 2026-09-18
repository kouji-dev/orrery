//! Load and validate: agent parameter schemas, role bindings, singleton
//! conflicts — all of it naming the file and the line.
//!
//! Grown in task 5.

use crate::discover::DiscoveryManifest;
use crate::error::ConfigError;
use crate::provenance::Provenanced;

/// What validation had to say.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Validation {
    /// Things that are not errors but somebody should know about.
    pub warnings: Vec<String>,
}

/// Validate the effective configuration.
///
/// # Errors
///
/// When a binding is missing or a parameter is unknown. The error names the
/// file and the line.
pub fn validate(
    values: &Provenanced,
    manifest: &DiscoveryManifest,
) -> Result<Validation, ConfigError> {
    let _ = (values, manifest);
    Ok(Validation::default())
}
