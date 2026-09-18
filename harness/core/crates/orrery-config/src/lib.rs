//! The five configuration layers, their TOML merge, provenance, profiles and trust gating.
//!
//! Implementation plan: `harness/docs/plans/10-config-layers.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod error;
pub mod layer;
pub mod merge;
pub mod provenance;

pub use error::ConfigError;
pub use layer::{CONFIG_DIR, CONFIG_FILE, ConfigPaths, LayerFile};
pub use merge::{MergeReport, Relaxation};
pub use provenance::{Fold, Origin, Provenanced, Slot};
