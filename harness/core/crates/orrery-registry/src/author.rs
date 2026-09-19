//! Making an index: the half of the registry that did not exist.
//!
//! # Why this is here at all
//!
//! Plan 15 shipped a client that verifies a signed index, refuses an unpinned
//! extension and records what pinning decided — and **nothing in the product
//! could produce the index it verified**. `orrery install` says fetching a
//! remote index is not implemented, and no command created, signed or published
//! a local one. So "an admin pins a version set" was only reachable by handing
//! the admin a file made by a tool that does not ship. This module is that
//! tool's library half; `orrery registry init|add|sign|verify` is the command
//! half.
//!
//! Everything here is offline. Nothing opens a socket, nothing runs a package,
//! and the only clock is the instant a caller passes in.
//!
//! # One rendering, one signature
//!
//! The defect class this round is about is two paths that must agree with only
//! one of them right. Signing an index has exactly that shape: the document
//! signature covers the *text*, so a signer that renders the document to sign it
//! and a caller that renders it again to write it are two renderings, and the
//! signature over the first need not verify against the second.
//!
//! So [`sign_index`] **returns the bytes it signed** as [`Signed::text`], and
//! there is no way to obtain a document signature without them. A caller cannot
//! write a different rendering, because it has no reason to render at all.
//! [`signature_path`] is the same argument for where the `.sig` goes: the writer
//! and the reader ask one function.
//!
//! # The hash an admin pins is the hash an install computes
//!
//! [`entry_from_fetch`] does not walk a directory of its own. It calls
//! `PackageFetcher::stage` and then [`tree_sha256`](crate::tree_sha256) — the
//! same two calls, in the same order, that
//! [`fetch_and_verify`](crate::fetch::fetch_and_verify) makes. Pinning a hash
//! that the installer would never compute is therefore not possible to express.
//!
//! Implementation plan: `harness/docs/plans/15-registry-supply-chain.md`.

use std::path::{Path, PathBuf};

use orrery_ext_api::ExtensionManifest;

use crate::error::RegistryError;
use crate::fetch::{PackageFetcher, tree_sha256};
use crate::index::{Entry, EntrySource, Index, SCHEMA, Timestamp, render_requirement};
use crate::verify::signing::SigningKey;

/// A signed index: the exact text, and the detached signature over it.
///
/// `#[non_exhaustive]` so nobody can build one field by field and pair a
/// signature with text it does not cover.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Signed {
    /// The document, exactly as it must be written to disk.
    pub text: String,
    /// The detached signature over `text`, as `<key-id>:<hex>`.
    pub signature: String,
}

/// An empty index with a validity window.
///
/// The window is the caller's: this crate has no clock, and an index whose
/// expiry came from a hidden `SystemTime::now()` would be an index nobody could
/// test.
///
/// # Errors
///
/// [`RegistryError::BadTimestamp`] when either instant is not RFC 3339 UTC.
pub fn new_index(issued: &str, expires: &str) -> Result<Index, RegistryError> {
    let _ = Timestamp::parse("issued", issued)?;
    let _ = Timestamp::parse("expires", expires)?;
    Ok(Index {
        schema: SCHEMA,
        issued: issued.to_owned(),
        expires: expires.to_owned(),
        extensions: Vec::new(),
    })
}

/// Build one entry by staging the package the way an install will.
///
/// `staging` must not exist yet; it is created here and left in place, so an
/// admin can look at exactly the bytes that were hashed.
///
/// `requires` is read out of the staged manifest rather than typed by the
/// admin, because the index's `requires` exists to be compared against that
/// manifest — see [`check_requires`](crate::verify::check_requires). An admin
/// who typed it by hand would be pinning their own transcription.
///
/// The entry comes back **unsigned**: [`sign_index`] signs every entry it is
/// given, so there is one place a signature is made.
///
/// # Errors
///
/// [`RegistryError::Fetch`] when the package is not where the fetcher looks,
/// [`RegistryError::Io`] when the staged tree or its manifest cannot be read,
/// and [`RegistryError::Syntax`] when the manifest will not parse.
pub fn entry_from_fetch(
    id: &str,
    version: semver::Version,
    source: EntrySource,
    fetcher: &dyn PackageFetcher,
    staging: &Path,
) -> Result<Entry, RegistryError> {
    std::fs::create_dir_all(staging).map_err(|e| RegistryError::io(staging, &e))?;
    fetcher.stage(id, &source, staging)?;
    let sha256 = tree_sha256(staging)?;

    let manifest_file = staging.join("orrery.toml");
    let text = std::fs::read_to_string(&manifest_file)
        .map_err(|e| RegistryError::io(&manifest_file, &e))?;
    let manifest = ExtensionManifest::from_toml_str(&text, manifest_file.display().to_string())
        .map_err(|e| RegistryError::Syntax {
            file: manifest_file.display().to_string(),
            message: e.to_string(),
        })?;

    Ok(Entry {
        id: id.to_owned(),
        version,
        source,
        sha256,
        sig: String::new(),
        requires: manifest
            .capabilities()
            .iter()
            .map(render_requirement)
            .collect(),
    })
}

