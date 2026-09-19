//! The signed registry client: pinning, signature checks, and refusing unpinned
//! extensions under a managed profile.
//!
//! Implementation plan: `harness/docs/plans/15-registry-supply-chain.md`
//!
//! # The registry is a pin list, not a package host
//!
//! Extensions ship on crates.io and npm. What this crate holds is an **index**:
//! names, versions, hashes and signatures, signed as a document by an
//! organisation, so that "these versions, these signatures, nothing else" is a
//! statement the harness can enforce.
//!
//! # Seven sources, one of them verified
//!
//! ```text
//! orrery install buildgraph                  the signed index — the only verified path
//! orrery install buildgraph@1.2.0            the same, pinned to a version
//! orrery install github:owner/repo#sha       a git host
//! orrery install https://…/repo.git          any git URL
//! orrery install crate:orrery-ext-buildgraph crates.io
//! orrery install npm:@scope/orrery-ext-x     npm
//! orrery install ./path [--link]             a local directory
//! ```
//!
//! **Five of those seven bypass the index.** That is a deliberate ergonomic
//! choice, and it is only safe because the boundary is explicit: every
//! non-registry install is recorded `pinned: false` in the supply-chain ledger,
//! with the source named, and managed `unpinned = "refuse"` blocks all five
//! outright. The rule, in one line: **the registry is how you trust an
//! extension; the other sources are how you try one.**
//!
//! # What the registry does not solve
//!
//! An extension's own dependency tree. We pin the extension; its crates.io
//! dependencies are pinned by its lockfile, which nobody here reviews. The wasm
//! sandbox (plan 14) closes that hole; the `native`, `node`, `python` and
//! `process` runtimes do not. Saying so is the threat model; implying otherwise
//! would be the bug.
//!
//! # Nothing executes before its signature is verified
//!
//! Fetching a crate is a download, not a `cargo install`: no build script and no
//! install hook runs before verification, and the type system is what enforces
//! it — see [`fetch`].

#![deny(missing_docs)]
#![forbid(unsafe_code)]

pub mod diff;
pub mod error;
pub mod fetch;
pub mod index;
pub mod install;
pub mod pin;
pub mod source;
pub mod verify;

pub use diff::{Approval, Decision, GrantDiff, GrantRow, RowState};
pub use error::RegistryError;
pub use fetch::{DirFetcher, NoHooks, PackageFetcher, PackageHooks, Verified, tree_sha256};
pub use index::{Entry, EntrySource, Index, SCHEMA, Timestamp};
pub use install::{InstallOptions, InstallRecord, Installer, Layout, Target, remove};
pub use pin::{
    ManagedRegistry, PinDecision, RECEIPTS_DIR, SupplyChainLedger, SupplyChainRecord, Unpinned,
    UnpinnedReason, load_refusal, receipt_beside,
};
pub use source::{GitRef, GitRunner, Source, SystemGit};
pub use verify::{Keyring, PublicKey};
