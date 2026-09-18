//! Phase 4: three ported extensions, run for real and turned into frames.
//!
//! §8 calls phase 4 "the honest test of the schema": three extensions that emit
//! real surfaces, render in both TUIs and in `--json`, and contain no drawing
//! code of their own. This crate is what makes that a test rather than a claim.
//!
//! # The path is the real one
//!
//! ```text
//! orrery.toml  →  NativeRegistry  →  ExtensionTable::load  →  Registry::dispatch
//!      →  ctx.ui.*  →  Outcome::Ok { surface }  →  SurfaceStore::emit  →  SurfacePatch
//!      →  Event::Delta  →  agui::Encoder  →  Frame  →  every client
//! ```
//!
//! Nothing here is a fixture. The manifest is parsed by the parser a third
//! party is held to, the extension is loaded through `orrery-host`, the call
//! goes through the tool registry and its policy check, and the frames are
//! produced by the kernel's own differ and encoder. The `.jsonl` files under
//! `clients/conformance/ported/` are **written from this**, and
//! `fixtures_are_what_the_extensions_produce` fails if they drift — which is
//! what lets the Ink suite, which cannot link Rust, snapshot the same thing.
//!
//! # Determinism
//!
//! Turn and surface ids are uuids, and a fixture full of fresh uuids would
//! differ on every run. So the turn id is a constant here, and the surface ids
//! are the ones **the extensions mint** — which is the decided rule (plan 09,
//! open question 4) rather than a trick for the sake of the snapshots.
//!
//! Implementation plan: `harness/docs/plans/09-surfaces.md` (phase 4, §8)

#![deny(missing_docs)]
#![forbid(unsafe_code)]

use std::path::{Path, PathBuf};
use std::sync::Arc;

use orrery_agui::{Encoder, Frame};
use orrery_ext_api::NativeExtension;
use orrery_host::{ExtensionTable, NativeHost, NativeRegistry};
use orrery_proto::{
    AgentScope, BranchId, CallId, Consent, Event, ExtId, Grant, Layer, LoadOutcome, Outcome, Seq,
    Subject, Surface, SurfaceId, ToolRef, TurnId, Usage,
};
use orrery_surface::SurfaceStore;
use orrery_tools::Registry;
use uuid::Uuid;

/// The turn every ported scenario runs in. Constant, so a fixture is stable.
pub const TURN: TurnId =
    TurnId::from_uuid(Uuid::from_u128(0x0193_7ce5_0000_7000_8000_0000_0000_00ff));

/// One ported extension, and what to call it with.
pub struct Example {
    /// The scenario name, which is also the fixture's file stem.
    pub name: &'static str,
    /// The extension id its manifest declares.
    pub ext: &'static str,
    /// The tool to call.
    pub tool: &'static str,
    /// Where its source lives, relative to the workspace root. Read by the
    /// "no drawing code" scan.
    pub source: &'static str,
    /// Its manifest on disk, relative to the workspace root.
    pub manifest: &'static str,
    /// The code, as a native bundle.
    pub code: fn() -> Arc<dyn NativeExtension>,
    /// One call per element. More than one is a re-emission: the second call
    /// patches the surface the first created.
    pub calls: fn() -> Vec<serde_json::Value>,
}

