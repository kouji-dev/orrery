//! The extension-facing API: manifest, instance, tool definitions, call context, and the mock broker extension authors test against.
//!
//! Implementation plan: `harness/docs/plans/06-extension-host.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod manifest;

pub use manifest::{
    ApiVersion, ExtensionManifest, ManifestError, ProcessSpec, Provides, Requirement, RuntimeKind,
    SingletonSlot, SUPPORTED_API_MAJOR,
};
