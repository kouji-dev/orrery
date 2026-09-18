//! `orrery ext` - list and test extensions.
//!
//! # No model, no network
//!
//! `ext list` and `ext test` both run against `orrery_ext_api::testing` — the
//! mock broker an extension author already writes their own tests with (plan
//! 06 task 8). Neither builds a kernel, opens a database or asks a provider for
//! anything, which is what makes `orrery ext test` runnable in a sandbox with
//! no key.
//!
//! Install and remove are **not** here: they are bare top-level verbs,
//! `orrery install <source>` and `orrery remove <name>`, in `cmd::install`.
//! What stays under `ext` is what is not an everyday action.
//!
//! Implementation plans: `harness/docs/plans/06-extension-host.md` for the
//! ledger and the test harness, `harness/docs/plans/15-registry-supply-chain.md`
//! for the registry itself.

use std::path::{Path, PathBuf};

use orrery_ext_api::ExtensionManifest;
use orrery_ext_api::testing::load_for_test;
use orrery_proto::{Capability, LoadOutcome};

use crate::args::{Cli, ExtCommand};
use crate::exit::{Exit, fail};

/// Dispatch an `ext` subcommand.
pub fn dispatch(_cli: &Cli, command: &ExtCommand) -> ! {
    match command {
        ExtCommand::List => list(),
        ExtCommand::Test { path } => test(path.as_deref()),
    }
}

/// Every first-party extension this build has, and how it loads.
fn list() -> ! {
    let mut native = orrery_host::NativeRegistry::new();
    orrery_harness::features::register_native(&mut native);
    let host = orrery_host::NativeHost::new(native);
    let manifests = host.manifests();
    if manifests.is_empty() {
        eprintln!("orrery: this build has no first-party extensions compiled in");
    }
    for manifest in manifests {
        report(&manifest);
    }
    Exit::Ok.exit()
}

/// Run an extension's manifest through the same load a session would.
fn test(path: Option<&Path>) -> ! {
    let dir = path.map_or_else(
        || std::env::current_dir().unwrap_or_else(|e| fail(Exit::Usage, e)),
        Path::to_path_buf,
    );
    let file = manifest_file(&dir);
    let src = std::fs::read_to_string(&file).unwrap_or_else(|e| {
        fail(
            Exit::Usage,
            format!("could not read {}: {e}", file.display()),
        )
    });
    let manifest = ExtensionManifest::from_toml_str(&src, file.display().to_string())
        .unwrap_or_else(|e| fail(Exit::Usage, e));

    // The grants are what the manifest itself asks for. `ext test` answers
    // "does this load, and what does it contribute" — not "what happens when
    // somebody refuses it", which is the author's own test to write.
    let grants = grant_strings(&manifest.capabilities());
    let refs: Vec<&str> = grants.iter().map(String::as_str).collect();
    let harness = load_for_test(&src, &refs).unwrap_or_else(|e| fail(Exit::Usage, e));
    let outcome = harness.load_outcome();
    report(&manifest);
    println!(
        "no model, no network: {} broker call(s) recorded",
        harness.recorded().len()
    );
    match outcome {
        LoadOutcome::Ok { .. } => Exit::Ok.exit(),
        _ => Exit::TaskFailed.exit(),
    }
}

/// `<dir>/orrery.toml`, or `<dir>` itself when it already names the file.
fn manifest_file(dir: &Path) -> PathBuf {
    if dir.is_file() {
        dir.to_path_buf()
    } else {
        dir.join("orrery.toml")
    }
}

/// One extension, as the ledger prints it: what loaded, what degraded, and why.
///
/// `missing` is the function the real host calls, so a degraded line here says
/// the same words a session would.
fn report(manifest: &ExtensionManifest) {
    let grants = manifest.capabilities();
    let problems = orrery_ext_api::testing::missing(manifest, &grants);
    let contributions = names(&manifest.contributions());
    if problems.is_empty() {
        println!(
            "{}  {}  ok  {contributions}",
            manifest.name, manifest.version
        );
        return;
    }
    println!(
        "{}  {}  degraded  {contributions}",
        manifest.name, manifest.version
    );
    for problem in problems {
        println!("    {problem}");
    }
}

fn names(contributions: &[orrery_proto::Contribution]) -> String {
    if contributions.is_empty() {
        return "nothing".to_owned();
    }
    contributions
        .iter()
        .map(|c| c.name.clone())
        .collect::<Vec<_>>()
        .join(", ")
}

/// `aspect` or `aspect:scope`, as `load_for_test` spells a grant.
fn grant_strings(capabilities: &[Capability]) -> Vec<String> {
    let mut out = Vec::new();
    for capability in capabilities {
        let aspect = serde_json::to_value(capability.aspect)
            .ok()
            .and_then(|v| v.as_str().map(str::to_owned))
            .unwrap_or_else(|| "ext".to_owned());
        if capability.scope.is_empty() {
            out.push(aspect);
        } else {
            for entry in &capability.scope {
                out.push(format!("{aspect}:{entry}"));
            }
        }
    }
    out
}
