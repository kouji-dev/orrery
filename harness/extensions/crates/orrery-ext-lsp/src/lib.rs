//! Language-server tools: hover, definitions, references and diagnostics over a managed LSP client.
//!
//! Four read-only verbs as a first-party extension, loaded through
//! `orrery-host` like any third-party one: the ledger shows it, a deny rule
//! disables it, `orrery ext test` runs it.
//!
//! # What a model gets out of this that it cannot get from `grep`
//!
//! `references` finds the uses of *this* symbol, not the places that string
//! appears. That difference is the whole reason to pay for a language server:
//! renaming `new` across a codebase with `grep` is how a model breaks a build.
//!
//! # The seams
//!
//! - [`client::LspTransport`] is how a server is reached, and it is a trait
//!   because a test must not need `rust-analyzer` installed. `tests/tools.rs`
//!   drives a fake language server that lives in the test process.
//! - [`child::ChildTransport`] is the real one: a process, `Content-Length`
//!   framing, and a reader thread correlating responses by id.
//! - [`Servers`] decides which server serves which file. It is data, not code,
//!   so a profile can add a language without a new build.
//!
//! # Where the framing lives, and where it should live
//!
//! Plan 06 puts framing in `orrery-jsonrpc`. That crate is `publish = false`,
//! and `deps-check` rule 2 forbids an extension from depending on an
//! unpublished core crate, so [`framing`] is sixty lines here instead. When
//! `orrery-jsonrpc` publishes, delete this module and use its — nothing else in
//! the crate touches bytes.
//!
//! # `symbols` is not here
//!
//! The scaffold's manifest declared it. `workspace/symbol` needs a whole
//! indexing story to be useful and this round did not build one, so it is not
//! declared: a manifest that lists a tool the extension does not have makes the
//! ledger a lie.
//!
//! Implementation plan: `harness/docs/plans/06-extension-host.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod child;
pub mod client;
pub mod framing;

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use async_trait::async_trait;
use orrery_ext_api::{
    BrokerError, CallCtx, HostError, NativeExtension, ReadRequest, ToolDef,
};
use orrery_proto::{Aspect, CancelReason, Outcome};
use parking_lot::Mutex;
use serde_json::{Value, json};

use crate::client::{LspClient, LspError, LspTransport};

/// The manifest this extension ships, parsed by the same parser a third
/// party's is.
pub const MANIFEST: &str = include_str!("../orrery.toml");

/// The most bytes of a file this bundle will send to a server.
///
/// A language server wants the whole document, and a tool that read a
/// gigabyte-long generated file into a request would take the session with it.
const MAX_DOC_BYTES: u64 = 4 << 20;

/// Which server serves which extension, and what to start it with.
///
/// Data rather than code: a profile adds a language by adding a row, and a
/// build that hard-coded the three the author happened to use would make every
/// other language a fork.
#[derive(Clone, Debug, Default)]
pub struct Servers(Vec<ServerSpec>);

/// One language server.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ServerSpec {
    /// File extensions it serves, without the dot: `["rs"]`.
    pub extensions: Vec<String>,
    /// The LSP `languageId` to announce: `rust`.
    pub language_id: String,
    /// The program.
    pub program: String,
    /// Its arguments.
    pub args: Vec<String>,
}

impl Servers {
    /// No servers. Every tool then reports that nothing serves the file, which
    /// is the honest answer for a build nobody configured.
    #[must_use]
    pub fn none() -> Self {
        Self::default()
    }

    /// The three this repository is written in. A starting point, not a
    /// closed set.
    #[must_use]
    pub fn common() -> Self {
        Self(vec![
            ServerSpec {
                extensions: vec!["rs".to_owned()],
                language_id: "rust".to_owned(),
                program: "rust-analyzer".to_owned(),
                args: Vec::new(),
            },
            ServerSpec {
                extensions: vec![
                    "ts".to_owned(),
                    "tsx".to_owned(),
                    "js".to_owned(),
                    "jsx".to_owned(),
                ],
                language_id: "typescript".to_owned(),
                program: "typescript-language-server".to_owned(),
                args: vec!["--stdio".to_owned()],
            },
            ServerSpec {
                extensions: vec!["py".to_owned()],
                language_id: "python".to_owned(),
                program: "pyright-langserver".to_owned(),
                args: vec!["--stdio".to_owned()],
            },
        ])
    }

    /// Add one.
    #[must_use]
    pub fn with(mut self, spec: ServerSpec) -> Self {
        self.0.push(spec);
        self
    }

