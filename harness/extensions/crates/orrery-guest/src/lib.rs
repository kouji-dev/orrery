//! The guest SDK for Orrery wasm extensions.
//!
//! Thin on purpose. It does three things:
//!
//! 1. **Hides the arena.** You build a normal tree with [`ui`]; the SDK
//!    flattens it on the way out. Nobody writing an extension should ever type
//!    a child index.
//! 2. **Names the broker.** [`Ctx`] is the only way out of the sandbox, and
//!    every method on it can be refused — as a value, which you handle.
//! 3. **Exports the world.** [`export_extension!`] writes the dispatch.
//!
//! ```ignore
//! orrery_guest::export_extension! {
//!     "impacted" => |input: &str, ctx: &orrery_guest::Ctx| {
//!         let out = ctx.proc.run("java", &["-jar", "bg.jar"])?;
//!         Ok(orrery_guest::ui::table(
//!             &["module", "reason"],
//!             parse(&out.stdout),
//!         ))
//!     },
//! }
//! ```
//!
//! # What the sandbox costs you
//!
//! No threads, no ambient filesystem, no sockets, and a memory ceiling. If your
//! extension needs any of those, it needs the process runtime instead — which
//! has no sandbox of its own beyond what the broker withholds. That is a real
//! trade and this crate does not pretend otherwise.
//!
//! # Component size
//!
//! A Rust component with this SDK is not small. The `[profile.release]` in the
//! examples — `opt-level = "z"`, `strip = true`, `panic = "abort"` — is what
//! takes the probe guest to about 110 KiB; `wasm-opt -Oz` takes roughly another
//! fifth off. Use both before publishing.
//!
//! Implementation plan: `harness/docs/plans/14-wasm-wit.md`

#![deny(missing_docs)]
#![forbid(unsafe_code)]

/// The generated bindings for `orrery:extension@1.0.0`.
///
/// Public because [`export_extension!`] expands into code that names them, and
/// because an author who wants the raw world should not have to fight the SDK
/// for it.
pub mod bindings {
    // Generated code: the docs are the `.wit`.
    #![allow(missing_docs)]

    wit_bindgen::generate!({
        path: "../../../wit",
        world: "orrery-extension",
        pub_export_macro: true,
    });
}

pub mod ui;

use bindings::orrery::extension::broker as raw;

/// Why something did not happen.
///
/// The same four cases as the world. **Denial and budget are terminal**:
/// report them and re-plan. Retrying a denial will not help, and the SDK will
/// not do it for you.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub enum Error {
    /// A policy rule said no.
    Denied(String),
    /// A budget ceiling was reached.
    Budget(String),
    /// It was attempted and the platform refused.
    Io(String),
    /// The turn was cancelled.
    Cancelled,
}

impl Error {
    /// Whether retrying could conceivably help. False for a denial.
    #[must_use]
    pub const fn is_terminal(&self) -> bool {
        matches!(self, Error::Denied(_) | Error::Cancelled)
    }
}

impl From<raw::Error> for Error {
    fn from(e: raw::Error) -> Self {
        match e {
            raw::Error::Denied(why) => Error::Denied(why),
            raw::Error::Budget(why) => Error::Budget(why),
            raw::Error::Io(why) => Error::Io(why),
            raw::Error::Cancelled => Error::Cancelled,
        }
    }
}

impl core::fmt::Display for Error {
    fn fmt(&self, f: &mut core::fmt::Formatter<'_>) -> core::fmt::Result {
        match self {
            Error::Denied(why) => write!(f, "denied: {why}"),
            Error::Budget(why) => write!(f, "over budget: {why}"),
            Error::Io(why) => write!(f, "{why}"),
            Error::Cancelled => write!(f, "cancelled"),
        }
    }
}

impl std::error::Error for Error {}

/// What a spawned process produced.
pub use raw::ProcOut;

/// Spawning.
#[derive(Debug, Default, Clone, Copy)]
pub struct Proc;

impl Proc {
    /// Run a program, with the call's default ceilings.
    ///
    /// # Errors
    ///
    /// [`Error::Denied`] without a `spawn` grant for it, [`Error::Budget`] when
    /// the turn is out of wall clock, [`Error::Cancelled`] when the turn was
    /// cancelled.
    pub fn run(&self, cmd: &str, args: &[&str]) -> Result<ProcOut, Error> {
        self.run_with(cmd, args, 0, 1 << 20)
    }

    /// Run a program with explicit ceilings. `timeout_ms` of 0 means the call's
    /// remaining budget.
    ///
    /// # Errors
    ///
    /// As [`Self::run`].
    pub fn run_with(
        &self,
        cmd: &str,
        args: &[&str],
        timeout_ms: u64,
        max_output_bytes: u64,
    ) -> Result<ProcOut, Error> {
        let args: Vec<String> = args.iter().map(|a| (*a).to_owned()).collect();
        raw::run_proc(
            cmd,
            &args,
            raw::RunOpts {
                timeout_ms,
                max_output_bytes,
            },
        )
        .map_err(Into::into)
    }
}

