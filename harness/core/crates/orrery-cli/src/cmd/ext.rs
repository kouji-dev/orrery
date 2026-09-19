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
//! # A name is a way in, not only a path
//!
//! `ext list` reports three things at once: what this build compiled in, what
//! the layers in force have **installed**, and how each one loads. Reporting
//! only the compiled-in set is how somebody installs an extension, runs the one
//! command named after listing them, and does not see it.
//!
//! `ext test` therefore takes a name as well as a path. A compiled-in bundle has
//! no `orrery.toml` anywhere on disk - its manifest is a value its Rust returns
//! - so `orrery ext test builtin` cannot be a path, and used to fail with
//!   `could not read builtin/orrery.toml`.
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
use crate::cmd::layers;
use crate::exit::{Exit, fail};

/// Dispatch an `ext` subcommand.
pub fn dispatch(cli: &Cli, command: &ExtCommand) -> ! {
    match command {
        ExtCommand::List => list(cli),
        ExtCommand::Test { path } => test(cli, path.as_deref()),
    }
}

/// Every extension this build can see: compiled in, then installed.
fn list(cli: &Cli) -> ! {
    let compiled = compiled_in();
    for manifest in &compiled {
        report(manifest, "builtin");
    }

    let installed = installed(cli);
    for found in &installed {
        report(&found.manifest, &found.layer);
    }

    if compiled.is_empty() && installed.is_empty() {
        eprintln!("orrery: this build has no extensions compiled in, and none is installed");
    }
    Exit::Ok.exit()
}

/// Run an extension's manifest through the same load a session would.
fn test(cli: &Cli, target: Option<&Path>) -> ! {
    let (manifest, src) = resolve(cli, target);

    // The grants are what the manifest itself asks for. `ext test` answers
    // "does this load, and what does it contribute" - not "what happens when
    // somebody refuses it", which is the author's own test to write.
    let grants = grant_strings(&manifest.capabilities());
    let refs: Vec<&str> = grants.iter().map(String::as_str).collect();
    let harness = match src {
        Some(src) => load_for_test(&src, &refs).unwrap_or_else(|e| fail(Exit::Usage, e)),
        None => orrery_ext_api::testing::load_parsed_for_test(manifest.clone(), &refs),
    };
    let outcome = harness.load_outcome();
    report(&manifest, "");
    println!(
        "no model, no network: {} broker call(s) recorded",
        harness.recorded().len()
    );
    match outcome {
        LoadOutcome::Ok { .. } => Exit::Ok.exit(),
        _ => Exit::TaskFailed.exit(),
    }
}

/// What `ext test` was pointed at: a directory, a manifest file, or a name.
///
/// The TOML source comes back with it when there was one, so a manifest that is
/// on disk still goes through the same parse a real load does. A compiled-in
/// bundle has no source, and says so with `None`.
fn resolve(cli: &Cli, target: Option<&Path>) -> (ExtensionManifest, Option<String>) {
    let Some(target) = target else {
        return from_dir(&std::env::current_dir().unwrap_or_else(|e| fail(Exit::Usage, e)));
    };
    if target.exists() {
        return from_dir(target);
    }

    // Not a path, so it is a name. Compiled-in first: that set is fixed at build
    // time and cannot be shadowed by something on disk.
    let name = target.to_string_lossy();
    if let Some(manifest) = compiled_in().into_iter().find(|m| m.name.as_str() == name) {
        return (manifest, None);
    }
    if let Some(found) = installed(cli)
        .into_iter()
        .find(|found| found.manifest.name.as_str() == name)
    {
        let src = std::fs::read_to_string(&found.file).ok();
        return (found.manifest, src);
    }

    fail(
        Exit::Usage,
        format!(
            "no extension named `{name}`: it is not a path, it is not compiled in ({}), \
             and no layer in force has it installed",
            compiled_in()
                .iter()
                .map(|m| m.name.to_string())
                .collect::<Vec<_>>()
                .join(", ")
        ),
    )
}

/// A manifest read from a directory or a file.
fn from_dir(dir: &Path) -> (ExtensionManifest, Option<String>) {
    let file = manifest_file(dir);
    let src = std::fs::read_to_string(&file).unwrap_or_else(|e| {
        fail(
            Exit::Usage,
            format!("could not read {}: {e}", file.display()),
        )
    });
    let manifest = ExtensionManifest::from_toml_str(&src, file.display().to_string())
        .unwrap_or_else(|e| fail(Exit::Usage, e));
    (manifest, Some(src))
}

/// The first-party bundles this build compiled in.
fn compiled_in() -> Vec<ExtensionManifest> {
    let mut native = orrery_host::NativeRegistry::new();
    orrery_harness::features::register_native(&mut native);
    orrery_host::NativeHost::new(native)
        .manifests()
        .into_iter()
        .map(|m| (*m).clone())
        .collect()
}

/// Every extension the layers in force have installed, with the layer that
/// contributed it.
///
/// This is plan 10's discovery pass, not a second walk of its own: what
/// `ext list` prints is what a session would load, or the command is a lie.
/// A manifest that will not parse is reported as a problem rather than aborting
/// the listing - one broken extension should not hide the other nine.
fn installed(cli: &Cli) -> Vec<Installed> {
    let resolved = layers::resolve(cli);
    let mut out = Vec::new();
    for found in &resolved.manifest.extensions {
        let layer = format!("{:?}", found.layer).to_lowercase();
        let file = if found.source.is_dir() {
            found.source.join("orrery.toml")
        } else {
            found.source.clone()
        };
        let Ok(src) = std::fs::read_to_string(&file) else {
            println!("{}  ?  unreadable  {layer}  {}", found.name, file.display());
            continue;
        };
        match ExtensionManifest::from_toml_str(&src, file.display().to_string()) {
            Ok(manifest) => out.push(Installed {
                manifest,
                layer,
                file,
            }),
            Err(e) => println!("{}  ?  unloadable  {layer}  {e}", found.name),
        }
    }
    out
}

/// One installed extension: its manifest, the layer that contributed it, and
/// the file it was read from.
struct Installed {
    manifest: ExtensionManifest,
    layer: String,
    file: PathBuf,
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
fn report(manifest: &ExtensionManifest, origin: &str) {
    let grants = manifest.capabilities();
    let problems = orrery_ext_api::testing::missing(manifest, &grants);
    let contributions = names(&manifest.contributions());
    let origin = if origin.is_empty() {
        String::new()
    } else {
        format!("  {origin}")
    };
    if problems.is_empty() {
        println!(
            "{}  {}  ok{origin}  {contributions}",
            manifest.name, manifest.version
        );
        return;
    }
    println!(
        "{}  {}  degraded{origin}  {contributions}",
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