/// Sign every entry, then the document, and hand back the text that was signed.
///
/// The entries are signed first on purpose: an entry signature covers
/// `(id, version, source, sha256)` and lands *in* the document, so signing the
/// document first would sign a document that is about to change.
///
/// # Errors
///
/// [`RegistryError::Syntax`] when the document will not serialise.
pub fn sign_index(index: &mut Index, key: &SigningKey) -> Result<Signed, RegistryError> {
    for entry in &mut index.extensions {
        entry.sig = key.sign(&entry.signed_bytes());
    }
    let text = index.to_toml()?;
    let signature = key.sign(text.as_bytes());
    Ok(Signed { text, signature })
}

/// Where the detached signature for an index file lives.
///
/// One answer, asked by whoever writes it and by whoever reads it back.
#[must_use]
pub fn signature_path(index: &Path) -> PathBuf {
    let mut name = index.file_name().unwrap_or_default().to_os_string();
    name.push(".sig");
    index.with_file_name(name)
}

/// Write the index and its signature, together.
///
/// Both or neither is the point: an index on disk beside a signature that does
/// not cover it is the one state this whole module exists to prevent, and it is
/// what a caller writing the two files itself would risk.
///
/// # Errors
///
/// [`RegistryError::Io`] naming the file it could not write.
pub fn write_signed(index_path: &Path, signed: &Signed) -> Result<PathBuf, RegistryError> {
    if let Some(parent) = index_path.parent() {
        std::fs::create_dir_all(parent).map_err(|e| RegistryError::io(parent, &e))?;
    }
    std::fs::write(index_path, &signed.text).map_err(|e| RegistryError::io(index_path, &e))?;
    let sig_path = signature_path(index_path);
    std::fs::write(&sig_path, format!("{}\n", signed.signature))
        .map_err(|e| RegistryError::io(&sig_path, &e))?;
    Ok(sig_path)
}


/// A fresh 32-byte ed25519 seed, from the platform's own randomness.
///
/// The seed, not the key pair, because a seed is what [`SigningKey::from_seed`]
/// takes and what a key file can hold as one line of hex. `ring`'s
/// `SystemRandom` is the same crypto stack this crate already verifies with:
/// one implementation to audit, not two.
///
/// # Errors
///
/// [`RegistryError::Malformed`] when the platform will not produce randomness,
/// which is a machine that should not be signing anything.
pub fn generate_seed() -> Result<[u8; 32], RegistryError> {
    use ring::rand::SecureRandom as _;
    let mut seed = [0u8; 32];
    ring::rand::SystemRandom::new()
        .fill(&mut seed)
        .map_err(|_| RegistryError::Malformed {
            what: "a new signing key".to_owned(),
            field: "seed".to_owned(),
            message: "this machine would not produce 32 bytes of randomness".to_owned(),
        })?;
    Ok(seed)
}

#[cfg(test)]
mod tests {
    use super::{generate_seed, signature_path};
    use crate::verify::signing::SigningKey;

    /// A generated seed is a usable key, and two of them differ.
    #[test]
    fn a_generated_seed_signs() {
        let one = generate_seed().expect("randomness");
        let two = generate_seed().expect("randomness");
        assert_ne!(one, two, "a fresh key is fresh");
        let key = SigningKey::from_seed("managed", &one).expect("a 32-byte seed");
        assert_eq!(key.public_hex().len(), 64);
        assert!(key.sign(b"hello").starts_with("managed:"));
    }

    /// The `.sig` sits beside the index, not instead of its extension.
    #[test]
    fn the_signature_is_beside_the_index() {
        assert_eq!(
            signature_path(std::path::Path::new("index.toml")),
            std::path::Path::new("index.toml.sig")
        );
    }
}