/// The three. Different surfaces, different shapes, one vocabulary.
#[must_use]
pub fn examples() -> Vec<Example> {
    vec![
        Example {
            name: "ported-workspace-census",
            ext: "example-workspace-census",
            tool: "census",
            source: "harness/extensions/examples/workspace-census/src/lib.rs",
            manifest: "harness/extensions/examples/workspace-census/orrery.toml",
            code: || Arc::new(workspace_census::WorkspaceCensus),
            calls: || {
                vec![serde_json::json!({
                    "crates": [
                        { "name": "orrery-proto", "area": "core", "published": true },
                        { "name": "orrery-ext-api", "area": "core", "published": true },
                        { "name": "orrery-surface", "area": "core", "published": false },
                        { "name": "patch-review", "area": "extension", "published": false }
                    ]
                })]
            },
        },
        Example {
            name: "ported-patch-review",
            ext: "example-patch-review",
            tool: "review",
            source: "harness/extensions/examples/patch-review/src/lib.rs",
            manifest: "harness/extensions/examples/patch-review/orrery.toml",
            code: || Arc::new(patch_review::PatchReview),
            calls: || {
                vec![serde_json::json!({
                    "path": "harness/core/crates/orrery-ext-api/src/sink.rs",
                    "hunks": [{
                        "old_start": 28, "old_lines": 3, "new_start": 28, "new_lines": 3,
                        "lines": [
                            { "op": "context", "text": "use orrery_proto::Surface;" },
                            { "op": "del", "text": "use orrery_ext_api::SurfaceSink;" },
                            { "op": "add", "text": "use crate::ctx::SurfaceSink;" }
                        ]
                    }]
                })]
            },
        },
        Example {
            name: "ported-release-train",
            ext: "example-release-train",
            tool: "status",
            source: "harness/extensions/examples/release-train/src/lib.rs",
            manifest: "harness/extensions/examples/release-train/orrery.toml",
            code: || Arc::new(release_train::ReleaseTrain),
            calls: || vec![train(false), train(true)],
        },
    ]
}

/// The release train, before and after the ratatui client shipped. Two calls,
/// one surface: the second is a re-emission, and what crosses the wire for it
/// is a patch.
fn train(ratatui_done: bool) -> serde_json::Value {
    serde_json::json!({
        "release": "v0.25.0",
        "stages": [
            { "id": "schema", "label": "surface schema", "status": "done" },
            { "id": "differ", "label": "kernel differ", "status": "done" },
            { "id": "ratatui", "label": "ratatui client",
              "status": if ratatui_done { "done" } else { "running" } },
            { "id": "ink", "label": "ink client", "status": "pending" },
            { "id": "ade", "label": "ade client", "status": "pending" }
        ]
    })
}

/// What one ported extension produced.
pub struct Ported {
    /// The scenario name.
    pub name: String,
    /// The ledger entry a real session would show for the load.
    pub load: LoadOutcome,
    /// Every surface the extension described through `ctx.ui`, in order —
    /// children included, because composing a surface describes its parts.
    pub described: Vec<Surface>,
    /// The surface each call returned, in call order.
    pub returned: Vec<(SurfaceId, Surface)>,
    /// What the tool answered the model with.
    pub outcomes: Vec<Outcome>,
    /// The frames a client receives.
    pub frames: Vec<Frame>,
}

