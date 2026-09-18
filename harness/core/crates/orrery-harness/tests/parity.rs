//! Plan 18, task 4: **the kernel does not know which runtime it dispatched to.**
//!
//! One logical tool — `hello.parity` — written four times: compiled in
//! (`native`), as a node child process (`node`), and as two wasm components
//! built from the same `.wit` by two languages (`wasm-hello-rs`,
//! `wasm-hello-go`). The assertion is not "they all work". It is that the four
//! `Surface` values are **equal**, so nothing downstream could tell them apart
//! if it tried.
//!
//! This is objective 9 made mechanical. It fails the day the kernel grows a
//! special case for a runtime.
//!
//! # Why the test lives in the facade
//!
//! `orrery-harness` is the one core crate `cargo xtask deps-check` allows to
//! name an `extensions/` crate, and `native-hello` is one. It is also the crate
//! whose whole job is "assemble the runtimes", which is the claim under test.
//!
//! # What is compared, and what is not
//!
//! The `native` and `node` legs go through `ExtensionTable::call_tool` — the
//! dispatch boundary a turn uses — and produce an `orrery_proto::Outcome`. The
//! wasm leg goes through `WasmHost::call` directly, because `orrery-host-wasm`
//! does not yet implement `ExtensionHost` (plan 14 stopped at the engine). So
//! the comparison is made on the **`Surface`**, which is the value every one of
//! them hands back and the only thing a client ever sees. When the wasm host
//! grows its `ExtensionHost` impl, this test should compare whole `Outcome`s.
//!
//! # What is skipped, and never silently
//!
//! - No `node` on PATH → the node leg says so, out loud, and is left out.
//! - The TinyGo leg needs `tinygo` and `wit-bindgen-go`, which cargo cannot
//!   install, so it is behind `--features tinygo-examples`. Without the feature
//!   this test prints that the Go leg was **not compared**.

use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Duration;

use orrery_ext_api::ExtensionManifest;
use orrery_host::{ExtensionTable, NativeHost, NativeRegistry};
use orrery_host_rpc::RpcHost;
use orrery_proto::surface::{Cell, Surface, SurfaceKind};
use orrery_proto::{AgentScope, BranchId, Consent, ExtId, Grant, Layer, LoadOutcome, Outcome};
use orrery_tools::{CallCtx, ToolBudget};

/// The tool every runtime implements, under the extension id every one of them
/// declares. Same id, same tool name, four implementations.
const TOOL_REF: &str = "hello.parity";

fn examples() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../../../extensions/examples")
}

fn id() -> ExtId {
    "hello".parse().expect("a legal extension id")
}

fn ctx() -> CallCtx {
    CallCtx::new(
        orrery_proto::CallId::new(),
        orrery_proto::Subject::Agent,
        AgentScope {
            agent: "main".into(),
            branch: BranchId::new(),
            tools: vec!["*".into()],
            grant: Grant::nothing(),
        },
        ToolBudget::new(30_000, 1 << 20),
    )
}

/// `parity` asks for nothing, so every runtime can be granted the same thing:
/// nothing at all. A capability difference between the legs would make the
/// comparison about policy instead of about dispatch.
fn granted() -> Grant {
    Grant {
        capabilities: vec![],
        consent: Consent::Always,
    }
}

fn cell(text: &str) -> Cell {
    Cell {
        text: text.to_owned(),
        style: None,
    }
}

/// What all four must produce. Written out here rather than taken from one of
/// them, so a change in any single runtime is a failure rather than a new
/// baseline.
fn expected() -> Surface {
    Surface::new(SurfaceKind::Table {
        columns: vec!["key".to_owned(), "value".to_owned()],
        rows: vec![
            vec![cell("tool"), cell("parity")],
            vec![cell("runtime"), cell("irrelevant")],
        ],
    })
}

fn surface_of(label: &str, outcome: Outcome) -> Surface {
    match outcome {
        Outcome::Ok { surface, .. } => {
            surface.unwrap_or_else(|| panic!("{label}: an Ok outcome with no surface"))
        }
        other => panic!("{label}: expected Ok, got {other:?}"),
    }
}

fn program_present(program: &str) -> bool {
    std::process::Command::new(program)
        .arg("--version")
        .stdout(std::process::Stdio::null())
        .stderr(std::process::Stdio::null())
        .status()
        .is_ok_and(|s| s.success())
}

// --- native -----------------------------------------------------------------

async fn native() -> Surface {
    let mut natives = NativeRegistry::new();
    natives.register(Arc::new(native_hello::NativeHello::new()));
    let host = Arc::new(NativeHost::new(natives));
    let manifest = host.manifest_of(&id()).expect("the example's manifest");

    let table = ExtensionTable::new();
    let outcome = table.load(host, manifest, Layer::Project, granted()).await;
    assert!(
        matches!(outcome, LoadOutcome::Ok { .. }),
        "native: did not load cleanly: {outcome:?}"
    );

    surface_of(
        "native",
        table
            .call_tool(
                &TOOL_REF.parse().expect("a legal ref"),
                serde_json::json!({}),
                &ctx(),
            )
            .await
            .expect("native dispatch"),
    )
}

// --- node -------------------------------------------------------------------

