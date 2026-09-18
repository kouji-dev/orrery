//! Fetching, hashing, and the order that makes the hash worth anything.
//!
//! # The order, and why it is a type rather than a comment
//!
//! ```text
//! resolve pin → fetch → verify sha256 → verify signature → parse manifest
//!   → compare manifest.requires vs index.requires → grant diff → load
//! ```
//!
//! [`fetch_and_verify`] is the only way to get a [`Verified`], and everything
//! downstream — unpacking into a layer, running an install hook, loading — takes
//! a `Verified`. So "nothing executes before its signature is verified" is not a
//! rule somebody has to remember: there is no value to execute *from* until the
//! verification has happened.
//!
//! A fetched package lands in a **quarantine** directory that is on no
//! discovery path, and is moved into a layer only after it verifies.
//! `fetch::nothing_executes_before_verification` is the sentinel test.
//!
//! # Hashing a tree, not a tarball
//!
//! A package arrives as a directory — that is what a git clone, a local path and
//! an unpacked crate all are. So the pin is over a **canonical tree hash**:
//! every file's relative path and bytes, in sorted order, length-prefixed so
//! that no rename can be absorbed by a neighbouring file's contents. One hash
//! for every runtime, which is open question 2's "lean duplicate" answered in
//! the only way that keeps a single verification path.

use std::path::{Path, PathBuf};

use sha2::{Digest, Sha256};

use crate::error::RegistryError;
use crate::index::{Entry, EntrySource, Timestamp};
use crate::verify::{Keyring, verify_entry};

/// How package bytes are produced.
///
/// Every implementation **stages into a directory it is handed** and returns.
/// It does not get to say where, and it does not get to run anything: a fetcher
/// that needed to execute the package to produce it would be a fetcher that has
/// already lost the argument this module exists to win.
pub trait PackageFetcher: Send + Sync {
    /// Put the package's files under `into`, which already exists and is empty.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Fetch`] with whatever went wrong, named by source.
    fn stage(&self, id: &str, source: &EntrySource, into: &Path) -> Result<(), RegistryError>;
}

/// A fetcher that copies from a directory on this machine.
///
/// What every test in this crate uses, and what a mirrored, air-gapped install
/// would use for real: `<root>/<name>/` is the package. Plan 15 says the tests
/// run offline, and an offline test needs a fetcher that is not a mock of the
/// real thing but a real thing pointed somewhere local.
#[derive(Clone, Debug)]
pub struct DirFetcher {
    root: PathBuf,
}

impl DirFetcher {
    /// Serve packages from under `root`.
    #[must_use]
    pub fn new(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    fn package_dir(&self, source: &EntrySource) -> PathBuf {
        let leaf = match source {
            EntrySource::CratesIo { name } | EntrySource::Npm { name } => name.clone(),
            EntrySource::Url { url } => url
                .trim_end_matches('/')
                .rsplit('/')
                .next()
                .unwrap_or("package")
                .to_owned(),
        };
        self.root.join(leaf)
    }
}

impl PackageFetcher for DirFetcher {
    fn stage(&self, id: &str, source: &EntrySource, into: &Path) -> Result<(), RegistryError> {
        let from = self.package_dir(source);
        if !from.is_dir() {
            return Err(RegistryError::Fetch {
                id: id.to_owned(),
                origin: source.to_string(),
                message: format!("{} is not a directory", from.display()),
            });
        }
        copy_tree(&from, into)
    }
}

/// Something a package could ask to have run once it is installed.
///
/// It exists so that "nothing runs before verification" is provable rather than
/// merely true today: [`fetch_and_verify`] never touches it, and the installer
/// calls it only with a [`Verified`] in hand. A test asserts both halves — the
/// hook does not fire on a failed verification, and it *does* fire on a
/// successful one, so the first assertion is not vacuous.
pub trait PackageHooks: Send + Sync {
    /// Run whatever the package wanted run, now that it is verified and placed.
    ///
    /// # Errors
    ///
    /// A message for the ledger. A failing hook degrades an install; it does
    /// not un-verify it.
    fn post_install(&self, id: &str, dir: &Path) -> Result<(), String>;
}

/// The default: packages do not get to run anything.
///
/// A crate is a download, not a `cargo install`; a node package is an unpack,
/// not an `npm install` with its lifecycle scripts. Anything more is a decision
/// somebody has to make on purpose by passing a different implementation.
#[derive(Copy, Clone, Debug, Default)]
pub struct NoHooks;

impl PackageHooks for NoHooks {
    fn post_install(&self, _id: &str, _dir: &Path) -> Result<(), String> {
        Ok(())
    }
}

/// A package that has passed every check the index can make.
///
/// Only [`fetch_and_verify`] constructs one — the fields are public but the
/// struct is `#[non_exhaustive]`, so no other crate can conjure one up and skip
/// the checks.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct Verified {
    /// The quarantine directory holding the package's files.
    pub dir: PathBuf,
    /// The hash that matched.
    pub sha256: String,
    /// The key that verified the entry, for the ledger.
    pub key: String,
    /// The entry it was verified against.
    pub entry: Entry,
}