/// Load one example through `orrery-host`, call it, and encode what it
/// described.
///
/// # Panics
///
/// If the extension does not load, does not dispatch, or answers with
/// something other than a surface — each of which would mean the port is
/// broken, and none of which a caller could do anything about.
pub async fn run(example: &Example) -> Ported {
    let ext: ExtId = example.ext.parse().expect("the example's id parses");

    // Load: the same registry, table, manifest parser and grant as anything
    // else. `Grant::nothing()`, because a ported extension that needed a
    // capability to draw would be drawing.
    let mut natives = NativeRegistry::new();
    natives.register((example.code)());
    assert!(
        natives.broken().is_empty(),
        "{}: {:?}",
        example.name,
        natives.broken()
    );
    let host = Arc::new(NativeHost::new(natives));
    let manifest = host
        .manifest_of(&ext)
        .unwrap_or_else(|| panic!("{} declares `{ext}`", example.name));
    let table = ExtensionTable::new();
    let load = table
        .load(
            host.clone(),
            manifest,
            Layer::Project,
            Grant {
                capabilities: vec![],
                consent: Consent::Always,
            },
        )
        .await;

    // `ctx.ui` goes somewhere we can read, which is what the kernel will do
    // with it too.
    let (sink, log) = orrery_ext_api::SurfaceSink::recording();
    table.set_surface_sink(sink);

    let mut registry = Registry::with_host(table.clone());
    table.register_into(&mut registry, &ext);
    let r#ref: ToolRef = format!("{}.{}", example.ext, example.tool)
        .parse()
        .expect("the tool ref parses");

    let mut store = SurfaceStore::new();
    let mut encoder = Encoder::new("sess-ported");
    let mut seq = Seq(0);
    let mut frames = Vec::new();
    let mut returned = Vec::new();
    let mut outcomes = Vec::new();

    let push = |event: Event, frames: &mut Vec<Frame>, encoder: &mut Encoder| {
        for agui in encoder.encode(&event) {
            let next = frames.len() as u64 + 1;
            frames.push(Frame::new(next, agui));
        }
    };

    seq = seq.next();
    push(
        Event::TurnStarted { seq, turn: TURN },
        &mut frames,
        &mut encoder,
    );

    for input in (example.calls)() {
        let outcome = registry
            .dispatch(&r#ref, input, call_ctx())
            .await
            .unwrap_or_else(|e| panic!("{}: {e}", example.name));
        let Outcome::Ok {
            surface: Some(surface),
            ..
        } = &outcome
        else {
            panic!("{}: expected a surface, got {outcome:?}", example.name);
        };

        // Extension-minted, per plan 09's open question 4. A surface with no id
        // is one nothing will ever patch, so it gets one here and can only be
        // replaced.
        let id = surface.id.unwrap_or_else(SurfaceId::new);
        let patches = store
            .emit(TURN, id, surface.clone())
            .unwrap_or_else(|e| panic!("{}: {e}", example.name));
        for patch in patches {
            seq = seq.next();
            push(
                Event::Delta {
                    seq,
                    surface: id,
                    patch,
                },
                &mut frames,
                &mut encoder,
            );
        }
        returned.push((id, surface.clone()));
        outcomes.push(outcome);
    }

    seq = seq.next();
    push(
        Event::TurnSettled {
            seq,
            turn: TURN,
            usage: Usage::default(),
        },
        &mut frames,
        &mut encoder,
    );
    store.seal(TURN);

    assert!(
        store.warnings().is_empty(),
        "{}: validation nudged about something, and a ported example is the \
         last place that should be ignored: {:?}",
        example.name,
        store.warnings()
    );

    Ported {
        name: example.name.to_owned(),
        load,
        described: log.all(),
        returned,
        outcomes,
        frames,
    }
}

/// Every example, run.
///
/// # Panics
///
/// See [`run`].
pub async fn run_all() -> Vec<Ported> {
    let mut out = Vec::new();
    for example in examples() {
        out.push(run(&example).await);
    }
    out
}

/// A tool call context: the agent, a ceiling, no grant.
fn call_ctx() -> orrery_tools::CallCtx {
    orrery_tools::CallCtx::new(
        CallId::new(),
        Subject::Agent,
        AgentScope {
            agent: "main".into(),
            branch: BranchId::new(),
            tools: vec!["*".into()],
            grant: Grant::nothing(),
        },
        orrery_tools::ToolBudget::new(30_000, 1 << 20),
    )
}

/// The workspace root, from this crate.
#[must_use]
pub fn workspace_root() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR"))
        .join("../../..")
        .canonicalize()
        .expect("the workspace root exists")
}

/// Where the ported fixtures live: beside the conformance set, in a directory
/// of their own so `conformance::load_all` — which is the protocol contract and
/// counts its files — does not pick them up.
#[must_use]
pub fn fixtures_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("../conformance/ported")
}

/// One scenario as a `.jsonl` fixture: every frame, then the store state a
/// client must agree on.
///
/// The same shape `clients/conformance` uses, so both SDKs' loaders read it
/// without knowing it was generated.
#[must_use]
pub fn fixture_text(ported: &Ported, header: &str) -> String {
    let mut out = String::new();
    for line in header.lines() {
        out.push_str("# ");
        out.push_str(line);
        out.push('\n');
    }
    for frame in &ported.frames {
        out.push_str(
            &serde_json::to_string(&serde_json::json!({ "ev": frame })).expect("a frame encodes"),
        );
        out.push('\n');
    }
    let mut store = orrery_client::SurfaceStore::new();
    for frame in &ported.frames {
        store.apply(frame);
    }
    out.push_str(
        &serde_json::to_string(&serde_json::json!({ "expect": store.state() }))
            .expect("a store state encodes"),
    );
    out.push('\n');
    out
}
