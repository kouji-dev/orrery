//! Signatures, and the one check that turns the index into a review record.
//!
//! # Decision: ed25519, and the implementation comes from `ring`
//!
//! Plan 15 task 2 asks for a choice between `ed25519-dalek` and sigstore, and
//! recommends the former. **The recommendation stands and is taken: ed25519,
//! detached signatures, keys the organisation manages.** Sigstore's transparency
//! log assumes the verifier can reach it, and the customer this plan is written
//! for is the air-gapped one — a verification step that needs the network is a
//! verification step that fails in the room where it matters most.
//!
//! What is *amended* is where the ed25519 comes from: `ring`, not
//! `ed25519-dalek`. `rustls` already puts `ring` in this workspace, so taking
//! `ed25519-dalek` would mean two curve25519 implementations in one binary —
//! two things to audit, two things to update when one of them has an advisory,
//! and no benefit, because the algorithm and the wire format are identical. The
//! API surface used here is four calls wide, so swapping the implementation
//! later is a day's work and no format change.
//!
//! # Two levels, deliberately separate
//!
//! 1. **The document is signed** by the organisation: one detached signature
//!    over the index text, which is what makes "these versions and no others" a
//!    statement somebody made rather than a file somebody edited.
//! 2. **Each entry is signed** over `(id, version, source, sha256)`, so an entry
//!    lifted out of one index and dropped into another does not verify.
//!
//! # Keys rotate, and the window overlaps
//!
//! A keyring holds several keys, each with its own validity window. Rotation is
//! therefore an overlap rather than a cutover: the new key is added, both are in
//! force for a while, the old one falls out. `verify::rotated_key_with_overlap_works`
//! is that scenario.

use orrery_ext_api::ExtensionManifest;
use orrery_proto::Capability;

use crate::error::RegistryError;
use crate::index::{Entry, Index, Timestamp, merge_capabilities, render_requirement};

/// How many bytes an ed25519 public key is.
pub const PUBLIC_KEY_LEN: usize = 32;
/// How many bytes an ed25519 signature is.
pub const SIGNATURE_LEN: usize = 64;

/// One public key, and when it is in force.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PublicKey {
    /// How a signature names it.
    pub id: String,
    /// The raw key.
    pub bytes: Vec<u8>,
    /// Not valid before this instant.
    pub not_before: Timestamp,
    /// Not valid after this instant.
    pub not_after: Timestamp,
}

impl PublicKey {
    /// A key from hex, with its window.
    ///
    /// # Errors
    ///
    /// [`RegistryError::Malformed`] when the hex is not 32 bytes, and
    /// [`RegistryError::BadTimestamp`] for a window that is not RFC 3339.
    pub fn new(
        id: impl Into<String>,
        hex_bytes: &str,
        not_before: &str,
        not_after: &str,
    ) -> Result<Self, RegistryError> {
        let id = id.into();
        let bytes = hex::decode(hex_bytes).map_err(|e| RegistryError::Malformed {
            what: format!("key {id}"),
            field: "public key".to_owned(),
            message: e.to_string(),
        })?;
        if bytes.len() != PUBLIC_KEY_LEN {
            return Err(RegistryError::Malformed {
                what: format!("key {id}"),
                field: "public key".to_owned(),
                message: format!("{} bytes, expected {PUBLIC_KEY_LEN}", bytes.len()),
            });
        }
        Ok(Self {
            id: id.clone(),
            bytes,
            not_before: Timestamp::parse(&format!("key {id} not_before"), not_before)?,
            not_after: Timestamp::parse(&format!("key {id} not_after"), not_after)?,
        })
    }

    /// Whether this key may be used at `now`.
    #[must_use]
    pub fn in_force(&self, now: &Timestamp) -> bool {
        now >= &self.not_before && now <= &self.not_after
    }
}

/// The keys an organisation distributes through the managed layer.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Keyring {
    keys: Vec<PublicKey>,
}

impl Keyring {
    /// A keyring holding these keys.
    #[must_use]
    pub fn new(keys: Vec<PublicKey>) -> Self {
        Self { keys }
    }

    /// Add one.
    pub fn push(&mut self, key: PublicKey) {
        self.keys.push(key);
    }

