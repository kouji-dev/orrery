//! What the run path would do with one discovered extension, before it does it.
//!
//! # Why this is a module and not two `if`s
//!
//! `orrery ext list` used to answer from the manifest alone: it parsed
//! `orrery.toml`, checked the grants, and printed `ok`. The run path answers
//! from more than that — it asks whether this build has a host for the declared
//! runtime, and (since phase 8's load gate) whether the managed layer still
//! allows an unpinned extension to load at all. The two answers disagreed, and
//! a driven acceptance run caught the listing printing `ok` for an extension
//! the session logged as skipped.
//!
//! So the question is asked **once**, here, and both callers ask it: the
//! builder in [`crate::build`] when it decides whether to hand a manifest to a
//! host, and `orrery ext list` / `orrery ext test` when they report. A third
//! copy of this reasoning is how the listing came to lie in the first place.

use std::path::Path;

use orrery_ext_api::RuntimeKind;
use orrery_proto::{ExtId, LoadOutcome, SkipReason};

/// Why the run path would not even try to load an extension.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Skip {
    /// The closed-set reason, as the ledger records it.
    pub reason: SkipReason,
    /// The sentence a person reads.
    pub why: String,
}

impl Skip {
    /// The ledger entry for this extension.
    #[must_use]
    pub fn outcome(&self, ext: &ExtId) -> LoadOutcome {
        LoadOutcome::Skipped {
            ext: ext.clone(),
            reason: self.reason,
        }
    }
}

/// Whether this build has a host that could run `runtime`, and why not.
///
/// `native` is the interesting `None`: a compiled-in bundle is registered at
/// build time by [`crate::features::register_native`], and a directory on disk
/// claiming `runtime = "native"` has no code this process could run. That is
/// correct, and the only thing that was wrong about it is that nothing said so.
///
/// Kept next to [`crate::features::host_for`] in spirit and checked against it
/// by `plan::tests::the_two_agree`: a runtime this says is hosted must be one
/// `host_for` returns a host for.
#[must_use]
pub fn no_host_reason(runtime: RuntimeKind) -> Option<String> {
    match runtime {
        RuntimeKind::Native => Some(
            "this build has no host for `native`; a native extension is compiled in, \
             not loaded from disk"
                .to_owned(),
        ),
        #[cfg(feature = "rpc-extensions")]
        RuntimeKind::Node | RuntimeKind::Python | RuntimeKind::Process => None,
        other => Some(format!("this build has no host for `{other}`")),
    }
}

/// The managed `[registry]` table in force on this machine, if there is one.
///
/// A file that will not parse is treated as no managed layer rather than as a
/// refusal: the load path is not the place a syntax error in `managed.toml`
/// should first be discovered, and `orrery install` already reports it.
#[must_use]
pub fn managed_registry() -> Option<orrery_registry::ManagedRegistry> {
    orrery_registry::ManagedRegistry::read(&orrery_config::layer::managed_path())
        .ok()
        .flatten()
}

/// What the run path would do with the extension whose files are at `root`.
///
/// `None` means it would be loaded. `Some` is the reason it would not be, in
/// the words the ledger and `ext list` both use.
#[must_use]
pub fn skip_for(
    runtime: RuntimeKind,
    root: &Path,
    managed: Option<&orrery_registry::ManagedRegistry>,
) -> Option<Skip> {
    if let Some(why) = no_host_reason(runtime) {
        return Some(Skip {
            reason: SkipReason::Unsupported,
            why,
        });
    }
    let receipt = orrery_registry::SupplyChainRecord::beside(root);
    if let Some(why) = orrery_registry::load_refusal(managed, receipt.as_ref()) {
        return Some(Skip {
            reason: SkipReason::PolicyDenied,
            why,
        });
    }
    None
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The two answers must not drift: anything `no_host_reason` calls hosted
    /// has to be something `features::host_for` really returns a host for.
    #[test]
    fn the_two_agree() {
        use std::sync::Arc;

        let ext: ExtId = "probe".parse().expect("a valid id");
        let root = std::env::temp_dir();
        for runtime in [
            RuntimeKind::Native,
            RuntimeKind::Node,
            RuntimeKind::Python,
            RuntimeKind::Process,
            RuntimeKind::Wasm,
        ] {
            let broker: Arc<dyn orrery_ext_api::BrokerFacade> =
                Arc::new(orrery_ext_api::testing::MockBroker::new(Vec::new()));
            let host = crate::features::host_for(runtime, &ext, &root, broker);
            assert_eq!(
                host.is_none(),
                no_host_reason(runtime).is_some(),
                "`{runtime}` disagrees"
            );
        }
    }

    /// The native answer says what to do about it, not only that it failed.
    #[test]
    fn a_native_extension_on_disk_says_why() {
        let why = no_host_reason(RuntimeKind::Native).expect("native has no disk host");
        assert!(why.contains("native"), "{why}");
        assert!(why.contains("compiled in"), "{why}");
    }
}
