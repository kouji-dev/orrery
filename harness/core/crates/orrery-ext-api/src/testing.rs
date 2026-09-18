//! Testing an extension needs no model, no network and no kernel.
//!
//! This is the same code path `orrery ext test` uses (plan 17 wires the
//! command; the harness is here). A community author gets it because it ships
//! in a published crate: the ledger they see and the denials they see are the
//! ones a real session produces, not a second implementation that agrees with
//! the real one until it does not.
//!
//! ```
//! # use orrery_ext_api::testing::load_for_test;
//! # use orrery_ext_api::SpawnRequest;
//! # async fn example() {
//! let harness = load_for_test(MANIFEST, &["read:$WORKSPACE/**"]).unwrap();
//! let ctx = harness.ctx("impacted");
//! // no `spawn` grant, so:
//! assert!(ctx.broker.spawn(SpawnRequest::new("java", ["-version"])).await.is_err());
//! assert_eq!(harness.recorded().len(), 1);
//! # }
//! # const MANIFEST: &str = "";
//! ```

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use orrery_proto::{Aspect, CallId, Capability, LoadOutcome, Surface};
use parking_lot::Mutex;
use tokio_util::sync::CancellationToken;

use crate::broker::{
    BrokerError, BrokerFacade, BrokerResult, ListEntry, ListRequest, Listing, NetRequest,
    NetResponse, ReadChunk, ReadRequest, SpawnOutput, SpawnRequest, WriteRequest,
};
use crate::ctx::{CallCtx, SurfaceLog, SurfaceSink, ToolBudget};
use crate::manifest::{ExtensionManifest, ManifestError, scope_matches};

/// Something an extension asked the broker for.
///
/// `allowed` is the half that makes a test worth writing: an author asserts
/// both that the call was made and that it was refused, which is what catches
/// an extension that "handles" a denial by quietly doing nothing.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum BrokerCall {
    /// A file read.
    Read {
        /// Which file.
        path: PathBuf,
        /// How much was asked for.
        limit: u64,
        /// Whether the grants covered it.
        allowed: bool,
    },
    /// A directory listing.
    List {
        /// Where it looked.
        path: PathBuf,
        /// Whether it descended.
        recursive: bool,
        /// Whether the grants covered it.
        allowed: bool,
    },
    /// A file write.
    Write {
        /// Which file.
        path: PathBuf,
        /// How many bytes.
        bytes: usize,
        /// Whether the write was all-or-nothing.
        atomic: bool,
        /// Whether the grants covered it.
        allowed: bool,
    },
    /// A process.
    Spawn {
        /// The program.
        program: String,
        /// Its arguments.
        args: Vec<String>,
        /// Whether the grants covered it.
        allowed: bool,
    },
    /// An HTTP request.
    Fetch {
        /// The method.
        method: String,
        /// The URL.
        url: String,
        /// Whether the grants covered it.
        allowed: bool,
    },
    /// A credential read.
    Credential {
        /// Which credential.
        name: String,
        /// Whether the grants covered it.
        allowed: bool,
    },
    /// A credential write — what a login flow does when it has a token.
    StoreCredential {
        /// Which credential.
        name: String,
        /// Whether the grants covered it.
        allowed: bool,
    },
    /// A credential being forgotten — what `logout` does.
    ForgetCredential {
        /// Which credential.
        name: String,
        /// Whether the grants covered it.
        allowed: bool,
    },
}

/// A broker that grants exactly what a test says and records everything.
///
/// The denials are worded the way the real broker words them, so a test that
/// asserts on the message is not asserting on a fake.
#[derive(Debug, Default)]
pub struct MockBroker {
    grants: Vec<Capability>,
    recorded: Mutex<Vec<BrokerCall>>,
    files: Mutex<BTreeMap<PathBuf, Vec<u8>>>,
    spawns: Mutex<BTreeMap<String, SpawnOutput>>,
    fetches: Mutex<BTreeMap<String, NetResponse>>,
    creds: Mutex<BTreeMap<String, String>>,
}

impl MockBroker {
    /// A broker granting exactly these capabilities.
    #[must_use]
    pub fn new(grants: Vec<Capability>) -> Self {
        Self {
            grants,
            ..Self::default()
        }
    }

    /// What the extension asked for, in order.
    #[must_use]
    pub fn recorded(&self) -> Vec<BrokerCall> {
        self.recorded.lock().clone()
    }