/// Fetch a pinned entry and check it, in the order the plan fixes.
///
/// `quarantine` must not exist yet: it is created here, and it is the caller's
/// to clean up. On any failure the directory is left in place on purpose, so a
/// person investigating a hash mismatch still has the bytes that did not match.
///
/// # Errors
///
/// [`RegistryError::HashMismatch`] with both hashes, or anything
/// [`verify_entry`] returns.
pub fn fetch_and_verify(
    entry: &Entry,
    fetcher: &dyn PackageFetcher,
    keyring: &Keyring,
    now: &Timestamp,
    quarantine: &Path,
) -> Result<Verified, RegistryError> {
    std::fs::create_dir_all(quarantine).map_err(|e| RegistryError::io(quarantine, &e))?;
    fetcher.stage(&entry.id, &entry.source, quarantine)?;

    let actual = tree_sha256(quarantine)?;
    if actual != entry.sha256.to_ascii_lowercase() {
        return Err(RegistryError::HashMismatch {
            id: entry.id.clone(),
            expected: entry.sha256.clone(),
            actual,
        });
    }

    let key = verify_entry(entry, keyring, now)?;

    Ok(Verified {
        dir: quarantine.to_path_buf(),
        sha256: actual,
        key,
        entry: entry.clone(),
    })
}

/// The canonical hash of a directory tree.
///
/// Every file, sorted by its forward-slashed relative path, contributing
/// `len(path) path len(bytes) bytes`. The lengths are what stop a file called
/// `ab` next to `c` hashing the same as a file called `a` next to `bc`.
/// Directories contribute nothing by themselves, because an empty directory is
/// not something any packaging format preserves reliably.
///
/// # Errors
///
/// [`RegistryError::Io`] naming the file it could not read.
pub fn tree_sha256(dir: &Path) -> Result<String, RegistryError> {
    let mut files = Vec::new();
    collect(dir, dir, &mut files)?;
    files.sort();
    let mut hasher = Sha256::new();
    hasher.update(b"orrery-package-tree/1\n");
    for rel in files {
        let path = dir.join(rel.replace('/', std::path::MAIN_SEPARATOR_STR));
        let bytes = std::fs::read(&path).map_err(|e| RegistryError::io(&path, &e))?;
        hasher.update((rel.len() as u64).to_le_bytes());
        hasher.update(rel.as_bytes());
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(&bytes);
    }
    Ok(hex::encode(hasher.finalize()))
}

fn collect(root: &Path, dir: &Path, out: &mut Vec<String>) -> Result<(), RegistryError> {
    let entries = std::fs::read_dir(dir).map_err(|e| RegistryError::io(dir, &e))?;
    for entry in entries {
        let entry = entry.map_err(|e| RegistryError::io(dir, &e))?;
        let path = entry.path();
        let kind = entry.file_type().map_err(|e| RegistryError::io(&path, &e))?;
        if kind.is_dir() {
            // `.git` is clone bookkeeping, not package content: two clones of
            // one commit differ inside it, and hashing it would make a git
            // install unpinnable even when the commit is exact.
            if path.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            collect(root, &path, out)?;
        } else {
            let rel = path
                .strip_prefix(root)
                .map_err(|_| RegistryError::Io {
                    path: path.clone(),
                    message: "is not under the tree being hashed".to_owned(),
                })?
                .to_string_lossy()
                .replace('\\', "/");
            out.push(rel);
        }
    }
    Ok(())
}

/// Copy a directory tree, creating `to`.
///
/// # Errors
///
/// [`RegistryError::Io`] naming the file it could not copy.
pub fn copy_tree(from: &Path, to: &Path) -> Result<(), RegistryError> {
    std::fs::create_dir_all(to).map_err(|e| RegistryError::io(to, &e))?;
    let entries = std::fs::read_dir(from).map_err(|e| RegistryError::io(from, &e))?;
    for entry in entries {
        let entry = entry.map_err(|e| RegistryError::io(from, &e))?;
        let src = entry.path();
        let dst = to.join(entry.file_name());
        let kind = entry.file_type().map_err(|e| RegistryError::io(&src, &e))?;
        if kind.is_dir() {
            if src.file_name().is_some_and(|n| n == ".git") {
                continue;
            }
            copy_tree(&src, &dst)?;
        } else {
            std::fs::copy(&src, &dst).map_err(|e| RegistryError::io(&src, &e))?;
        }
    }
    Ok(())
}