    /// Every key, whether or not it is in force.
    #[must_use]
    pub fn keys(&self) -> &[PublicKey] {
        &self.keys
    }

    /// The keys usable at `now`.
    #[must_use]
    pub fn in_force(&self, now: &Timestamp) -> Vec<&PublicKey> {
        self.keys.iter().filter(|k| k.in_force(now)).collect()
    }

    fn names(&self, now: &Timestamp) -> String {
        let live = self.in_force(now);
        if live.is_empty() {
            "no key in force".to_owned()
        } else {
            live.iter()
                .map(|k| k.id.as_str())
                .collect::<Vec<_>>()
                .join(", ")
        }
    }
}

/// A signature, optionally naming the key that made it.
struct ParsedSig {
    key_id: Option<String>,
    bytes: Vec<u8>,
}

fn parse_sig(what: &str, sig: &str) -> Result<ParsedSig, RegistryError> {
    let (key_id, hex_part) = match sig.split_once(':') {
        Some((id, rest)) => (Some(id.to_owned()), rest),
        None => (None, sig),
    };
    let bytes = hex::decode(hex_part.trim()).map_err(|e| RegistryError::Malformed {
        what: what.to_owned(),
        field: "signature".to_owned(),
        message: e.to_string(),
    })?;
    if bytes.len() != SIGNATURE_LEN {
        return Err(RegistryError::Malformed {
            what: what.to_owned(),
            field: "signature".to_owned(),
            message: format!("{} bytes, expected {SIGNATURE_LEN}", bytes.len()),
        });
    }
    Ok(ParsedSig { key_id, bytes })
}

/// Check a detached signature over `message` against the keyring.
///
/// Returns the id of the key that verified it, which is what the ledger records
/// as "the rule that verified this".
///
/// # Errors
///
/// [`RegistryError::Malformed`] for a signature that is not 64 bytes,
/// [`RegistryError::UnknownKey`] when the signature names a key the ring does
/// not hold in force, and [`RegistryError::BadSignature`] when no key verifies.
pub fn verify_detached(
    what: &str,
    message: &[u8],
    sig: &str,
    keyring: &Keyring,
    now: &Timestamp,
) -> Result<String, RegistryError> {
    let parsed = parse_sig(what, sig)?;
    let candidates: Vec<&PublicKey> = match &parsed.key_id {
        Some(id) => {
            let live: Vec<&PublicKey> = keyring
                .in_force(now)
                .into_iter()
                .filter(|k| &k.id == id)
                .collect();
            if live.is_empty() {
                return Err(RegistryError::UnknownKey {
                    what: what.to_owned(),
                    key: id.clone(),
                    now: now.to_string(),
                });
            }
            live
        }
        None => keyring.in_force(now),
    };
    for key in candidates {
        let public = ring::signature::UnparsedPublicKey::new(&ring::signature::ED25519, &key.bytes);
        if public.verify(message, &parsed.bytes).is_ok() {
            return Ok(key.id.clone());
        }
    }
    Err(RegistryError::BadSignature {
        what: what.to_owned(),
        keys: keyring.names(now),
    })
}

/// Everything the index document has to satisfy before an entry is read.
///
/// Schema and timestamps are already checked by [`Index::parse`]; this adds the
/// freshness check and the document signature, in that order, because an
/// expired index that is correctly signed is still not usable and saying so is
/// the more useful message.
///
/// # Errors
///
/// [`RegistryError::Expired`], or anything [`verify_detached`] returns.
pub fn verify_index(
    index: &Index,
    text: &str,
    signature: &str,
    keyring: &Keyring,
    now: &Timestamp,
    file: &str,
    index_url: &str,
) -> Result<String, RegistryError> {
    index.check_fresh(now, file, index_url)?;
    verify_detached(
        &format!("index {file}"),
        text.as_bytes(),
        signature,
        keyring,
        now,
    )
}

/// Check one entry's own signature.
///
/// # Errors
///
/// Anything [`verify_detached`] returns.
pub fn verify_entry(
    entry: &Entry,
    keyring: &Keyring,
    now: &Timestamp,
) -> Result<String, RegistryError> {
    verify_detached(
        &format!("{}@{}", entry.id, entry.version),
        &entry.signed_bytes(),
        &entry.sig,
        keyring,
        now,
    )
}

