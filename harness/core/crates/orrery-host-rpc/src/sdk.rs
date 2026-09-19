//! `@orrery/ext`, carried in the binary and vendored beside a node extension.
//!
//! # The gap this closes
//!
//! `orrery install ./node-hello --to user` succeeded, `orrery ext list` said
//! `ok`, and the first turn that called `hello.greet` answered:
//!
//! ```text
//! Failed { stage: Activate, message: "ERR_MODULE_NOT_FOUND: Cannot find module
//!   '<home>/.orrery/node/ext-sdk/src/index.mjs'" }
//! ```
//!
//! The example imported the SDK by a relative path that resolves only inside
//! this repository, and **nothing installed the SDK beside the extension**.
//! There is no third place it could have come from: this build makes no network
//! request, so `npm install` is not an option, and the harness ships as one
//! executable, so there is no `lib/` directory next to it either.
//!
//! So the SDK travels in the binary. [`vendor`] writes it to
//! `<ext root>/node_modules/@orrery/ext/`, which is where node's ESM resolver
//! looks for a bare specifier — the one mechanism that works for both a copy
//! under `~/.orrery/extensions/` and the worked example in this tree, with no
//! rewriting of anybody's source.
//!
//! # Why the host does it rather than the installer
//!
//! [`RpcHost::install`](crate::RpcHost::install) is told where an extension's
//! files are on every path that can run one: the session builder, `orrery ext
//! test`, and this crate's own tests. Vendoring there means an extension that
//! arrived by `orrery install`, by `--link`, or by being checked into a
//! repository all resolve the SDK the same way. An installer-only fix would
//! have left `--link` and the in-tree example broken.
//!
//! # It is not a sandbox
//!
//! Same warning the SDK's own README carries: the loader hook that hides `fs`
//! from a guest is a convenience. A node extension runs with this process's
//! privileges; wasm is the runtime with a real boundary.

use std::io;
use std::path::Path;

/// The published name, and the directory it is vendored under.
pub const PACKAGE: &str = "@orrery/ext";

/// Every file of the package, as `(path under the package root, contents)`.
///
/// `include_str!` rather than a build script: the SDK is six small modules, it
/// lives in this repository, and a missing file becomes a compile error rather
/// than a runtime one.
pub const FILES: &[(&str, &str)] = &[
    (
        "package.json",
        include_str!("../../../../extensions/node/ext-sdk/package.json"),
    ),
    (
        "src/index.mjs",
        include_str!("../../../../extensions/node/ext-sdk/src/index.mjs"),
    ),
    (
        "src/ctx.mjs",
        include_str!("../../../../extensions/node/ext-sdk/src/ctx.mjs"),
    ),
    (
        "src/framing.mjs",
        include_str!("../../../../extensions/node/ext-sdk/src/framing.mjs"),
    ),
    (
        "src/loader.mjs",
        include_str!("../../../../extensions/node/ext-sdk/src/loader.mjs"),
    ),
    (
        "src/rpc.mjs",
        include_str!("../../../../extensions/node/ext-sdk/src/rpc.mjs"),
    ),
    (
        "src/strip.mjs",
        include_str!("../../../../extensions/node/ext-sdk/src/strip.mjs"),
    ),
];

/// Where the package lands under an extension's root.
#[must_use]
pub fn package_dir(root: &Path) -> std::path::PathBuf {
    root.join("node_modules").join(PACKAGE)
}

/// Write `@orrery/ext` beside the extension at `root`, if it is not already the
/// same bytes.
///
/// Idempotent, and cheap on the common path: it compares each file and writes
/// only what differs, so loading an extension twice does not rewrite anything
/// and a directory somebody keeps open is not churned.
///
/// # Errors
///
/// Whatever the filesystem said. A read-only extension directory is a real
/// possibility and is **not** fatal to a load — the caller logs it and lets the
/// guest fail with node's own message, which names the specifier it could not
/// resolve.
pub fn vendor(root: &Path) -> io::Result<()> {
    let dir = package_dir(root);
    for (name, contents) in FILES {
        let path = dir.join(name);
        if std::fs::read_to_string(&path).is_ok_and(|on_disk| on_disk == *contents) {
            continue;
        }
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&path, contents)?;
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The package is complete: every relative import the SDK makes of itself
    /// resolves to a file that travels with it.
    #[test]
    fn every_relative_import_is_carried() {
        for (name, contents) in FILES {
            for line in contents.lines() {
                let Some(rest) = line.split_once("from \"./") else {
                    continue;
                };
                let Some((target, _)) = rest.1.split_once('"') else {
                    continue;
                };
                let carried = FILES
                    .iter()
                    .any(|(other, _)| other.ends_with(&format!("/{target}")));
                assert!(carried, "`{name}` imports `./{target}`, which is not carried");
            }
        }
    }

    /// Vendoring twice writes once.
    #[test]
    fn vendoring_is_idempotent() {
        let dir = tempfile::tempdir().expect("a temporary extension root");
        vendor(dir.path()).expect("the first vendor");
        let entry = package_dir(dir.path()).join("src/index.mjs");
        let first = std::fs::metadata(&entry)
            .and_then(|m| m.modified())
            .expect("a timestamp");
        vendor(dir.path()).expect("the second vendor");
        let second = std::fs::metadata(&entry)
            .and_then(|m| m.modified())
            .expect("a timestamp");
        assert_eq!(first, second, "the second vendor rewrote an identical file");
    }
}
