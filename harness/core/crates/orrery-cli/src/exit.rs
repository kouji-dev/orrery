//! Process exit codes. A CI script's response to 3, 4 and 5 is different, so
//! they are distinct (`17-cli.md`, exit-code table).

/// The exit codes the `orrery` binary may return.
///
/// Only [`Exit::Usage`] is reachable in this build. The mapping from
/// `TurnOutcome` and `KernelError` onto the rest lands with `17-cli.md` task 5,
/// and each code gets a test there; the enum is written out now so the contract
/// is one place rather than scattered literals.
#[allow(dead_code)]
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[repr(i32)]
pub enum Exit {
    /// Success.
    Ok = 0,
    /// The turn completed, but the task failed: a grader or gate said no.
    TaskFailed = 1,
    /// Usage error, including a subcommand that is not implemented yet.
    Usage = 2,
    /// A budget was exceeded.
    Budget = 3,
    /// Denied by policy, with no fallback.
    Denied = 4,
    /// A provider needs a login.
    NeedsLogin = 5,
    /// The kernel or the transport failed.
    Kernel = 6,
}

impl Exit {
    /// Terminate the process with this code.
    pub fn exit(self) -> ! {
        std::process::exit(self as i32)
    }
}

/// Report that a subcommand is present in the tree but not implemented in this
/// build, naming the plan file that will implement it, and exit [`Exit::Usage`].
///
/// **stdout is data, stderr is narration**: this writes to stderr only, so
/// `--json` stdout stays parseable even for a command that does nothing yet.
pub fn not_implemented(plan: &str) -> ! {
    eprintln!("not implemented in this build — see harness/docs/plans/{plan}");
    Exit::Usage.exit()
}