async fn node() -> Option<Surface> {
    if !program_present("node") {
        eprintln!("NOT COMPARED: the node leg needs `node` on PATH.");
        return None;
    }
    let dir = examples().join("node-hello");
    let host = Arc::new(RpcHost::node());
    host.install(&id(), dir.clone());

    let table = ExtensionTable::new();
    let manifest =
        Arc::new(ExtensionManifest::from_path(dir.join("orrery.toml")).expect("it parses"));
    let outcome = table
        .load(host.clone(), manifest, Layer::Project, granted())
        .await;
    // `count_files` wants `read`, which nothing here grants, so the extension
    // degrades. `parity` asks for nothing and is unaffected — which is the
    // degrade rule working rather than a wrinkle in this test.
    assert!(
        matches!(outcome, LoadOutcome::Ok { .. } | LoadOutcome::Degraded { .. }),
        "node: did not load: {outcome:?} (stderr: {:?})",
        host.stderr_tail(&id())
    );

    let surface = surface_of(
        "node",
        table
            .call_tool(
                &TOOL_REF.parse().expect("a legal ref"),
                serde_json::json!({}),
                &ctx(),
            )
            .await
            .expect("node dispatch"),
    );
    table
        .unload(&id(), Duration::from_secs(2))
        .await
        .expect("the child goes away");
    Some(surface)
}

// --- wasm -------------------------------------------------------------------

async fn wasm(bytes: &[u8], label: &str) -> Surface {
    use orrery_host_wasm::{Ceilings, DenyAll, Outcome as WasmOutcome, WasmHost};

    let host = WasmHost::new().expect("the engine builds");
    let component = host.compile(bytes).expect("the component loads");
    let outcome = host
        .call(
            &component,
            Ceilings::DEFAULT,
            Arc::new(DenyAll::default()),
            host.cancel_handle(),
            "parity",
            "{}",
        )
        .await
        .unwrap_or_else(|e| panic!("{label}: the call broke: {e}"));
    match outcome {
        WasmOutcome::Ok(surface) => surface,
        other => panic!("{label}: expected a surface, got {other:?}"),
    }
}

fn build_wasm_rs() -> Vec<u8> {
    let dir = examples().join("wasm-hello-rs");
    // `--target-dir` explicitly, and the same path is read back. Without it an
    // inherited `CARGO_TARGET_DIR` — which this repo sets, because eleven
    // worktrees sharing one cache is the difference between 25 GB and 273 GB —
    // sends the build somewhere else and the read below silently returns a
    // **stale component from a previous run**. That failure looks like "the
    // guest is missing a tool", which is a bad hour.
    let out = dir.join("target");
    let status = std::process::Command::new(env!("CARGO"))
        .args(["build", "--release", "--target", "wasm32-wasip2"])
        .arg("--target-dir")
        .arg(&out)
        .current_dir(&dir)
        .status()
        .expect("cargo runs");
    assert!(status.success(), "the Rust example did not build");
    std::fs::read(out.join("wasm32-wasip2/release/wasm_hello_rs.wasm"))
        .expect("the component is where cargo put it")
}

#[cfg(feature = "tinygo-examples")]
fn build_wasm_go() -> Vec<u8> {
    let dir = examples().join("wasm-hello-go");
    let wit = Path::new("../../../../wit");

    let generated = std::process::Command::new("wit-bindgen-go")
        .args(["generate", "--world", "orrery-extension", "--out", "internal"])
        .arg(wit)
        .current_dir(&dir)
        .status()
        .expect("wit-bindgen-go is on PATH under the `tinygo-examples` feature");
    assert!(generated.success(), "wit-bindgen-go failed");

    let built = std::process::Command::new("tinygo")
        .args([
            "build",
            "-target=wasip2",
            "-o",
            "wasm-hello-go.wasm",
            "--wit-package",
        ])
        .arg(wit)
        .args(["--wit-world", "orrery-extension", "."])
        .current_dir(&dir)
        .status()
        .expect("tinygo is on PATH under the `tinygo-examples` feature");
    assert!(built.success(), "tinygo failed");

    std::fs::read(dir.join("wasm-hello-go.wasm")).expect("tinygo wrote the component")
}

async fn wasm_legs() -> Vec<(&'static str, Surface)> {
    #[allow(unused_mut)]
    let mut legs = vec![("wasm-rust", wasm(&build_wasm_rs(), "wasm-rust").await)];

    #[cfg(feature = "tinygo-examples")]
    legs.push(("wasm-go", wasm(&build_wasm_go(), "wasm-go").await));

    #[cfg(not(feature = "tinygo-examples"))]
    eprintln!(
        "NOT COMPARED: the TinyGo leg needs `--features tinygo-examples`, tinygo and \
         wit-bindgen-go. See extensions/examples/wasm-hello-go/README.md."
    );

    legs
}

// --- the test ---------------------------------------------------------------

#[tokio::test(flavor = "multi_thread")]
async fn all_runtimes_produce_the_same_output() {
    let want = expected();

    assert_eq!(native().await, want, "the native runtime disagreed");

    if let Some(node) = node().await {
        assert_eq!(node, want, "the node runtime disagreed");
    }

    for (label, surface) in wasm_legs().await {
        assert_eq!(surface, want, "the {label} runtime disagreed");
    }
}
