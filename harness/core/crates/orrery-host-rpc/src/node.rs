//! The host for extensions that are a child process: node, python, anything
//! that speaks the protocol.
//!
//! # The threat-model caveat (translation #13)
//!
//! A child process is a *real* boundary for file descriptors and for the host's
//! memory, and **not** a boundary for the operating system's opinion of who the
//! child is. A node extension runs with this process's OS privileges: it can
//! `require('fs')` and read whatever this user can read. The SDK's loader hook
//! strips `fs` and `child_process` from the isolate, which catches honest
//! mistakes and makes the brokered path the path of least resistance — it is
//! not a guarantee and must never be described as one. The wasm path (plan 14)
//! is the one with a real boundary.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use std::time::Instant;

use async_trait::async_trait;
use orrery_ext_api::{
    BrokerFacade, CallCtx, DeniesEverything, ExtensionManifest, HostError, RuntimeKind, ToolDef,
};
use orrery_host::ExtensionHost;
use orrery_jsonrpc::{Framing, RpcError};
use orrery_proto::{CancelReason, ContributionKind, ExtId, Grant, LoadOutcome, LoadStage, Outcome};
use parking_lot::RwLock;
use serde_json::Value;
use tokio::process::Command;

use crate::broker_bridge::BrokerBridge;
use crate::protocol;
use crate::spawn::Guest;

/// How long a guest gets to answer `ext/load` before it is written off.
///
/// A guest that never answers is the failure mode a timeout exists for: there
/// is no reply, no error and no EOF, so nothing else would ever settle. Ten
/// seconds is long enough for a cold `node` start on a laptop with an
/// antivirus in the way, and short enough that a session still starts.
pub const LOAD_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// What one loaded guest is, from the host's side.
struct Loaded {
    guest: Arc<Guest>,
    tools: Vec<ToolDef>,
    disabled: Vec<String>,
}

/// The host for child-process extensions.
pub struct RpcHost {
    runtime: RuntimeKind,
    framing: Framing,
    roots: RwLock<HashMap<ExtId, PathBuf>>,
    broker: RwLock<Arc<dyn BrokerFacade>>,
    loaded: RwLock<HashMap<ExtId, Arc<Loaded>>>,
}

impl std::fmt::Debug for RpcHost {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("RpcHost")
            .field("runtime", &self.runtime)
            .field("loaded", &self.loaded.read().len())
            .finish_non_exhaustive()
    }
}

impl RpcHost {
    /// A host for `node` extensions.
    #[must_use]
    pub fn node() -> Self {
        Self::for_runtime(RuntimeKind::Node)
    }

    /// A host for `python` extensions.
    #[must_use]
    pub fn python() -> Self {
        Self::for_runtime(RuntimeKind::Python)
    }

    /// A host for any other child process that speaks the protocol.
    #[must_use]
    pub fn process() -> Self {
        Self::for_runtime(RuntimeKind::Process)
    }

    /// A host for one child-process runtime.
    #[must_use]
    pub fn for_runtime(runtime: RuntimeKind) -> Self {
        Self {
            runtime,
            framing: Framing::ContentLength,
            roots: RwLock::new(HashMap::new()),
            broker: RwLock::new(Arc::new(DeniesEverything)),
            loaded: RwLock::new(HashMap::new()),
        }
    }

    /// Where an extension's files are. Its `[process]` command is relative to
    /// this, and so is the default entry point.
    /// Where an extension's files are, and where `@orrery/ext` is put so the
    /// guest can import it.
    ///
    /// The SDK travels in this binary (see [`crate::sdk`]) because there is
    /// nowhere else it could come from: the default build makes no network
    /// request, so `npm install` is not available, and an installed extension
    /// is a copied directory with no `node_modules` of its own. Without this,
    /// `orrery install ./node-hello` produced an extension whose first turn
    /// died with `ERR_MODULE_NOT_FOUND`.
    ///
    /// A directory that will not take the files is **not** fatal here: the load
    /// goes on and the guest fails with node's own message, which names the
    /// specifier it could not resolve. Failing the install instead would make a
    /// read-only extension directory unloadable for reasons unrelated to the
    /// extension.
    pub fn install(&self, ext: &ExtId, root: impl Into<PathBuf>) {
        let root = root.into();
        if self.runtime == RuntimeKind::Node
            && let Err(e) = crate::sdk::vendor(&root)
        {
            tracing::warn!(
                target: "orrery.host.rpc",
                ext = %ext,
                root = %root.display(),
                error = %e,
                "could not vendor `@orrery/ext` beside the extension"
            );
        }
        self.roots.write().insert(ext.clone(), root);
    }

    /// Give guests a broker to call back into. Until this is set they are
    /// denied everything, which is the direction a missing broker must fail in.
    pub fn set_broker(&self, broker: Arc<dyn BrokerFacade>) {
        *self.broker.write() = broker;
    }