/// Files — through the broker, because there is no other way.
#[derive(Debug, Default, Clone, Copy)]
pub struct Fs;

impl Fs {
    /// Read a file, truncated at `max_bytes`.
    ///
    /// Returns the bytes and whether there were more. There is deliberately no
    /// "read it all" here: an unbounded read is how a tool blows a budget.
    ///
    /// # Errors
    ///
    /// [`Error::Denied`] without a `read` grant covering the path.
    pub fn read(&self, path: &str, max_bytes: u64) -> Result<(Vec<u8>, bool), Error> {
        raw::read_file(path, max_bytes)
            .map(|out| (out.bytes, out.truncated))
            .map_err(Into::into)
    }

    /// Write a file all-or-nothing: a temp file and a rename, so a cancelled
    /// write leaves the original intact.
    ///
    /// # Errors
    ///
    /// [`Error::Denied`] without a `write` grant covering the path.
    pub fn write_atomic(&self, path: &str, bytes: &[u8]) -> Result<(), Error> {
        raw::write_file(path, bytes, true).map_err(Into::into)
    }

    /// Write a file in place. Prefer [`Self::write_atomic`] unless you know why
    /// you do not want it.
    ///
    /// # Errors
    ///
    /// As [`Self::write_atomic`].
    pub fn write(&self, path: &str, bytes: &[u8]) -> Result<(), Error> {
        raw::write_file(path, bytes, false).map_err(Into::into)
    }
}

/// Outbound requests.
#[derive(Debug, Default, Clone, Copy)]
pub struct Net;

impl Net {
    /// GET a url, bounded.
    ///
    /// # Errors
    ///
    /// [`Error::Denied`] without a `net` grant for the host.
    pub fn get(&self, url: &str, max_response_bytes: u64) -> Result<raw::FetchOut, Error> {
        raw::fetch(
            url,
            &raw::FetchOpts {
                method: "GET".to_owned(),
                headers: Vec::new(),
                body: None,
                timeout_ms: 0,
                max_response_bytes,
            },
        )
        .map_err(Into::into)
    }

    /// Ask the host to attach a stored credential to the next request to
    /// `host`. **The secret never enters this component's memory** — you get
    /// back only whether it was permitted.
    ///
    /// # Errors
    ///
    /// [`Error::Denied`] without a `cred` grant for that name.
    pub fn use_credential(&self, name: &str, host: &str, purpose: &str) -> Result<(), Error> {
        raw::use_credential(
            name,
            &raw::CredUsage {
                host: host.to_owned(),
                purpose: purpose.to_owned(),
            },
        )
        .map_err(Into::into)
    }
}

/// Everything one tool call can reach.
///
/// Note what is **not** on it: a capability token. There is nowhere to put one
/// — the host looks the call's authority up on its own side. You ask; it
/// decides.
#[derive(Debug, Default, Clone, Copy)]
pub struct Ctx {
    /// Spawning.
    pub proc: Proc,
    /// Files.
    pub fs: Fs,
    /// The network.
    pub net: Net,
}

impl Ctx {
    /// The context for one call.
    #[must_use]
    pub const fn new() -> Self {
        Self {
            proc: Proc,
            fs: Fs,
            net: Net,
        }
    }
}

/// Export an extension: one arm per tool.
///
/// Each arm is `"name" => expr`, where `expr` is anything callable as
/// `fn(&str, &Ctx) -> Result<ui::Node, String>`. The `&str` is the call's JSON
/// input; the `String` is the failure message the model sees.
///
/// Everything else — the `Guest` impl, the `export!`, and flattening your tree
/// into the arena — is written for you.
#[macro_export]
macro_rules! export_extension {
    ($($name:literal => $handler:expr),+ $(,)?) => {
        #[doc(hidden)]
        struct __OrreryExtension;

        impl $crate::bindings::exports::orrery::extension::tools::Guest for __OrreryExtension {
            fn call(
                name: ::std::string::String,
                input: ::std::string::String,
            ) -> ::std::result::Result<
                $crate::bindings::orrery::extension::surfaces::Surface,
                ::std::string::String,
            > {
                let ctx = $crate::Ctx::new();
                match name.as_str() {
                    $(
                        $name => {
                            let handler = $handler;
                            let node: $crate::ui::Node = handler(input.as_str(), &ctx)?;
                            // The author never sees an index: this is the only
                            // place the arena is built.
                            ::std::result::Result::Ok($crate::ui::flatten(&node))
                        }
                    )+
                    other => ::std::result::Result::Err(
                        ::std::format!("no such tool: {other}")
                    ),
                }
            }
        }

        $crate::bindings::export!(__OrreryExtension with_types_in $crate::bindings);
    };
}
