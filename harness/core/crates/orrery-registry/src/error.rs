//! Everything that can go wrong between "install this" and "it is loaded".
//!
//! One enum, because every one of these ends up in the same two places: a
//! message a person reads, and a supply-chain record an auditor greps. Free
//! text would be fine for the first and useless for the second.

use std::path::PathBuf;

/// What went wrong.
#[non_exhaustive]
#[derive(Debug, thiserror::Error)]
pub enum RegistryError {
    /// The index is not TOML, or not the shape an index has.
    #[error("{file}: the index does not parse: {message}")]
    Syntax {
        /// Where it came from.
        file: String,
        /// What the parser said.
        message: String,
    },

    /// The index declares a schema this build does not understand.
    ///
    /// Refused rather than best-effort parsed: an index from the future may
    /// carry a field whose *absence* is what a check depends on.
    #[error("{file}: index schema {found} is not understood (this build reads schema {supported})")]
    UnknownSchema {
        /// Where it came from.
        file: String,
        /// What it said.
        found: u32,
        /// What this build reads.
        supported: u32,
    },

    /// The index's validity window has closed.
    #[error("{file}: the index expired at {expires} (now {now}); fetch a fresh one from {index}")]
    Expired {
        /// Where it came from.
        file: String,
        /// When it stopped being valid.
        expires: String,
        /// The moment it was checked against.
        now: String,
        /// The index URL, so the message says what to do next.
        index: String,
    },

    /// A timestamp is not an RFC 3339 instant in UTC.
    #[error("{field}: `{value}` is not an RFC 3339 UTC instant (expected YYYY-MM-DDTHH:MM:SSZ)")]
    BadTimestamp {
        /// Which field.
        field: String,
        /// What it held.
        value: String,
    },

    /// The signature does not check out against any key in force.
    #[error("{what}: the signature does not verify against any key in force ({keys})")]
    BadSignature {
        /// What was being verified: the index, or one entry.
        what: String,
        /// The key ids that were tried.
        keys: String,
    },

    /// A signature names a key the keyring does not hold, or holds only outside
    /// its validity window.
    #[error("{what}: no key `{key}` is in force at {now}")]
    UnknownKey {
        /// What was being verified.
        what: String,
        /// The key id it named.
        key: String,
        /// When it was checked.
        now: String,
    },

    /// A signature or a key is not the right number of bytes.
    #[error("{what}: malformed {field}: {message}")]
    Malformed {
        /// What was being verified.
        what: String,
        /// Which field.
        field: String,
        /// What is wrong with it.
        message: String,
    },

    /// The fetched bytes are not the bytes the index pinned.
    #[error("{id}: the package does not match the pinned hash; expected sha256 {expected}, actual sha256 {actual}")]
    HashMismatch {
        /// Which extension.
        id: String,
        /// What the index pinned.
        expected: String,
        /// What arrived.
        actual: String,
    },

    /// The fetcher could not produce the bytes at all.
    #[error("{id}: could not fetch from {origin}: {message}")]
    Fetch {
        /// Which extension.
        id: String,
        /// Where from.
        origin: String,
        /// What went wrong.
        message: String,
    },

    /// The manifest asks for capabilities the index does not mirror.
    ///
    /// A hard failure and a tampering signal: the index is what an admin
    /// reviewed, and the package is asking for more than that review covered.
    #[error("{id}: the package asks for capabilities the index does not list - this is a tampering signal, not a prompt; extra: {extra}")]
    RequiresMismatch {
        /// Which extension.
        id: String,
        /// What the manifest asked for that the index does not carry.
        extra: String,
    },

    /// A remote index was named, and this build cannot fetch one.
    ///
    /// **Not** [`RegistryError::NotInIndex`]: nothing was consulted, so nothing
    /// may be reported about what the index holds. Saying "not in the registry
    /// index <url>" for an index that was never opened reads as "I looked and
    /// it is not there", which is the one thing that did not happen.
    #[error(
        "{id}: this build cannot fetch a remote registry index, so {index} was never consulted —          fetching one is a `net` call, and a `net` call needs a session. Pass          `--index <path>` with a local copy of the index file, or set `registry.index` in the          managed layer to a path on this machine."
    )]
    RemoteIndex {
        /// What was asked for.
        id: String,
        /// The index that was named and not fetched.
        index: String,
    },

    /// The index has no such extension.
    #[error("{id}: not in the registry index {index}")]
    NotInIndex {
        /// What was asked for.
        id: String,
        /// Which index was consulted.
        index: String,
    },

    /// The index has the extension, at other versions.
    #[error("{id}@{version}: not in the registry index {index}; it has {available}")]
    VersionNotInIndex {
        /// What was asked for.
        id: String,
        /// The version asked for.
        version: String,
        /// Which index.
        index: String,
        /// What it does have.
        available: String,
    },

    /// The index has several versions and the request named none.
    #[error("{id}: the index has {available} - name one, `orrery install {id}@<version>`")]
    VersionRequired {
        /// What was asked for.
        id: String,
        /// What the index has.
        available: String,
    },

    /// A source string does not parse.
    #[error("`{input}` is not an install source: {message}")]
    SourceSyntax {
        /// What was typed.
        input: String,
        /// Why it is not a source.
        message: String,
    },

    /// A non-registry source under managed `unpinned = "refuse"`.
    #[error("{origin}: refused - {file} sets `registry.unpinned = \"refuse\"`, so only the signed index at {index} may be installed from; a user-layer setting cannot override this")]
    Unpinned {
        /// The source that was refused.
        origin: String,
        /// The managed file that said so.
        file: String,
        /// The index that is the one allowed path.
        index: String,
    },

    /// `remove` found the extension at more than one layer.
    #[error("{id}: installed at {layers} - say which, `orrery remove {id} --user` or `orrery remove {id} --workspace`")]
    AmbiguousRemove {
        /// Which extension.
        id: String,
        /// Where it was found.
        layers: String,
    },

    /// `remove` found it nowhere.
    #[error("{id}: not installed at any layer")]
    NotInstalled {
        /// Which extension.
        id: String,
    },

    /// Something is already installed there.
    #[error("{path}: already installed; remove it first")]
    AlreadyInstalled {
        /// Where.
        path: PathBuf,
    },

    /// A `--link` install could not create the symlink.
    ///
    /// Named rather than swallowed because on Windows the usual cause is a
    /// policy, not a bug: creating a symlink needs Developer Mode or the
    /// `SeCreateSymbolicLinkPrivilege`.
    #[error("{path}: could not create the development symlink: {message}; on Windows this needs Developer Mode or SeCreateSymbolicLinkPrivilege - without it, install without --link")]
    SymlinkUnavailable {
        /// Where the link would have gone.
        path: PathBuf,
        /// What the OS said.
        message: String,
    },

    /// Git said no.
    #[error("git {args}: {message}")]
    Git {
        /// What it was asked to do.
        args: String,
        /// What it said.
        message: String,
    },

    /// Plain I/O.
    #[error("{path}: {message}")]
    Io {
        /// What was being touched.
        path: PathBuf,
        /// What the OS said.
        message: String,
    },

    /// A name that is not a valid extension id.
    #[error(transparent)]
    Id(#[from] orrery_proto::IdError),
}

impl RegistryError {
    /// I/O, with the path that caused it.
    pub(crate) fn io(path: impl Into<PathBuf>, e: &std::io::Error) -> Self {
        Self::Io {
            path: path.into(),
            message: e.to_string(),
        }
    }
}