    /// What a guest last complained about, for a report a person will read.
    #[must_use]
    pub fn stderr_tail(&self, ext: &ExtId) -> Option<String> {
        self.loaded.read().get(ext).map(|l| l.guest.stderr_tail())
    }

    /// The command that starts this extension.
    fn argv(&self, manifest: &ExtensionManifest) -> Result<Command, String> {
        let root = self
            .roots
            .read()
            .get(&manifest.name)
            .cloned()
            .ok_or_else(|| {
                format!(
                    "`{}` is a `{}` extension and the host does not know where its \
                     files are — call `RpcHost::install` first",
                    manifest.name, manifest.runtime
                )
            })?;

        let mut command = match &manifest.process {
            Some(spec) => {
                let program = if spec.command.starts_with("./") || spec.command.starts_with(".\\") {
                    root.join(&spec.command).to_string_lossy().into_owned()
                } else {
                    spec.command.clone()
                };
                let mut command = Command::new(program);
                command.args(&spec.args);
                for (key, value) in &spec.env {
                    command.env(key, value);
                }
                command
            }
            // No `[process]`: the runtime implies the argv. `node index.mjs` is
            // the convention `@orrery/ext` scaffolds.
            None => match self.runtime {
                RuntimeKind::Node => {
                    let mut command = Command::new("node");
                    command.arg(root.join("index.mjs"));
                    command
                }
                RuntimeKind::Python => {
                    let mut command = Command::new("python");
                    command.arg(root.join("main.py"));
                    command
                }
                other => {
                    return Err(format!(
                        "a `{other}` extension needs a [process] table naming the \
                         command to run"
                    ));
                }
            },
        };
        command.current_dir(&root);
        Ok(command)
    }
}

#[async_trait]
impl ExtensionHost for RpcHost {
    fn runtime(&self) -> RuntimeKind {
        self.runtime
    }

    async fn load(&self, manifest: Arc<ExtensionManifest>, grant: Grant) -> LoadOutcome {
        let started = Instant::now();
        let ext = manifest.name.clone();

        let command = match self.argv(&manifest) {
            Ok(command) => command,
            Err(message) => {
                return LoadOutcome::Failed {
                    ext,
                    stage: LoadStage::Resolve,
                    message,
                };
            }
        };

        let bridge: Arc<dyn orrery_jsonrpc::Handler> =
            Arc::new(BrokerBridge::new(ext.clone(), self.broker.read().clone()));
        let guest = match Guest::start(command, self.framing, bridge) {
            Ok(guest) => Arc::new(guest),
            Err(e) => {
                return LoadOutcome::Failed {
                    ext,
                    stage: LoadStage::Link,
                    message: e.to_string(),
                };
            }
        };

        // The guest tells us what it actually has. The manifest said what it
        // promised; the difference is the ledger's business.
        let cancel = tokio_util::sync::CancellationToken::new();
        let reply = match tokio::time::timeout(
            LOAD_TIMEOUT,
            guest
                .peer
                .call(protocol::LOAD, serde_json::json!({}), &cancel),
        )
        .await
        {
            Ok(reply) => reply,
            Err(_) => {
                return LoadOutcome::Failed {
                    ext,
                    stage: LoadStage::Activate,
                    message: format!(
                        "the guest did not answer `{load}` within {secs}s; stderr: {tail}",
                        load = protocol::LOAD,
                        secs = LOAD_TIMEOUT.as_secs(),
                        tail = guest.stderr_tail()
                    ),
                };
            }
        };
        let reply: protocol::LoadReply = match reply {
            Ok(value) => match serde_json::from_value(value) {
                Ok(reply) => reply,
                Err(e) => {
                    return LoadOutcome::Failed {
                        ext,
                        stage: LoadStage::Activate,
                        message: format!(
                            "the guest answered `{LOAD}` with something unusable: {e}",
                            LOAD = protocol::LOAD
                        ),
                    };
                }
            },
            Err(e) => {
                return LoadOutcome::Failed {
                    ext,
                    stage: LoadStage::Activate,
                    message: format!("{e}; stderr: {}", guest.stderr_tail()),
                };
            }
        };

        let tools: Vec<ToolDef> = reply
            .tools
            .into_iter()
            .map(protocol::ToolWire::into_def)
            .collect();
        let mut problems = reply.problems;

        for promised in &manifest.provides.tools {
            if !tools.iter().any(|t| &t.name == promised) {
                problems.push(format!(
                    "the manifest promises tool `{promised}`, and the extension does \
                     not contribute it"
                ));
            }
        }

        let (disabled, ungranted) = orrery_host::host::disabled_by_grant(&tools, &grant);
        problems.extend(ungranted);

        let live: Vec<String> = tools
            .iter()
            .filter(|t| !disabled.contains(&t.name))
            .map(|t| t.name.clone())
            .collect();
        let contributions: Vec<orrery_proto::Contribution> = manifest
            .contributions()
            .into_iter()
            .filter(|c| c.kind != ContributionKind::Tool || live.contains(&c.name))
            .collect();

        self.loaded.write().insert(
            ext.clone(),
            Arc::new(Loaded {
                guest,
                tools,
                disabled,
            }),
        );

        let ms = started.elapsed().as_millis() as u64;
        if problems.is_empty() {
            LoadOutcome::Ok {
                ext,
                contributions,
                ms,
            }
        } else {
            LoadOutcome::Degraded {
                ext,
                contributions,
                ms,
                problems,
            }
        }
    }