    /// Put a file where the extension will look for it.
    pub fn add_file(&self, path: impl Into<PathBuf>, contents: impl Into<Vec<u8>>) {
        self.files.lock().insert(path.into(), contents.into());
    }

    /// What a program should appear to print.
    pub fn add_spawn_response(&self, program: impl Into<String>, stdout: impl Into<Vec<u8>>) {
        self.spawns.lock().insert(
            program.into(),
            SpawnOutput {
                status: Some(0),
                stdout: stdout.into(),
                stderr: Vec::new(),
                truncated: false,
            },
        );
    }

    /// What a URL should appear to answer.
    pub fn add_fetch_response(&self, url: impl Into<String>, response: NetResponse) {
        self.fetches.lock().insert(url.into(), response);
    }

    /// What a credential should appear to hold.
    pub fn add_credential(&self, name: impl Into<String>, value: impl Into<String>) {
        self.creds.lock().insert(name.into(), value.into());
    }

    /// Whether these grants cover this aspect and this name.
    #[must_use]
    pub fn allows(&self, aspect: Aspect, what: &str) -> bool {
        self.grants
            .iter()
            .filter(|c| c.aspect == aspect)
            .any(|c| c.scope.is_empty() || c.scope.iter().any(|p| scope_matches(p, what)))
    }

    fn check(&self, aspect: Aspect, what: &str) -> BrokerResult<()> {
        if self.allows(aspect, what) {
            Ok(())
        } else {
            Err(BrokerError::denied_aspect(aspect, what))
        }
    }

    fn record(&self, call: BrokerCall) {
        self.recorded.lock().push(call);
    }
}

#[async_trait]
impl BrokerFacade for MockBroker {
    async fn read(&self, req: ReadRequest) -> BrokerResult<ReadChunk> {
        let what = req.path.display().to_string();
        let allowed = self.allows(Aspect::Read, &what);
        self.record(BrokerCall::Read {
            path: req.path.clone(),
            limit: req.limit,
            allowed,
        });
        self.check(Aspect::Read, &what)?;

        let files = self.files.lock();
        let contents = files.get(&req.path).ok_or_else(|| BrokerError::Io {
            message: format!("no such file: {what}"),
        })?;
        let start = usize::try_from(req.offset)
            .unwrap_or(usize::MAX)
            .min(contents.len());
        let end = start
            .saturating_add(usize::try_from(req.limit).unwrap_or(usize::MAX))
            .min(contents.len());
        Ok(ReadChunk {
            bytes: contents[start..end].to_vec(),
            eof: end == contents.len(),
            total: Some(contents.len() as u64),
        })
    }

    async fn list(&self, req: ListRequest) -> BrokerResult<Listing> {
        let what = req.path.display().to_string();
        let allowed = self.allows(Aspect::Read, &what);
        self.record(BrokerCall::List {
            path: req.path.clone(),
            recursive: req.recursive,
            allowed,
        });

        // The files the test declared, filtered to the ones this call may both
        // see and read. A mock that named a file the grants do not cover would
        // teach an author the opposite of what a real session does.
        let files = self.files.lock();
        let mut entries: Vec<ListEntry> = Vec::new();
        for (path, contents) in files.iter() {
            let Ok(rest) = path.strip_prefix(&req.path) else {
                continue;
            };
            if !req.recursive && rest.components().count() > 1 {
                continue;
            }
            if !self.allows(Aspect::Read, &path.display().to_string()) {
                continue;
            }
            entries.push(ListEntry {
                path: path.clone(),
                is_dir: false,
                size: Some(contents.len() as u64),
            });
        }
        let limit = usize::try_from(req.limit).unwrap_or(usize::MAX);
        let truncated = entries.len() > limit;
        entries.truncate(limit);
        Ok(Listing { entries, truncated })
    }

    async fn write(&self, req: WriteRequest) -> BrokerResult<()> {
        let what = req.path.display().to_string();
        let allowed = self.allows(Aspect::Write, &what);
        self.record(BrokerCall::Write {
            path: req.path.clone(),
            bytes: req.contents.len(),
            atomic: req.atomic,
            allowed,
        });
        self.check(Aspect::Write, &what)?;
        self.files.lock().insert(req.path, req.contents);
        Ok(())
    }