/// The `requires` mirroring check.
///
/// The index carries what the admin reviewed. The manifest carries what the
/// package will actually ask the broker for. **The manifest asking for more is
/// a hard failure**, not a prompt: the difference is exactly the capability
/// nobody reviewed, and a prompt at this point asks a developer to approve
/// something an admin has already been told does not exist.
///
/// Asking for *less* is fine, and is not even unusual — a manifest may drop a
/// capability in a patch release while the index still lists it.
///
/// # Errors
///
/// [`RegistryError::RequiresMismatch`] naming the extra capabilities, and
/// [`RegistryError::Syntax`] when a `requires` string does not parse.
pub fn check_requires(
    entry: &Entry,
    manifest: &ExtensionManifest,
) -> Result<(), RegistryError> {
    let indexed = merge_capabilities(&entry.requires, &entry.id)?;
    let asked = manifest.capabilities();
    let mut extra: Vec<String> = Vec::new();
    for want in &asked {
        if !covered(want, &indexed) {
            extra.push(render_requirement(want));
        }
    }
    if extra.is_empty() {
        return Ok(());
    }
    Err(RegistryError::RequiresMismatch {
        id: entry.id.clone(),
        extra: extra.join(", "),
    })
}

/// Whether the index covers one asked-for capability.
///
/// Deliberately **not** glob-aware. Glob narrowing lives in `orrery-policy`,
/// and a second, subtly different implementation of "does this pattern contain
/// that one" is how a tampering check ends up with a hole in it. The rule here
/// is the strict one: the index must list the aspect, and unless the index
/// lists it unqualified, every scope string the manifest asks for must appear
/// in the index verbatim. A package that widened `read(./src/**)` to
/// `read(./**)` is a package whose review is out of date, which is the thing
/// this check exists to catch.
fn covered(want: &Capability, indexed: &[Capability]) -> bool {
    let Some(have) = indexed.iter().find(|c| c.aspect == want.aspect) else {
        return false;
    };
    if have.scope.is_empty() {
        return true;
    }
    if want.scope.is_empty() {
        // The manifest asks for the whole aspect; the index only pinned part.
        return false;
    }
    want.scope.iter().all(|s| have.scope.contains(s))
}

/// Making signatures: fixtures, and whatever eventually signs a real index.
///
/// In-tree because plan 15's tests are **offline and hand-written**: a
/// committed signature fixture cannot be regenerated when a canonical form
/// changes, so the fixtures are built from seeds at test time and the seeds are
/// what is committed. A seed is 32 bytes of nothing in particular; these are not
/// secrets and there is no secret in this repository.
pub mod signing {
    use super::{RegistryError, SIGNATURE_LEN};

    /// A key that can sign.
    pub struct SigningKey {
        id: String,
        pair: ring::signature::Ed25519KeyPair,
    }

    impl SigningKey {
        /// Derive a key from a 32-byte seed.
        ///
        /// # Errors
        ///
        /// [`RegistryError::Malformed`] when the seed is the wrong length or
        /// the curve rejects it.
        pub fn from_seed(id: impl Into<String>, seed: &[u8]) -> Result<Self, RegistryError> {
            let id = id.into();
            let pair = ring::signature::Ed25519KeyPair::from_seed_unchecked(seed).map_err(|e| {
                RegistryError::Malformed {
                    what: format!("key {id}"),
                    field: "seed".to_owned(),
                    message: e.to_string(),
                }
            })?;
            Ok(Self { id, pair })
        }

        /// The public half, as the hex an index's keyring holds.
        #[must_use]
        pub fn public_hex(&self) -> String {
            use ring::signature::KeyPair as _;
            hex::encode(self.pair.public_key().as_ref())
        }

        /// This key's id.
        #[must_use]
        pub fn id(&self) -> &str {
            &self.id
        }

        /// Sign, in the `<key-id>:<hex>` form the index writes.
        #[must_use]
        pub fn sign(&self, message: &[u8]) -> String {
            let sig = self.pair.sign(message);
            debug_assert_eq!(sig.as_ref().len(), SIGNATURE_LEN);
            format!("{}:{}", self.id, hex::encode(sig.as_ref()))
        }
    }
}
