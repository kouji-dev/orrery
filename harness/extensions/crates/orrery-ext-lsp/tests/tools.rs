//! The four verbs, against a fake language server in this process.
//!
//! **No process is started anywhere in this file, and nothing is installed.**
//! `FakeServer` implements [`LspTransport`] by answering from a table, which is
//! the only way `rust-analyzer`-shaped behaviour — a `LocationLink` rather than
//! a `Location`, diagnostics arriving as a push some time after `didOpen` — is
//! reachable on a machine that has no language server on it.
//!
//! The framing tests are separate, in `tests/framing.rs`, because framing is
//! the one part of this crate that a fake cannot stand in for.

use std::path::Path;
use std::sync::Arc;

use async_trait::async_trait;
use orrery_ext_api::testing::{TestHarness, load_for_test};
use orrery_ext_api::{CallCtx, NativeExtension, ToolBudget};
use orrery_ext_lsp::client::{LspError, LspTransport};
use orrery_ext_lsp::{Launcher, LspTools, MANIFEST, ServerSpec, Servers, uri_of};
use orrery_proto::Outcome;
use parking_lot::Mutex;
use serde_json::{Value, json};

/// What the fake server was asked, and what it answered.
#[derive(Default)]
struct Script {
    /// `method` → the `result` to answer with.
    answers: Vec<(String, Value)>,
    /// Requests seen, in order, as `(method, params)`.
    seen: Vec<(String, Value)>,
    /// Notifications *sent to* the server, in order.
    sent: Vec<(String, Value)>,
    /// Notifications waiting to be drained *by* the client.
    queued: Vec<(String, Value)>,
    /// A method that fails however it is called.
    refuse: Option<String>,
}

#[derive(Default)]
struct FakeServer(Mutex<Script>);

impl FakeServer {
    fn answering(pairs: &[(&str, Value)]) -> Arc<Self> {
        let me = Arc::new(Self::default());
        me.0.lock().answers = pairs
            .iter()
            .map(|(m, v)| ((*m).to_owned(), v.clone()))
            .collect();
        me
    }

    /// Publish diagnostics the way a real server does: at some moment of its
    /// own choosing, as a notification nobody asked for.
    fn publish(&self, uri: &str, diagnostics: Value) {
        self.0.lock().queued.push((
            "textDocument/publishDiagnostics".to_owned(),
            json!({ "uri": uri, "diagnostics": diagnostics }),
        ));
    }

    fn refusing(self: Arc<Self>, method: &str) -> Arc<Self> {
        self.0.lock().refuse = Some(method.to_owned());
        self
    }

    fn methods(&self) -> Vec<String> {
        self.0.lock().seen.iter().map(|(m, _)| m.clone()).collect()
    }

    fn notifications(&self) -> Vec<String> {
        self.0.lock().sent.iter().map(|(m, _)| m.clone()).collect()
    }

    fn params_for(&self, method: &str) -> Option<Value> {
        self.0
            .lock()
            .seen
            .iter()
            .find(|(m, _)| m == method)
            .map(|(_, p)| p.clone())
    }
}

#[async_trait]
impl LspTransport for FakeServer {
    async fn request(&self, _id: i64, method: &str, params: Value) -> Result<Value, LspError> {
        // A real round trip suspends; answering without yielding would hide
        // every ordering bug this file exists to catch.
        tokio::task::yield_now().await;
        let mut script = self.0.lock();
        script.seen.push((method.to_owned(), params));
        if script.refuse.as_deref() == Some(method) {
            return Err(LspError::Refused {
                method: method.to_owned(),
                message: "the fake server refuses this one".to_owned(),
            });
        }
        Ok(script
            .answers
            .iter()
            .find(|(m, _)| m == method)
            .map_or(Value::Null, |(_, v)| v.clone()))
    }

    async fn notify(&self, method: &str, params: Value) -> Result<(), LspError> {
        self.0.lock().sent.push((method.to_owned(), params));
        Ok(())
    }

    async fn drain_notifications(&self) -> Vec<(String, Value)> {
        std::mem::take(&mut self.0.lock().queued)
    }
}

/// Hands out the one fake server, and counts how often it was asked to.
struct FakeLauncher {
    server: Arc<FakeServer>,
    launches: Mutex<usize>,
}