    async fn spawn(&self, req: SpawnRequest) -> BrokerResult<SpawnOutput> {
        let allowed = self.allows(Aspect::Spawn, &req.program);
        self.record(BrokerCall::Spawn {
            program: req.program.clone(),
            args: req.args.clone(),
            allowed,
        });
        self.check(Aspect::Spawn, &req.program)?;
        Ok(self
            .spawns
            .lock()
            .get(&req.program)
            .cloned()
            .unwrap_or_default())
    }

    async fn fetch(&self, req: NetRequest) -> BrokerResult<NetResponse> {
        let allowed = self.allows(Aspect::Net, &req.url);
        self.record(BrokerCall::Fetch {
            method: req.method.clone(),
            url: req.url.clone(),
            allowed,
        });
        self.check(Aspect::Net, &req.url)?;
        self.fetches
            .lock()
            .get(&req.url)
            .cloned()
            .ok_or_else(|| BrokerError::Io {
                message: format!(
                    "the test declared no response for {url} — `ext test` never \
                     reaches the network",
                    url = req.url
                ),
            })
    }

    async fn credential(&self, name: &str) -> BrokerResult<String> {
        let allowed = self.allows(Aspect::Creds, name);
        self.record(BrokerCall::Credential {
            name: name.to_owned(),
            allowed,
        });
        self.check(Aspect::Creds, name)?;
        self.creds
            .lock()
            .get(name)
            .cloned()
            .ok_or_else(|| BrokerError::Io {
                message: format!("the test declared no credential `{name}`"),
            })
    }

    async fn store_credential(&self, name: &str, secret: &str) -> BrokerResult<()> {
        let allowed = self.allows(Aspect::Creds, name);
        self.record(BrokerCall::StoreCredential {
            name: name.to_owned(),
            allowed,
        });
        self.check(Aspect::Creds, name)?;
        self.creds.lock().insert(name.to_owned(), secret.to_owned());
        Ok(())
    }

    async fn forget_credential(&self, name: &str) -> BrokerResult<()> {
        let allowed = self.allows(Aspect::Creds, name);
        self.record(BrokerCall::ForgetCredential {
            name: name.to_owned(),
            allowed,
        });
        self.check(Aspect::Creds, name)?;
        self.creds.lock().remove(name);
        Ok(())
    }

    async fn has_credential(&self, name: &str) -> BrokerResult<bool> {
        // Deliberately **not** recorded: asking whether a name exists is what
        // `state()` does on every turn, and a ledger full of it would bury the
        // reads that matter.
        self.check(Aspect::Creds, name)?;
        Ok(self.creds.lock().contains_key(name))
    }
}

/// One extension, loaded the way a test session loads it.
pub struct TestHarness {
    manifest: Arc<ExtensionManifest>,
    /// The broker the extension is given. Also reachable through `ctx`.
    pub broker: Arc<MockBroker>,
    surfaces: SurfaceLog,
    outcome: LoadOutcome,
}

impl std::fmt::Debug for TestHarness {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("TestHarness")
            .field("ext", &self.manifest.name.as_str())
            .field("outcome", &self.outcome)
            .finish_non_exhaustive()
    }
}

impl TestHarness {
    /// The manifest, as parsed.
    #[must_use]
    pub fn manifest(&self) -> &Arc<ExtensionManifest> {
        &self.manifest
    }

    /// The ledger entry a real session would show for this load.
    #[must_use]
    pub fn load_outcome(&self) -> LoadOutcome {
        self.outcome.clone()
    }

    /// What the extension asked the broker for, in order.
    #[must_use]
    pub fn recorded(&self) -> Vec<BrokerCall> {
        self.broker.recorded()
    }

    /// Every surface the extension described, as data. No client, no drawing.
    #[must_use]
    pub fn surfaces(&self) -> Vec<Surface> {
        self.surfaces.all()
    }

    /// A call context for one of the extension's tools.
    #[must_use]
    pub fn ctx(&self, tool: impl Into<String>) -> CallCtx {
        self.ctx_with(tool, ToolBudget::default(), CancellationToken::new())
    }

    /// A call context with a ceiling and a cancel token of the test's choosing.
    #[must_use]
    pub fn ctx_with(
        &self,
        tool: impl Into<String>,
        budget: ToolBudget,
        cancel: CancellationToken,
    ) -> CallCtx {
        CallCtx::new(
            CallId::new(),
            self.manifest.name.clone(),
            tool,
            budget,
            cancel,
            self.broker.clone(),
        )
        .with_ui(SurfaceSink::to(Arc::new(self.surfaces.clone())))
    }