    /// Which server serves this path, if any.
    #[must_use]
    pub fn for_path(&self, path: &Path) -> Option<&ServerSpec> {
        let ext = path.extension()?.to_str()?.to_ascii_lowercase();
        self.0.iter().find(|s| s.extensions.iter().any(|e| *e == ext))
    }
}

/// How a server is started, so a test can start a fake one.
///
/// The bundle owns *when* a server is started and how long it is kept; a
/// `Launcher` owns *what* gets started. Splitting them is what lets
/// `tests/tools.rs` exercise the keeping without a process.
#[async_trait]
pub trait Launcher: Send + Sync {
    /// Start a server for this spec, rooted here.
    ///
    /// # Errors
    ///
    /// [`LspError::NotRunning`] when it will not start — the normal case for a
    /// language server that is simply not installed.
    async fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
    ) -> Result<Arc<dyn LspTransport>, LspError>;
}

/// Starts real processes.
#[derive(Debug, Default, Clone, Copy)]
pub struct SpawnLauncher;

#[async_trait]
impl Launcher for SpawnLauncher {
    async fn launch(
        &self,
        spec: &ServerSpec,
        root: &Path,
    ) -> Result<Arc<dyn LspTransport>, LspError> {
        let transport = child::ChildTransport::spawn(&spec.program, &spec.args, Some(root))?;
        Ok(Arc::new(transport))
    }
}

/// The language-server bundle.
pub struct LspTools {
    servers: Servers,
    launcher: Arc<dyn Launcher>,
    /// One client per program, kept for the life of the bundle.
    ///
    /// Started per call, a language server would re-index the tree on every
    /// hover — which for a large repository is tens of seconds each time, and
    /// is the difference between this bundle being useful and being a way to
    /// burn a turn budget.
    clients: Mutex<HashMap<String, Arc<LspClient>>>,
}

impl std::fmt::Debug for LspTools {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("LspTools")
            .field("running", &self.clients.lock().len())
            .finish_non_exhaustive()
    }
}

impl Default for LspTools {
    fn default() -> Self {
        Self::new()
    }
}

impl LspTools {
    /// The bundle, starting real servers for the common languages.
    #[must_use]
    pub fn new() -> Self {
        Self::with_launcher(Servers::common(), Arc::new(SpawnLauncher))
    }

    /// The bundle over a server table and a launcher of the caller's choosing.
    #[must_use]
    pub fn with_launcher(servers: Servers, launcher: Arc<dyn Launcher>) -> Self {
        Self {
            servers,
            launcher,
            clients: Mutex::new(HashMap::new()),
        }
    }

    /// The client for this file, started if it is not running.
    async fn client_for(
        &self,
        path: &Path,
        root: &Path,
    ) -> Result<(Arc<LspClient>, &ServerSpec), LspError> {
        let spec = self.servers.for_path(path).ok_or_else(|| {
            LspError::NotRunning(format!(
                "no language server is configured for {}",
                path.display()
            ))
        })?;
        if let Some(client) = self.clients.lock().get(&spec.program).cloned() {
            return Ok((client, spec));
        }
        let transport = self.launcher.launch(spec, root).await?;
        let client = Arc::new(LspClient::new(transport, uri_of(root)));
        // Re-check under the lock: two concurrent calls for the same language
        // would otherwise leave one server running with nobody holding it.
        let client = {
            let mut held = self.clients.lock();
            held.entry(spec.program.clone())
                .or_insert(client)
                .clone()
        };
        Ok((client, spec))
    }

    /// Stop every server this bundle started.
    pub async fn shutdown(&self) {
        let clients: Vec<_> = self.clients.lock().drain().map(|(_, c)| c).collect();
        for client in clients {
            client.shutdown().await;
        }
    }
}

/// A `file://` URI for a path, with forward slashes and a drive letter that
/// does not start the path component on Windows.
#[must_use]
pub fn uri_of(path: &Path) -> String {
    let text = path.display().to_string().replace('\\', "/");
    if text.starts_with('/') {
        format!("file://{text}")
    } else {
        // `C:/x` becomes `file:///C:/x`, which is what every language server
        // and every editor on Windows agrees on.
        format!("file:///{text}")
    }
}

fn position(input: &Value) -> Value {
    json!({
        "line": input.get("line").and_then(Value::as_u64).unwrap_or(0),
        "character": input.get("character").and_then(Value::as_u64).unwrap_or(0),
    })
}

