//! The one crate that names an `extensions/` crate.
//!
//! Cargo features select the first-party set, and this module is where each
//! `#[cfg]` lives so that the rest of the facade is written once. `cargo xtask
//! deps-check` allow-lists this crate explicitly; everything else in `core/`
//! that named an extension would be a layering violation.

use std::path::Path;
use std::sync::Arc;

use orrery_ext_api::BrokerFacade;
use orrery_host::NativeRegistry;
use orrery_provider::Provider;
use orrery_session::SessionStore;

use crate::build::BuildError;

/// Which first-party extensions this build has.
///
/// # `anthropic` links, and is not yet selectable
///
/// The feature compiles the provider in and this list reports it, but
/// [`ProviderChoice`](crate::ProviderChoice) has no variant that picks it: a
/// real provider needs a model id, a credential name and an auth flow, all of
/// which are plan 10's profile config. The feature exists now so that the
/// layering is settled - this is the crate that may name an extension - rather
/// than so that a key can be used today. Nothing in this repo's tests may reach
/// the network, which is also why it is off by default.
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
    #[cfg(feature = "views-default")]
    out.push("views-default");
    #[cfg(feature = "agents-default")]
    out.push("agents-default");
    #[cfg(feature = "memory-file")]
    out.push("memory-file");
    #[cfg(feature = "rpc-extensions")]
    out.push("host-rpc");
    #[cfg(feature = "wasm-extensions")]
    out.push("host-wasm");
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
    // The floor, section 6.7 and section 4.6. Registered here, through the same
    // door the builtin tools go through, so each one gets a manifest parse, a
    // policy check, a ledger entry and a deny rule that switches it off. A
    // shortcut that installed either one inside the kernel would give the five
    // bindings everybody needs a different path from everybody else's.
    #[cfg(feature = "views-default")]
    registry.register(Arc::new(orrery_ext_views_default::DefaultViews));
    #[cfg(feature = "agents-default")]
    registry.register(Arc::new(orrery_ext_agents_default::DefaultAgents));
}

/// Build the Anthropic provider a config asked for.
///
/// # Errors
///
/// [`BuildError::NoProvider`] when the `anthropic` feature is off. That is the
/// honest answer: the variant exists in every build so that configuration can
/// name it and the error can say what to do, rather than the enum pretending
/// real models do not exist.
#[allow(unused_variables)]
pub fn anthropic_provider(
    model: &str,
    credential: &str,
    base_url: Option<&str>,
) -> Result<Arc<dyn Provider>, BuildError> {
    #[cfg(feature = "anthropic")]
    {
        use orrery_ext_api::creds::{CredStore, EnvCredStore, LayeredCredStore, MemoryCredStore};

        // The key comes from the environment until a session broker is here to
        // ask: `AnthropicProvider` takes a `CredStore`, and `EnvCredStore` is
        // the documented development fallback. A credential the store does not
        // hold is a `NeedsLogin` turn outcome, not a panic and not a silent
        // unauthenticated request.
        let store: Arc<dyn CredStore> = if credential == orrery_ext_provider_anthropic::GRANT {
            Arc::new(EnvCredStore)
        } else {
            Arc::new(LayeredCredStore::new(
                Arc::new(MemoryCredStore::default()),
                Arc::new(EnvCredStore),
            ))
        };
        let mut provider = orrery_ext_provider_anthropic::AnthropicProvider::new(store);
        if let Some(base_url) = base_url {
            provider = provider.with_base_url(base_url);
        }
        let _ = model;
        Ok(Arc::new(provider))
    }
    #[cfg(not(feature = "anthropic"))]
    {
        Err(BuildError::NoProvider {
            which: "anthropic".to_owned(),
        })
    }
}

/// A host for one extension's declared runtime.
///
/// `native` is not here on purpose: a compiled-in bundle is registered at build
/// time by [`register_native`], and something discovered on disk claiming
/// `runtime = "native"` has no code this process could run. It comes back
/// `None`, which the caller reports as a skipped extension rather than loading
/// something else instead.
///
/// The broker is handed over here rather than set later because a host with no
/// broker denies everything, and "it silently did nothing" is the one failure a
/// guest must not be able to have.
#[allow(unused_variables)]
#[must_use]
pub fn host_for(
    runtime: orrery_ext_api::RuntimeKind,
    ext: &orrery_proto::ExtId,
    root: &Path,
    broker: Arc<dyn BrokerFacade>,
) -> Option<Arc<dyn orrery_host::ExtensionHost>> {
    use orrery_ext_api::RuntimeKind;
    match runtime {
        RuntimeKind::Native => None,
        #[cfg(feature = "rpc-extensions")]
        RuntimeKind::Node | RuntimeKind::Python | RuntimeKind::Process => {
            let host = orrery_host_rpc::RpcHost::for_runtime(runtime);
            host.install(ext, root);
            host.set_broker(broker);
            Some(Arc::new(host))
        }
        _ => None,
    }
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

/// The built-in graders, for `orrery eval`.
///
/// Behind the `graders` feature and **in the default set**: an eval runner with
/// no grader can run a case and cannot score it, which is not a runner. The
/// judge is optional because a judge costs money — a build with no provider
/// bound for grading gets the two free graders rather than a failure.
///
/// This is here rather than in `orrery-eval` for the same reason everything
/// else in this module is: `orrery-ext-graders` is an extension, and this is
/// the one core crate allowed to name one.
#[allow(unused_variables)]
#[must_use]
pub fn graders(
    broker: Arc<dyn BrokerFacade>,
    store: Option<Arc<dyn SessionStore>>,
    judge: Option<(Arc<dyn Provider>, String)>,
) -> Vec<Arc<dyn orrery_grader::Grader>> {
    #[cfg(feature = "graders")]
    {
        orrery_ext_graders::graders(broker, store, judge)
    }
    #[cfg(not(feature = "graders"))]
    {
        Vec::new()
    }
}