    /// Put a file where the extension will look for it.
    #[must_use]
    pub fn with_file(self, path: impl Into<PathBuf>, contents: impl Into<Vec<u8>>) -> Self {
        self.broker.add_file(path, contents);
        self
    }

    /// Declare what a program prints.
    #[must_use]
    pub fn with_spawn_response(
        self,
        program: impl Into<String>,
        stdout: impl Into<Vec<u8>>,
    ) -> Self {
        self.broker.add_spawn_response(program, stdout);
        self
    }

    /// Declare what a credential holds.
    #[must_use]
    pub fn with_credential(self, name: impl Into<String>, value: impl Into<String>) -> Self {
        self.broker.add_credential(name, value);
        self
    }
}

/// Load an extension's manifest against a set of grants and report what a real
/// session would report.
///
/// Each grant is `aspect` or `aspect:scope`, as a person writes it: `"spawn"`,
/// `"read:$WORKSPACE/**"`, `"net:https://example.test/**"`.
///
/// # Errors
///
/// [`ManifestError`] when the manifest would be refused at load — the same
/// refusal, at the same [`LoadStage`](orrery_proto::LoadStage), the host gives.
pub fn load_for_test(manifest: &str, grants: &[&str]) -> Result<TestHarness, ManifestError> {
    let manifest = ExtensionManifest::from_toml_str(manifest, "orrery.toml")?;
    Ok(load_parsed_for_test(manifest, grants))
}

/// The same, for a manifest that is already parsed.
///
/// A **compiled-in** bundle has no `orrery.toml` anywhere on disk: its manifest
/// is a value its own Rust code returns. Without this, `orrery ext test builtin`
/// has nothing to hand [`load_for_test`] and the one runtime that ships in every
/// build is the one runtime the test harness cannot reach — which would make
/// `native` a back door after all.
#[must_use]
pub fn load_parsed_for_test(manifest: ExtensionManifest, grants: &[&str]) -> TestHarness {
    let grants: Vec<Capability> = grants.iter().copied().map(parse_grant).collect();

    let problems = missing(&manifest, &grants);
    let contributions = manifest.contributions();
    let outcome = if problems.is_empty() {
        LoadOutcome::Ok {
            ext: manifest.name.clone(),
            contributions,
            ms: 0,
        }
    } else {
        LoadOutcome::Degraded {
            ext: manifest.name.clone(),
            contributions,
            ms: 0,
            problems,
        }
    };

    TestHarness {
        manifest: Arc::new(manifest),
        broker: Arc::new(MockBroker::new(grants)),
        surfaces: SurfaceLog::default(),
        outcome,
    }
}

/// Load an extension's manifest from disk against a set of grants.
///
/// # Errors
///
/// [`ManifestError`], as [`load_for_test`].
pub fn load_path_for_test(
    manifest: impl AsRef<Path>,
    grants: &[&str],
) -> Result<TestHarness, ManifestError> {
    let src =
        std::fs::read_to_string(manifest.as_ref()).map_err(|e| ManifestError::Unreadable {
            file: manifest.as_ref().display().to_string(),
            message: e.to_string(),
        })?;
    load_for_test(&src, grants)
}

/// What the manifest asked for and the grants did not cover, in the words the
/// ledger shows. The host calls the same function.
#[must_use]
pub fn missing(manifest: &ExtensionManifest, grants: &[Capability]) -> Vec<String> {
    manifest.unmet(grants)
}

/// `aspect` or `aspect:scope`. An unknown aspect becomes a grant of nothing
/// rather than a panic, because a typo in a test should fail the assertion the
/// test is about, not blow up somewhere else.
fn parse_grant(raw: &str) -> Capability {
    let (aspect, scope) = match raw.split_once(':') {
        // `mem.read:x` — the aspect itself contains a dot, never a colon.
        Some((a, s)) => (a, Some(s)),
        None => (raw, None),
    };
    let aspect = serde_json::from_value::<Aspect>(serde_json::Value::String(aspect.to_owned()))
        .unwrap_or(Aspect::Ext);
    match scope {
        Some(scope) => Capability::scoped(aspect, [scope]),
        None => Capability::all(aspect),
    }
}
