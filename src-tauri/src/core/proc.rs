//! Spawn child processes without flashing a console window on Windows.
//!
//! The app runs as a GUI (windows subsystem) process. When it shells out to a
//! console program via `std::process::Command` — `git push`, `npx ccusage`, … —
//! Windows allocates a brand-new console window for the child, which pops up and
//! steals focus before the command finishes. The `CREATE_NO_WINDOW` creation
//! flag suppresses that window. (Agent terminals are spawned through ConPTY
//! instead, which is already windowless, so they don't go through here.)
//!
//! No-op on every non-Windows platform, so call sites stay platform-agnostic.

use std::process::Command;

/// Windows `CREATE_NO_WINDOW`: run the child without allocating a console.
#[cfg(windows)]
const CREATE_NO_WINDOW: u32 = 0x0800_0000;

/// Apply the no-console-window flag to `cmd` (Windows only; no-op elsewhere).
/// Returns `&mut cmd` so it chains inside a builder expression.
pub fn no_window(cmd: &mut Command) -> &mut Command {
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(CREATE_NO_WINDOW);
    }
    cmd
}