#[async_trait]
impl Launcher for FakeLauncher {
    async fn launch(
        &self,
        _spec: &ServerSpec,
        _root: &Path,
    ) -> Result<Arc<dyn LspTransport>, LspError> {
        *self.launches.lock() += 1;
        Ok(Arc::clone(&self.server) as Arc<dyn LspTransport>)
    }
}

fn servers() -> Servers {
    Servers::none().with(ServerSpec {
        extensions: vec!["rs".to_owned()],
        language_id: "rust".to_owned(),
        program: "fake-analyzer".to_owned(),
        args: Vec::new(),
    })
}

struct Rig {
    tools: LspTools,
    harness: TestHarness,
    launcher: Arc<FakeLauncher>,
    path: String,
}

impl Rig {
    fn new(server: Arc<FakeServer>) -> Self {
        // An absolute path, because that is what the broker resolves and what a
        // `file://` uri needs.
        let path = "/work/src/lib.rs".to_owned();
        let harness =
            load_for_test(MANIFEST, &["read", "spawn"]).expect("the shipped manifest loads");
        harness
            .broker
            .add_file(&path, "fn main() {}\n".as_bytes().to_vec());
        let launcher = Arc::new(FakeLauncher {
            server,
            launches: Mutex::new(0),
        });
        Self {
            tools: LspTools::with_launcher(servers(), Arc::clone(&launcher) as Arc<dyn Launcher>),
            harness,
            launcher,
            path,
        }
    }

    fn ctx(&self, tool: &str) -> CallCtx {
        self.harness.ctx(tool)
    }

    async fn call(&self, tool: &str, input: Value) -> Outcome {
        self.tools
            .call(tool, input, &self.ctx(tool))
            .await
            .expect("the harness carried the call")
    }

    fn at(&self, line: u64, character: u64) -> Value {
        json!({ "path": self.path, "line": line, "character": character })
    }

    fn uri(&self) -> String {
        uri_of(Path::new(&self.path))
    }
}

fn value_of(outcome: &Outcome) -> &Value {
    match outcome {
        Outcome::Ok { value: Some(v), .. } => v,
        other => panic!("expected Ok with a value, got {other:?}"),
    }
}

#[tokio::test]
async fn the_shipped_manifest_matches_what_is_implemented() {
    let harness = load_for_test(MANIFEST, &["read", "spawn"]).expect("it parses");
    let declared: Vec<String> = harness
        .manifest()
        .contributions()
        .iter()
        .map(|c| c.name.clone())
        .collect();
    assert_eq!(
        declared,
        ["hover", "definition", "references", "diagnostics"]
    );
    let implemented: Vec<String> = LspTools::with_launcher(
        Servers::none(),
        Arc::new(FakeLauncher {
            server: Arc::new(FakeServer::default()),
            launches: Mutex::new(0),
        }),
    )
    .tools()
    .into_iter()
    .map(|t| t.name)
    .collect();
    // `symbols` is gone from both, which is the point: a manifest that lists a
    // tool the extension does not have makes the ledger a lie.
    assert_eq!(declared, implemented);
}

#[tokio::test]
async fn hover_handshakes_once_opens_the_document_and_answers() {
    let server = FakeServer::answering(&[(
        "textDocument/hover",
        json!({ "contents": { "kind": "markdown", "value": "fn main()" } }),
    )]);
    let rig = Rig::new(Arc::clone(&server));

    let out = rig.call("hover", rig.at(0, 3)).await;
    assert_eq!(value_of(&out)["contents"], "fn main()");

    // `initialize` before anything else, and `initialized` and `didOpen` as
    // notifications rather than requests — waiting for a notification is how a
    // client hangs on the first call.
    assert_eq!(rig.harness.surfaces().len(), 1, "one surface, described");
    assert_eq!(
        server.methods().first().map(String::as_str),
        Some("initialize")
    );
    assert_eq!(
        server.notifications(),
        ["initialized", "textDocument/didOpen"]
    );

    // A second call reuses the running server: starting one per call would
    // re-index the tree on every hover.
    let _ = rig.call("hover", rig.at(1, 0)).await;
    assert_eq!(*rig.launcher.launches.lock(), 1);
    assert_eq!(
        server
            .methods()
            .iter()
            .filter(|m| *m == "initialize")
            .count(),
        1
    );
    // And the second read is a `didChange`, not a second `didOpen`, which most
    // servers treat as a protocol error.
    assert_eq!(
        server.notifications(),
        [
            "initialized",
            "textDocument/didOpen",
            "textDocument/didChange"
        ]
    );
}