    fn tools(&self, ext: &ExtId) -> Vec<ToolDef> {
        self.loaded
            .read()
            .get(ext)
            .map(|l| l.tools.clone())
            .unwrap_or_default()
    }

    fn disabled(&self, ext: &ExtId) -> Vec<String> {
        self.loaded
            .read()
            .get(ext)
            .map(|l| l.disabled.clone())
            .unwrap_or_default()
    }

    async fn call(
        &self,
        ext: &ExtId,
        tool: &str,
        input: Value,
        ctx: CallCtx,
    ) -> Result<Outcome, HostError> {
        let loaded = self.loaded.read().get(ext).cloned();
        let Some(loaded) = loaded else {
            return Err(HostError::NotLoaded { ext: ext.clone() });
        };

        let params = protocol::CallParams {
            call: ctx.call.to_string(),
            tool: tool.to_owned(),
            input,
            budget: protocol::CeilingWire {
                wall_clock_ms: ctx.budget.wall_clock_ms,
                output_bytes: ctx.budget.output_bytes,
                memory_bytes: ctx.budget.memory_bytes,
            },
        };
        let params = serde_json::to_value(params).map_err(|e| HostError::Transport {
            ext: ext.clone(),
            message: e.to_string(),
        })?;

        // The call's own ceiling, enforced here: a guest that neither answers
        // nor dies must not hold a turn open forever.
        let deadline = std::time::Duration::from_millis(ctx.budget.wall_clock_ms.max(1));
        let answered =
            match tokio::time::timeout(deadline, loaded.peer_call(protocol::CALL, params, &ctx))
                .await
            {
                Ok(answered) => answered,
                Err(_) => {
                    return Ok(Outcome::Cancelled {
                        reason: CancelReason::Timeout,
                    });
                }
            };

        match answered {
            Ok(value) => {
                let reply: protocol::CallReply =
                    serde_json::from_value(value).map_err(|e| HostError::Misbehaved {
                        ext: ext.clone(),
                        message: format!("its answer is not an outcome: {e}"),
                    })?;
                Ok(reply.outcome)
            }
            // One call cancelled. The connection, and everything else on it,
            // carries on.
            Err(RpcError::Cancelled) => Ok(Outcome::Cancelled {
                reason: CancelReason::User,
            }),
            // The child is gone. `Transport` is the shape the table turns into
            // "the extension degrades and the session lives".
            Err(RpcError::Closed) => Err(HostError::Transport {
                ext: ext.clone(),
                message: format!(
                    "the extension's process exited during the call; stderr: {}",
                    loaded.guest.stderr_tail()
                ),
            }),
            Err(e) => Err(HostError::Transport {
                ext: ext.clone(),
                message: e.to_string(),
            }),
        }
    }

    async fn unload(&self, ext: &ExtId) -> Result<(), HostError> {
        let loaded = self.loaded.write().remove(ext);
        let Some(loaded) = loaded else {
            return Ok(());
        };

        // Ask nicely, briefly. A guest that will not go is killed with its whole
        // tree when the containment drops with the `Arc`.
        if loaded.guest.is_alive() {
            let cancel = tokio_util::sync::CancellationToken::new();
            let asked = tokio::time::timeout(
                std::time::Duration::from_millis(500),
                loaded
                    .guest
                    .peer
                    .call(protocol::SHUTDOWN, serde_json::json!({}), &cancel),
            )
            .await;
            if asked.is_err() {
                tracing::debug!(
                    target: "orrery.host.rpc",
                    %ext,
                    "the guest did not answer ext/shutdown; killing its tree"
                );
            }
        }
        Ok(())
    }
}

impl Loaded {
    /// Make one call, with this call's cancel token.
    async fn peer_call(
        &self,
        method: &str,
        params: Value,
        ctx: &CallCtx,
    ) -> Result<Value, RpcError> {
        self.guest.peer.call(method, params, &ctx.cancel).await
    }
}