/// What the model is told when the server could not answer.
fn failed(e: &LspError) -> Outcome {
    let code = match e {
        LspError::NotRunning(_) => "no_language_server",
        LspError::Refused { .. } => "refused",
        LspError::Transport(_) => "transport",
        LspError::TimedOut { .. } => "timeout",
    };
    Outcome::Failed {
        code: code.to_owned(),
        message: e.to_string(),
    }
}

#[async_trait]
impl NativeExtension for LspTools {
    fn manifest(&self) -> &str {
        MANIFEST
    }

    fn tools(&self) -> Vec<ToolDef> {
        let at_a_position = json!({
            "type": "object",
            "required": ["path"],
            "properties": {
                "path": { "type": "string", "description": "The file." },
                "line": {
                    "type": "integer",
                    "description": "Zero-based, as LSP counts.",
                    "minimum": 0,
                },
                "character": {
                    "type": "integer",
                    "description": "Zero-based, as LSP counts.",
                    "minimum": 0,
                },
            },
        });
        // `read` **and** `spawn`: without both in `requires`, a build with no
        // `spawn` grant would discover it at the first hover instead of saying
        // so in the ledger at load.
        let needs = [Aspect::Read, Aspect::Spawn];

        vec![
            ToolDef::new("hover")
                .described("What the language server knows about the symbol at a position.")
                .with_schema(at_a_position.clone())
                .requiring(needs),
            ToolDef::new("definition")
                .described("Where the symbol at a position is defined.")
                .with_schema(at_a_position.clone())
                .requiring(needs),
            ToolDef::new("references")
                .described(
                    "Every use of the symbol at a position. Finds uses of the \
                     symbol, not places the string appears.",
                )
                .with_schema(at_a_position)
                .requiring(needs),
            ToolDef::new("diagnostics")
                .described("Errors and warnings the language server has published for a file.")
                .with_schema(json!({
                    "type": "object",
                    "required": ["path"],
                    "properties": {
                        "path": { "type": "string", "description": "The file." },
                    },
                }))
                .requiring(needs),
        ]
    }

    async fn call(&self, tool: &str, input: Value, ctx: &CallCtx) -> Result<Outcome, HostError> {
        let Some(path) = input.get("path").and_then(Value::as_str) else {
            return Ok(Outcome::Failed {
                code: "bad_request".to_owned(),
                message: format!("`{tool}` needs a `path`"),
            });
        };
        let path = PathBuf::from(path);

        // The document itself comes through the broker: the server is told what
        // this call is allowed to read, and nothing else. Reading it with
        // `std::fs` would hand a language server a file the policy refuses.
        let chunk = match ctx
            .broker
            .read(ReadRequest::new(path.clone(), MAX_DOC_BYTES))
            .await
        {
            Ok(chunk) => chunk,
            Err(e @ (BrokerError::Denied { .. } | BrokerError::Cancelled { .. })) => {
                return Ok(e.into_outcome());
            }
            Err(e) => {
                return Ok(Outcome::Failed {
                    code: "io".to_owned(),
                    message: e.to_string(),
                });
            }
        };
        if ctx.is_cancelled() {
            return Ok(Outcome::Cancelled {
                reason: CancelReason::User,
            });
        }
        let text = String::from_utf8_lossy(&chunk.bytes).into_owned();

        let root = path.parent().unwrap_or(Path::new(".")).to_path_buf();
        let (client, language_id) = match self.client_for(&path, &root).await {
            Ok((client, spec)) => (client, spec.language_id.clone()),
            Err(e) => return Ok(failed(&e)),
        };
        let uri = uri_of(&path);
        if let Err(e) = client.open(&uri, &language_id, &text).await {
            return Ok(failed(&e));
        }

        let doc = json!({ "uri": uri });
        let at = json!({ "textDocument": doc, "position": position(&input) });

        Ok(match tool {
            "hover" => match client.request("textDocument/hover", at).await {
                Ok(result) => {
                    let text = hover_text(&result);
                    Outcome::Ok {
                        surface: Some(ctx.ui.markdown(text.clone(), true)),
                        value: Some(json!({ "contents": text })),
                    }
                }
                Err(e) => failed(&e),
            },
            "definition" => match client.request("textDocument/definition", at).await {
                Ok(result) => locations_outcome(ctx, &locations(&result)),
                Err(e) => failed(&e),
            },
            "references" => {
                let mut params = at;
                if let Some(map) = params.as_object_mut() {
                    // Without this the server omits the declaration itself, and
                    // "every use" quietly means "every use but one".
                    map.insert(
                        "context".to_owned(),
                        json!({ "includeDeclaration": true }),
                    );
                }
                match client.request("textDocument/references", params).await {
                    Ok(result) => locations_outcome(ctx, &locations(&result)),
                    Err(e) => failed(&e),
                }
            }
            "diagnostics" => {
                let items = client.diagnostics(&uri).await;
                let rows: Vec<Vec<String>> = items
                    .iter()
                    .map(|d| {
                        vec![
                            format!(
                                "{}:{}",
                                d["range"]["start"]["line"].as_u64().unwrap_or(0) + 1,
                                d["range"]["start"]["character"].as_u64().unwrap_or(0) + 1
                            ),
                            severity(d["severity"].as_u64()).to_owned(),
                            d["message"].as_str().unwrap_or_default().to_owned(),
                        ]
                    })
                    .collect();
                let surface = if rows.is_empty() {
                    ctx.ui.text("no diagnostics")
                } else {
                    ctx.ui.table(["at", "severity", "message"], rows)
                };
                Outcome::Ok {
                    surface: Some(surface),
                    value: Some(json!(items)),
                }
            }
            other => {
                return Err(HostError::NoSuchTool {
                    ext: ctx.ext.clone(),
                    tool: other.to_owned(),
                });
            }
        })
    }