#[tokio::test]
async fn definition_understands_a_location_link() {
    // What rust-analyzer actually sends, which is not a `Location`.
    let server = FakeServer::answering(&[(
        "textDocument/definition",
        json!([{
            "targetUri": "file:///work/src/other.rs",
            "targetSelectionRange": {
                "start": { "line": 9, "character": 4 },
                "end": { "line": 9, "character": 8 },
            },
        }]),
    )]);
    let rig = Rig::new(server);
    let out = rig.call("definition", rig.at(0, 3)).await;
    let rows = value_of(&out).as_array().expect("an array").clone();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["uri"], "file:///work/src/other.rs");
    // Zero-based in `value`, because that is what LSP means and what a follow-up
    // request has to send back.
    assert_eq!(rows[0]["line"], 9);
}

#[tokio::test]
async fn definition_understands_a_bare_location_too() {
    let server = FakeServer::answering(&[(
        "textDocument/definition",
        json!({
            "uri": "file:///work/src/other.rs",
            "range": { "start": { "line": 2, "character": 0 }, "end": { "line": 2, "character": 3 } },
        }),
    )]);
    let rig = Rig::new(server);
    let rows = value_of(&rig.call("definition", rig.at(0, 3)).await)
        .as_array()
        .expect("an array")
        .clone();
    assert_eq!(rows.len(), 1);
    assert_eq!(rows[0]["line"], 2);
}

#[tokio::test]
async fn references_asks_for_the_declaration_too() {
    let server = FakeServer::answering(&[("textDocument/references", json!([]))]);
    let rig = Rig::new(Arc::clone(&server));
    let _ = rig.call("references", rig.at(0, 3)).await;

    let params = server
        .params_for("textDocument/references")
        .expect("it was asked");
    // Without this the server omits the declaration and "every use" quietly
    // means "every use but one".
    assert_eq!(params["context"]["includeDeclaration"], true);
}

#[tokio::test]
async fn nothing_found_is_an_empty_answer_not_a_failure() {
    let server = FakeServer::answering(&[("textDocument/definition", Value::Null)]);
    let rig = Rig::new(server);
    let out = rig.call("definition", rig.at(0, 3)).await;
    assert_eq!(value_of(&out).as_array().expect("an array").len(), 0);
}

#[tokio::test]
async fn diagnostics_are_the_latest_published_set_not_every_set_ever() {
    let server = FakeServer::answering(&[("textDocument/hover", Value::Null)]);
    let rig = Rig::new(Arc::clone(&server));
    let uri = rig.uri();

    server.publish(
        &uri,
        json!([{
            "range": { "start": { "line": 0, "character": 4 }, "end": { "line": 0, "character": 8 } },
            "severity": 1,
            "message": "cannot find `main`",
        }, {
            "range": { "start": { "line": 3, "character": 0 }, "end": { "line": 3, "character": 1 } },
            "severity": 2,
            "message": "unused variable",
        }]),
    );
    let out = rig.call("diagnostics", json!({ "path": rig.path })).await;
    let items = value_of(&out).as_array().expect("an array").clone();
    assert_eq!(items.len(), 2);
    assert_eq!(items[0]["message"], "cannot find `main`");

    // The person fixed one and the server republished. The *whole* set is
    // replaced: appending would keep showing an error already fixed.
    server.publish(
        &uri,
        json!([{
            "range": { "start": { "line": 3, "character": 0 }, "end": { "line": 3, "character": 1 } },
            "severity": 2,
            "message": "unused variable",
        }]),
    );
    let out = rig.call("diagnostics", json!({ "path": rig.path })).await;
    let items = value_of(&out).as_array().expect("an array").clone();
    assert_eq!(items.len(), 1, "{items:?}");
    assert_eq!(items[0]["message"], "unused variable");
}

