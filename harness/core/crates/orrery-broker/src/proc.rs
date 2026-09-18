//! Creating a process, where the grant is actually enforced.
//!
//! A `spawn` rule decides whether to ask. **This** is where what can run is
//! decided: the command is started by the broker, inside a containment, under a
//! wall-clock watchdog, with its output bounded while it is produced.

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::process::Stdio;
use std::time::Duration;

use crate::contain::{Containment, Guard, MemoryEnforcement, contain};
use crate::error::BrokerError;
use crate::limit::LimitedReader;

/// What to run.
#[derive(Clone, Debug, Default)]
pub struct SpawnSpec {
    /// The program.
    pub program: String,
    /// Its arguments, already split: the broker never hands a string to a shell.
    pub args: Vec<String>,
    /// Where to run it.
    pub cwd: Option<PathBuf>,
    /// The environment, which is the whole environment and not an overlay.
    pub env: BTreeMap<String, String>,
}

impl SpawnSpec {
    /// A program with no arguments.
    #[must_use]
    pub fn new(program: impl Into<String>) -> Self {
        Self {
            program: program.into(),
            ..Self::default()
        }
    }

    /// Add an argument.
    #[must_use]
    pub fn arg(mut self, arg: impl Into<String>) -> Self {
        self.args.push(arg.into());
        self
    }

    /// Add several.
    #[must_use]
    pub fn args<I: IntoIterator<Item = S>, S: Into<String>>(mut self, args: I) -> Self {
        self.args.extend(args.into_iter().map(Into::into));
        self
    }

    /// Run somewhere in particular.
    #[must_use]
    pub fn cwd(mut self, dir: impl Into<PathBuf>) -> Self {
        self.cwd = Some(dir.into());
        self
    }

    /// The command text a rule would have matched, for the audit and the ledger.
    #[must_use]
    pub fn command_text(&self) -> String {
        std::iter::once(self.program.clone())
            .chain(self.args.iter().cloned())
            .collect::<Vec<_>>()
            .join(" ")
    }
}

/// How the grace window between "stop" and "die" is spent.
pub const GRACE: Duration = Duration::from_millis(500);

/// A running process, and the grip on its tree.
#[derive(Debug)]
pub struct Child {
    inner: tokio::process::Child,
    guard: Guard,
    stdout: Option<tokio::process::ChildStdout>,
    stderr: Option<tokio::process::ChildStderr>,
    output_ceiling: u64,
    wall_clock: Duration,
}

/// What a finished child left behind.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Output {
    /// Its exit code, when it had one.
    pub code: Option<i32>,
    /// Stdout, up to the budget's ceiling.
    pub stdout: Vec<u8>,
    /// Stderr, up to the budget's ceiling.
    pub stderr: Vec<u8>,
    /// Whether the ceiling cut the output short. The child was given a closed
    /// pipe rather than a growing buffer, so it sees a broken pipe and stops.
    pub truncated: bool,
    /// Whether the watchdog had to end it.
    pub timed_out: bool,
    /// Whether the memory ceiling was real on this platform.
    pub memory: MemoryEnforcement,
}

impl Child {
    /// Start a process inside a containment.
    ///
    /// # Errors
    ///
    /// When the program cannot be started, or the platform will not contain it.
    pub fn spawn(
        spec: &SpawnSpec,
        wall_clock: Duration,
        output_ceiling: u64,
        memory_bytes: Option<u64>,
    ) -> Result<Self, BrokerError> {
        let mut command = tokio::process::Command::new(&spec.program);
        command
            .args(&spec.args)
            .stdin(Stdio::null())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        if let Some(cwd) = &spec.cwd {
            command.current_dir(cwd);
        }
        if !spec.env.is_empty() {
            command.env_clear().envs(&spec.env);
        }

        let prepared = contain(&mut command, Containment { memory_bytes })?;
        let mut inner = command
            .spawn()
            .map_err(|e| BrokerError::io(&spec.program, e))?;
        let guard = prepared.attach(&inner)?;

        Ok(Self {
            stdout: inner.stdout.take(),
            stderr: inner.stderr.take(),
            inner,
            guard,
            output_ceiling,
            wall_clock,
        })
    }

    /// Whether the memory ceiling is enforced or merely sampled here.
    #[must_use]
    pub fn memory_enforcement(&self) -> MemoryEnforcement {
        self.guard.memory_enforcement()
    }

    /// The child's own process id, before anything it spawns.
    #[must_use]
    pub fn process_id(&self) -> Option<u32> {
        self.inner.id()
    }

    /// Kill the process and everything it started, now.
    pub fn kill_tree(&mut self) {
        self.guard.kill_tree();
    }

    /// Wait for it, bounded on both axes.
    ///
    /// Output is read **while** the child runs and stops at the ceiling; the
    /// pipe is then dropped, so a child writing faster than we read gets a
    /// broken pipe rather than growing our heap. A child that ignores the ask to
    /// stop is killed once the grace window is up.
    ///
    /// # Errors
    ///
    /// When the pipes cannot be read.
    pub async fn wait(mut self) -> Result<Output, BrokerError> {
        let ceiling = self.output_ceiling;
        let out_pipe = self.stdout.take();
        let err_pipe = self.stderr.take();

        let pump = async move {
            let mut out = Vec::new();
            let mut err = Vec::new();
            let mut truncated = false;
            if let Some(pipe) = out_pipe {
                let mut reader = LimitedReader::new(pipe, ceiling);
                let (bytes, cut) = reader.take_bytes().await?;
                out = bytes;
                truncated |= cut;
                // Dropping the reader closes our end of the pipe. That is the
                // backpressure: the child's next write fails instead of our
                // buffer growing.
            }
            if let Some(pipe) = err_pipe {
                let mut reader = LimitedReader::new(pipe, ceiling);
                let (bytes, cut) = reader.take_bytes().await?;
                err = bytes;
                truncated |= cut;
            }
            Ok::<_, BrokerError>((out, err, truncated))
        };

        let (stdout, stderr, truncated) = match tokio::time::timeout(self.wall_clock, pump).await {
            Ok(result) => result?,
            Err(_) => {
                self.guard.kill_tree();
                let _ = self.inner.start_kill();
                return Ok(Output {
                    code: None,
                    stdout: Vec::new(),
                    stderr: Vec::new(),
                    truncated: true,
                    timed_out: true,
                    memory: self.guard.memory_enforcement(),
                });
            }
        };

        // Ask it to stop, then insist. A child that ignores the ask is killed
        // once the grace window is up, with its whole tree.
        let waited = tokio::time::timeout(self.wall_clock, self.inner.wait()).await;
        let (code, timed_out) = match waited {
            Ok(Ok(status)) => (status.code(), false),
            Ok(Err(e)) => return Err(BrokerError::io("<child>", e)),
            Err(_) => {
                let _ = self.inner.start_kill();
                let hard = tokio::time::timeout(GRACE, self.inner.wait()).await;
                if hard.is_err() {
                    self.guard.kill_tree();
                }
                (None, true)
            }
        };

        Ok(Output {
            code,
            stdout,
            stderr,
            truncated,
            timed_out,
            memory: self.guard.memory_enforcement(),
        })
    }
}
