//! The ONE construction site for child processes.
//!
//! The app runs as a GUI (windows subsystem) process. When it shells out to a
//! console program via `std::process::Command` — `git push`, `npx ccusage`, … —
//! Windows allocates a brand-new console window for the child, which pops up and
//! steals focus before the command finishes. The `CREATE_NO_WINDOW` creation
//! flag suppresses that window. (Agent terminals are spawned through ConPTY
//! instead, which is already windowless, so they don't go through here.)
//!
//! Every spawn must go through [`cmd`] — `std::process::Command::new` is banned
//! repo-wide via clippy's `disallowed-methods` (see `clippy.toml`), so a future
//! call site cannot forget the flag. Callers needing EXTRA creation flags (e.g.
//! the updater's `CREATE_BREAKAWAY_FROM_JOB`) call `.creation_flags()` again with
//! `NO_WINDOW | <extra>` — the flags REPLACE, they don't accumulate.
//!
//! No-op on every non-Windows platform, so call sites stay platform-agnostic.

use std::ffi::OsStr;
use std::process::Command;

/// Windows `CREATE_NO_WINDOW`: run the child without allocating a visible console.
#[cfg(windows)]
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Windows `BELOW_NORMAL_PRIORITY_CLASS`: schedule the child below interactive
/// work. For housekeeping children (ccusage) that must never compete with the
/// UI or agent processes for CPU.
#[cfg(windows)]
pub const BELOW_NORMAL_PRIORITY_CLASS: u32 = 0x0000_4000;

/// Build a [`Command`] that will never flash a console window on Windows.
/// The only sanctioned `Command` constructor in this codebase.
pub fn cmd(program: impl AsRef<OsStr>) -> Command {
    #[allow(clippy::disallowed_methods)] // the one place Command::new may be called
    let mut c = Command::new(program);
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        c.creation_flags(CREATE_NO_WINDOW);
    }
    c
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn cmd_spawns_and_completes_without_error() {
        // the helper must produce a runnable Command on each platform
        let mut c = if cfg!(windows) {
            let mut c = cmd("cmd");
            c.args(["/C", "exit 0"]);
            c
        } else {
            let mut c = cmd("sh");
            c.args(["-c", "exit 0"]);
            c
        };
        let status = c.status().expect("spawn through the helper");
        assert!(status.success());
    }
}