#[tokio::test]
async fn diagnostics_that_arrive_during_another_request_are_not_lost() {
    let server = FakeServer::answering(&[("textDocument/hover", json!({ "contents": "x" }))]);
    let rig = Rig::new(Arc::clone(&server));
    // Published before anybody asks for diagnostics, which is exactly when a
    // real server publishes them.
    server.publish(
        &rig.uri(),
        json!([{
            "range": { "start": { "line": 0, "character": 0 }, "end": { "line": 0, "character": 1 } },
            "severity": 1,
            "message": "arrived during a hover",
        }]),
    );
    let _ = rig.call("hover", rig.at(0, 0)).await;
    let out = rig.call("diagnostics", json!({ "path": rig.path })).await;
    let items = value_of(&out).as_array().expect("an array").clone();
    assert_eq!(
        items.len(),
        1,
        "a push collected only by `diagnostics` is lost"
    );
}

#[tokio::test]
async fn a_file_no_server_serves_says_so() {
    let harness = load_for_test(MANIFEST, &["read", "spawn"]).expect("the manifest loads");
    harness.broker.add_file("/work/notes.md", b"hello".to_vec());
    let tools = LspTools::with_launcher(
        servers(),
        Arc::new(FakeLauncher {
            server: Arc::new(FakeServer::default()),
            launches: Mutex::new(0),
        }),
    );
    let out = tools
        .call(
            "hover",
            json!({ "path": "/work/notes.md" }),
            &harness.ctx("hover"),
        )
        .await
        .expect("the harness carried the call");
    match out {
        Outcome::Failed { code, message } => {
            assert_eq!(code, "no_language_server");
            assert!(message.contains("notes.md"), "{message}");
        }
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn a_server_that_refuses_is_reported_not_swallowed() {
    let server = FakeServer::answering(&[]).refusing("textDocument/hover");
    let rig = Rig::new(server);
    match rig.call("hover", rig.at(0, 0)).await {
        Outcome::Failed { code, .. } => assert_eq!(code, "refused"),
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn a_document_the_policy_refuses_never_reaches_the_server() {
    let server = FakeServer::answering(&[("textDocument/hover", json!({ "contents": "x" }))]);
    let harness =
        load_for_test(MANIFEST, &["read:/elsewhere/**", "spawn"]).expect("the manifest loads");
    let tools = LspTools::with_launcher(
        servers(),
        Arc::new(FakeLauncher {
            server: Arc::clone(&server),
            launches: Mutex::new(0),
        }),
    );
    let out = tools
        .call(
            "hover",
            json!({ "path": "/work/src/lib.rs", "line": 0, "character": 0 }),
            &harness.ctx("hover"),
        )
        .await
        .expect("the harness carried the call");

    assert!(matches!(out, Outcome::Denied { .. }), "{out:?}");
    // Handing a language server a file the policy refuses would leak it through
    // the server's own index, which outlives the call.
    assert!(server.methods().is_empty(), "{:?}", server.methods());
}

#[tokio::test]
async fn a_call_without_a_path_says_so() {
    let rig = Rig::new(FakeServer::answering(&[]));
    match rig.call("hover", json!({})).await {
        Outcome::Failed { code, .. } => assert_eq!(code, "bad_request"),
        other => panic!("expected Failed, got {other:?}"),
    }
}

#[tokio::test]
async fn an_unknown_tool_is_an_error_not_an_empty_answer() {
    let rig = Rig::new(FakeServer::answering(&[]));
    let e = rig
        .tools
        .call("rename", json!({ "path": rig.path }), &rig.ctx("rename"))
        .await
        .expect_err("no such tool");
    assert!(e.to_string().contains("rename"), "{e}");
}

#[tokio::test]
async fn a_cancelled_call_starts_no_server() {
    let server = FakeServer::answering(&[]);
    let rig = Rig::new(Arc::clone(&server));
    let cancel = tokio_util::sync::CancellationToken::new();
    cancel.cancel();
    let ctx = rig.harness.ctx_with("hover", ToolBudget::default(), cancel);
    let out = rig
        .tools
        .call("hover", rig.at(0, 0), &ctx)
        .await
        .expect("the harness carried the call");
    assert!(matches!(out, Outcome::Cancelled { .. }), "{out:?}");
    assert_eq!(*rig.launcher.launches.lock(), 0);
}

#[test]
fn a_uri_is_the_shape_every_editor_agrees_on() {
    assert_eq!(
        uri_of(Path::new("/work/src/lib.rs")),
        "file:///work/src/lib.rs"
    );
    // A Windows path: the drive letter must not start the path component, or
    // half the servers in the world reject it.
    assert_eq!(
        uri_of(Path::new(r"C:\work\src\lib.rs")),
        "file:///C:/work/src/lib.rs"
    );
}
