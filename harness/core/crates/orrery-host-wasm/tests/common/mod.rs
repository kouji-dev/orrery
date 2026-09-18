//! The guest side of every test here, and the brokers the host answers with.
//!
//! The probe component is built once per test binary, for `wasm32-wasip2`, from
//! `tests/guests/sandbox-probe`. That crate is deliberately outside the
//! workspace: it is another target, another profile, and it must be buildable
//! with `wit-bindgen` alone, because proving the world is usable without an
//! SDK of ours is half the point.

#![allow(dead_code)]

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};

use async_trait::async_trait;
use orrery_host_wasm::broker::{CredUsage, FetchOpts, FetchOut, FileOut, ProcOut, RunOpts};
use orrery_host_wasm::{Failure, HostBroker};

/// Build the probe guest and return its bytes.
///
/// Cached for the life of the test binary: the build is the slow part, and
/// every test wants the same component.
pub fn probe() -> &'static [u8] {
    static BYTES: OnceLock<Vec<u8>> = OnceLock::new();
    BYTES.get_or_init(|| {
        let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("tests/guests/sandbox-probe");
        let status = std::process::Command::new(env!("CARGO"))
            .args(["build", "--release", "--target", "wasm32-wasip2"])
            .current_dir(&dir)
            .status()
            .expect("cargo runs");
        assert!(
            status.success(),
            "the probe guest did not build. If the target is missing: \
             `rustup target add wasm32-wasip2`. This test cannot be skipped — \
             it is what proves the sandbox."
        );
        let wasm = dir.join("target/wasm32-wasip2/release/sandbox_probe.wasm");
        std::fs::read(&wasm).unwrap_or_else(|e| panic!("reading {}: {e}", wasm.display()))
    })
}

/// A broker that answers whatever the test wants, and records what it was asked.
#[derive(Default)]
pub struct Fake {
    /// What `run_proc` answers.
    pub proc: Option<Result<ProcOut, Failure>>,
    /// The bytes `read_file` has, before truncation.
    pub file: Option<Vec<u8>>,
    /// What every call is refused with when nothing above answers.
    pub refusal: Option<Failure>,
    /// What was asked, in order.
    pub seen: Mutex<Vec<String>>,
    /// What `write_file` was handed, by path.
    pub written: Mutex<Vec<(String, Vec<u8>)>>,
    /// Blocks `run_proc` until this is notified, then answers `cancelled`.
    pub block_until_cancelled: Option<Arc<tokio::sync::Notify>>,
}

impl Fake {
    /// A broker that refuses everything, like a tool granted nothing.
    pub fn denying() -> Self {
        Self {
            refusal: Some(Failure::Denied("no `spawn` grant for this tool".to_owned())),
            ..Self::default()
        }
    }

    /// A broker holding one file.
    pub fn holding(bytes: &[u8]) -> Self {
        Self {
            file: Some(bytes.to_vec()),
            ..Self::default()
        }
    }

    fn note(&self, what: impl Into<String>) {
        self.seen.lock().expect("no panics in tests").push(what.into());
    }

    /// Everything the guest asked for.
    pub fn calls(&self) -> Vec<String> {
        self.seen.lock().expect("no panics in tests").clone()
    }

    /// What the guest wrote, by path.
    pub fn writes(&self) -> Vec<(String, Vec<u8>)> {
        self.written.lock().expect("no panics in tests").clone()
    }

    fn refuse<T>(&self) -> Result<T, Failure> {
        Err(self
            .refusal
            .clone()
            .unwrap_or_else(|| Failure::Denied("nothing was granted".to_owned())))
    }
}

#[async_trait]
impl HostBroker for Fake {
    async fn run_proc(
        &self,
        cmd: &str,
        args: &[String],
        _: RunOpts,
    ) -> Result<ProcOut, Failure> {
        self.note(format!("run-proc {cmd} {}", args.join(" ")));
        if let Some(gate) = &self.block_until_cancelled {
            // What a guest blocked inside an import looks like. The epoch does
            // not free this; the broker does, by returning.
            gate.notified().await;
            return Err(Failure::Cancelled);
        }
        match &self.proc {
            Some(Ok(out)) => Ok(out.clone()),
            Some(Err(e)) => Err(e.clone()),
            None => self.refuse(),
        }
    }

    async fn read_file(&self, path: &str, max_bytes: u64) -> Result<FileOut, Failure> {
        self.note(format!("read-file {path} max={max_bytes}"));
        match &self.file {
            Some(all) => {
                let max = usize::try_from(max_bytes).unwrap_or(usize::MAX);
                let truncated = all.len() > max;
                Ok(FileOut {
                    bytes: all[..all.len().min(max)].to_vec(),
                    truncated,
                })
            }
            None => self.refuse(),
        }
    }

    async fn write_file(&self, path: &str, bytes: &[u8], _: bool) -> Result<(), Failure> {
        self.note(format!("write-file {path}"));
        self.written
            .lock()
            .expect("no panics in tests")
            .push((path.to_owned(), bytes.to_vec()));
        Ok(())
    }

    async fn fetch(&self, url: &str, _: FetchOpts) -> Result<FetchOut, Failure> {
        self.note(format!("fetch {url}"));
        self.refuse()
    }

    async fn use_credential(&self, name: &str, usage: CredUsage) -> Result<(), Failure> {
        self.note(format!("use-credential {name} -> {}", usage.host));
        self.refuse()
    }
}
