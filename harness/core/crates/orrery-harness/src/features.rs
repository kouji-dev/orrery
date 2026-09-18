//! The one crate that names an `extensions/` crate.
//!
//! Cargo features select the first-party set, and this module is where each
//! `#[cfg]` lives so that the rest of the facade is written once. `cargo xtask
//! deps-check` allow-lists this crate explicitly; everything else in `core/`
//! that named an extension would be a layering violation.

use std::path::Path;
use std::sync::Arc;

use orrery_host::NativeRegistry;
use orrery_provider::Provider;
use orrery_session::SessionStore;

use crate::build::BuildError;

/// Which first-party extensions this build has.
// Each `push` is behind its own `#[cfg]`, so the `vec![]` clippy suggests
// cannot express it.
#[allow(clippy::vec_init_then_push)]
#[must_use]
pub fn compiled_in() -> Vec<&'static str> {
    #[allow(unused_mut)]
    let mut out: Vec<&'static str> = Vec::new();
    #[cfg(feature = "builtin-tools")]
    out.push("builtin");
    #[cfg(feature = "sqlite")]
    out.push("session-sqlite");
    #[cfg(feature = "fixture-provider")]
    out.push("provider-fixture");
    #[cfg(feature = "anthropic")]
    out.push("provider-anthropic");
    out
}

/// Add every compiled-in native extension to a registry.
///
/// A build without `--features builtin-tools` simply has fewer entries, and
/// nothing else in the system knows the difference — which is translation #14:
/// `native` is a runtime, not a back door.
#[allow(unused_variables)]
pub fn register_native(registry: &mut NativeRegistry) {
    #[cfg(feature = "builtin-tools")]
    registry.register(Arc::new(orrery_ext_tools_builtin::BuiltinTools::new()));
}

/// Open the default session store.
///
/// # Errors
///
/// [`BuildError::NoStore`] when no store feature is enabled, and
/// [`SessionError`] when the database will not open.
#[allow(unused_variables)]
pub fn open_store(state_dir: &Path) -> Result<Arc<dyn SessionStore>, BuildError> {
    #[cfg(feature = "sqlite")]
    {
        orrery_ext_session_sqlite::build(state_dir).map_err(BuildError::Store)
    }
    #[cfg(not(feature = "sqlite"))]
    {
        let _ = state_dir;
        Err(BuildError::NoStore)
    }
}

/// Load a fixture provider: one `.jsonl` per pass, the last one repeating.
///
/// # Errors
///
/// [`BuildError::NoProvider`] without the feature, [`BuildError::Provider`]
/// when a fixture will not parse.
#[allow(unused_variables)]
pub fn fixture_provider(paths: &[std::path::PathBuf]) -> Result<Arc<dyn Provider>, BuildError> {
    #[cfg(feature = "fixture-provider")]
    {
        crate::fixture::Sequence::load(paths).map(|p| p as Arc<dyn Provider>)
    }
    #[cfg(not(feature = "fixture-provider"))]
    {
        Err(BuildError::NoProvider {
            which: "fixture".to_owned(),
        })
    }
}