    async fn deactivate(&self) -> Result<(), HostError> {
        self.shutdown().await;
        Ok(())
    }
}

/// LSP severity numbers, which no model should be asked to remember.
fn severity(n: Option<u64>) -> &'static str {
    match n {
        Some(1) => "error",
        Some(2) => "warning",
        Some(3) => "information",
        Some(4) => "hint",
        _ => "unknown",
    }
}

/// `Hover.contents` in all three shapes the protocol has had.
fn hover_text(result: &Value) -> String {
    let contents = &result["contents"];
    if let Some(s) = contents.as_str() {
        return s.to_owned();
    }
    if let Some(s) = contents["value"].as_str() {
        return s.to_owned();
    }
    if let Some(items) = contents.as_array() {
        return items
            .iter()
            .filter_map(|i| {
                i.as_str()
                    .map(ToOwned::to_owned)
                    .or_else(|| i["value"].as_str().map(ToOwned::to_owned))
            })
            .collect::<Vec<_>>()
            .join("\n\n");
    }
    String::new()
}

/// One place in a file.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Location {
    /// The document.
    pub uri: String,
    /// Zero-based, as LSP counts.
    pub line: u64,
    /// Zero-based, as LSP counts.
    pub character: u64,
}

/// `Location`, `Location[]` or `LocationLink[]` — the protocol allows all three
/// and servers disagree about which they send.
#[must_use]
pub fn locations(result: &Value) -> Vec<Location> {
    let one = |v: &Value| -> Option<Location> {
        // A `LocationLink` names the target differently from a `Location`.
        let uri = v["uri"]
            .as_str()
            .or_else(|| v["targetUri"].as_str())?
            .to_owned();
        let range = if v["range"].is_null() {
            &v["targetSelectionRange"]
        } else {
            &v["range"]
        };
        Some(Location {
            uri,
            line: range["start"]["line"].as_u64().unwrap_or(0),
            character: range["start"]["character"].as_u64().unwrap_or(0),
        })
    };
    match result {
        Value::Array(items) => items.iter().filter_map(one).collect(),
        Value::Null => Vec::new(),
        v => one(v).into_iter().collect(),
    }
}

fn locations_outcome(ctx: &CallCtx, found: &[Location]) -> Outcome {
    let surface = if found.is_empty() {
        ctx.ui.text("nothing found")
    } else {
        ctx.ui.table(
            ["uri", "line", "character"],
            found.iter().map(|l| {
                vec![
                    l.uri.clone(),
                    // One-based for a person, zero-based in `value` for the
                    // model: an editor counts from one and LSP counts from
                    // zero, and quietly picking one is how an off-by-one ships.
                    (l.line + 1).to_string(),
                    (l.character + 1).to_string(),
                ]
            }),
        )
    };
    Outcome::Ok {
        surface: Some(surface),
        value: Some(json!(
            found
                .iter()
                .map(|l| json!({
                    "uri": l.uri,
                    "line": l.line,
                    "character": l.character,
                }))
                .collect::<Vec<_>>()
        )),
    }
}
