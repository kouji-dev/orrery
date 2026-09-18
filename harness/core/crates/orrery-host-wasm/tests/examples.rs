//! Two example extensions, in two languages, from the same `.wit`.
//!
//! The Rust one proves `orrery-guest` works. The TinyGo one proves the *world*
//! works — a language Orrery ships nothing for binds against it with
//! `wit-bindgen` alone. The second is the one that matters: if it is painful,
//! the WIT is wrong.
//!
//! The Go half is behind the `tinygo-examples` feature, because a machine
//! without TinyGo cannot build it. It is **switched off on purpose** rather
//! than silently skipped: without the feature this test says so, out loud.

mod common;

use std::path::{Path, PathBuf};
use std::sync::Arc;

use orrery_host_wasm::{Ceilings, Outcome, WasmHost};
use orrery_proto::surface::{Surface, SurfaceKind};

fn examples_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../../extensions/examples")
}

fn build_rust_example() -> Vec<u8> {
    let dir = examples_dir().join("wasm-hello-rs");
    // `--target-dir` explicitly, and the same path is read back. Without it an
    // inherited `CARGO_TARGET_DIR` sends the build elsewhere while the read
    // below returns a **stale component from a previous run** — a green test
    // over code that was never compiled.
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

/// Build the TinyGo example. Only called under `tinygo-examples`.
#[cfg(feature = "tinygo-examples")]
fn build_go_example() -> Vec<u8> {
    let dir = examples_dir().join("wasm-hello-go");
    let wit = Path::new("../../../wit");

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

async fn hello(bytes: &[u8]) -> Surface {
    let host = WasmHost::new().expect("the engine builds");
    let component = host.compile(bytes).expect("the example loads");
    let outcome = host
        .call(
            &component,
            Ceilings::DEFAULT,
            Arc::new(common::Fake::denying()),
            host.cancel_handle(),
            "hello",
            "",
        )
        .await
        .expect("the call runs");
    match outcome {
        Outcome::Ok(surface) => surface,
        other => panic!("the example should return a surface: {other:?}"),
    }
}

/// What both languages must produce: a titled column holding a line of text and
/// a two-column table. Compared structurally rather than by `Debug`, so the one
/// legitimate difference — the SDK's name in the table — is the only thing that
/// may differ.
fn shape(surface: &Surface) -> (String, Vec<String>, Vec<Vec<String>>) {
    let SurfaceKind::Stack { title, children, .. } = &surface.kind else {
        panic!("expected a stack: {surface:?}");
    };
    let SurfaceKind::Text { value, .. } = &children[0].kind else {
        panic!("expected text first: {surface:?}");
    };
    let SurfaceKind::Table { columns, rows } = &children[1].kind else {
        panic!("expected a table second: {surface:?}");
    };
    (
        format!("{}|{value}", title.clone().unwrap_or_default()),
        columns.clone(),
        rows.iter()
            .map(|row| row.iter().map(|c| c.text.clone()).collect())
            .collect(),
    )
}

#[tokio::test(flavor = "multi_thread")]
async fn both_load_and_dispatch() {
    let rust = hello(&build_rust_example()).await;
    let (heading, columns, rows) = shape(&rust);
    assert_eq!(heading, "hello|hello, world");
    assert_eq!(columns, ["language", "sdk"]);
    assert_eq!(rows[0][0], "rust");

    #[cfg(feature = "tinygo-examples")]
    {
        let go = hello(&build_go_example()).await;
        let (go_heading, go_columns, go_rows) = shape(&go);
        assert_eq!(
            (heading, columns),
            (go_heading, go_columns),
            "the two languages produced different surfaces from the same world"
        );
        assert_eq!(go_rows[0][0], "go");
        assert_eq!(rows[0].len(), go_rows[0].len());
    }

    #[cfg(not(feature = "tinygo-examples"))]
    {
        let _ = Path::new("");
        println!(
            "NOT COMPARED: the TinyGo half needs `--features tinygo-examples`, \
             tinygo and wit-bindgen-go. See extensions/examples/wasm-hello-go/README.md."
        );
    }
}

/// **`guest::hides_the_arena`.**
///
/// `wasm-hello-rs` is written entirely with `ui::section` / `ui::text` /
/// `ui::table` and contains no index anywhere. That it rebuilds into a nested
/// `Surface` at all is the proof: the SDK produced a valid arena on the
/// author's behalf.
#[tokio::test(flavor = "multi_thread")]
async fn the_sdk_hides_the_arena() {
    let source =
        std::fs::read_to_string(examples_dir().join("wasm-hello-rs/src/lib.rs")).expect("the example");
    assert!(
        !source.contains("children: vec!["),
        "the example touches arena indices; the SDK is not hiding them"
    );

    let surface = hello(&build_rust_example()).await;
    // Nesting is what the arena carried, and it is back.
    let SurfaceKind::Stack { children, .. } = &surface.kind else {
        panic!("expected a stack");
    };
    assert_eq!(children.len(), 2);
}

/// The other half of the SDK's job: a denial reaches the author's code as a
/// value, and the example reports it rather than failing.
#[tokio::test(flavor = "multi_thread")]
async fn the_sdk_hands_a_denial_to_the_author() {
    let host = WasmHost::new().expect("the engine builds");
    let component = host
        .compile(&build_rust_example())
        .expect("the example loads");
    let outcome = host
        .call(
            &component,
            Ceilings::DEFAULT,
            Arc::new(common::Fake::denying()),
            host.cancel_handle(),
            "try-spawn",
            "",
        )
        .await
        .expect("the call runs");

    match outcome {
        Outcome::Ok(surface) => {
            let SurfaceKind::Text { value, .. } = &surface.kind else {
                panic!("expected text: {surface:?}");
            };
            assert!(value.starts_with("not this time: denied:"), "{value}");
        }
        other => panic!("a denial must not trap: {other:?}"),
    }
}
