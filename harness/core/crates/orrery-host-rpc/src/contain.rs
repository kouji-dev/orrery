//! Nothing we spawn outlives us.
//!
//! An extension's child process is not the only thing we have to account for:
//! `node` forks, a build tool forks, and `child.kill()` reaches the direct
//! child only. On a force-kill or a crash no shutdown code runs at all, so the
//! guarantee cannot be "we remember to clean up" — it has to be the OS holding
//! the rope.
//!
//! - **Windows**: a Job Object per child, with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`.
//!   The child and everything it spawns inherit the job; when our handle to it
//!   closes — deliberately, or because our process died — the OS kills the tree.
//!   This is `ade/src-tauri/src/runtime/jobobj.rs`'s mechanism, narrowed from
//!   one job for the whole process to one job per extension, because an
//!   extension has to be killable on its own.
//! - **Unix**: the child leads its own process group, and the group is signalled.
//!
//! This is the only module in the harness that writes `unsafe`, and it writes it
//! for exactly one reason: there is no safe wrapper for `AssignProcessToJobObject`.

use std::io;

use tokio::process::{Child, Command};

/// Holds whatever the OS needs held for a child's tree to die with us.
///
/// Dropping it kills the tree. That is the point: a `Containment` is not a
/// handle you may casually let fall out of scope.
#[derive(Debug)]
pub struct Containment {
    pid: u32,
    #[cfg(windows)]
    job: imp::OwnedJob,
}

impl Containment {
    /// Set up the command *before* it is spawned.
    ///
    /// On unix this is where the child is given its own process group; on
    /// Windows nothing is needed until the child exists.
    pub fn configure(command: &mut Command) {
        #[cfg(unix)]
        {
            // Safe: `process_group` is a plain pre-exec setting, not a closure.
            command.process_group(0);
        }
        #[cfg(not(unix))]
        {
            let _ = command;
        }
    }

    /// Take ownership of a spawned child's tree.
    ///
    /// # Errors
    ///
    /// [`io::Error`] when the OS refuses to create or assign the job. The
    /// caller's right move is to kill the child it just spawned: an
    /// uncontainable child is worse than no child.
    pub fn capture(child: &Child) -> io::Result<Self> {
        let pid = child
            .id()
            .ok_or_else(|| io::Error::other("the child has already exited"))?;
        #[cfg(windows)]
        {
            let job = imp::assign(child)?;
            Ok(Self { pid, job })
        }
        #[cfg(not(windows))]
        {
            Ok(Self { pid })
        }
    }

    /// The contained process's id.
    #[must_use]
    pub fn pid(&self) -> u32 {
        self.pid
    }

    /// Kill the child and everything it spawned, now.
    pub fn kill_tree(&self) {
        #[cfg(windows)]
        {
            self.job.terminate();
        }
        #[cfg(unix)]
        {
            // The negative pid is the process group: the child leads its own,
            // so this reaches the grandchildren a `kill(pid)` would leave.
            // SAFETY: `kill` on a pid we spawned; a dead group answers ESRCH,
            // which is the outcome we wanted anyway.
            unsafe {
                libc::kill(-(self.pid as i32), libc::SIGKILL);
            }
        }
        #[cfg(not(any(windows, unix)))]
        {
            tracing::warn!(
                target: "orrery.host.rpc",
                pid = self.pid,
                "no containment on this platform; the child may outlive us"
            );
        }
    }
}

impl Drop for Containment {
    fn drop(&mut self) {
        self.kill_tree();
    }
}

#[cfg(windows)]
mod imp {
    use std::io;

    use tokio::process::Child;
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOBOBJECT_EXTENDED_LIMIT_INFORMATION, JobObjectExtendedLimitInformation,
        SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    /// A job handle that kills its processes when it closes.
    #[derive(Debug)]
    pub struct OwnedJob(HANDLE);

    // A HANDLE is a raw pointer. We only ever assign to it, terminate it and
    // close it, all of which are safe from any thread.
    unsafe impl Send for OwnedJob {}
    unsafe impl Sync for OwnedJob {}

    impl OwnedJob {
        fn is_valid(&self) -> bool {
            !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE
        }

        /// Kill everything in the job without waiting for the handle to close.
        pub fn terminate(&self) {
            if self.is_valid() {
                // SAFETY: a valid job handle; an exit code of 1 for a killed
                // tree is what every other kill path reports.
                unsafe {
                    TerminateJobObject(self.0, 1);
                }
            }
        }
    }

    impl Drop for OwnedJob {
        fn drop(&mut self) {
            if self.is_valid() {
                // Closing the last handle to a kill-on-close job terminates its
                // processes, which is the teardown we want on a crash as much
                // as on a clean exit.
                // SAFETY: a valid handle we own, closed exactly once.
                unsafe {
                    CloseHandle(self.0);
                }
            }
        }
    }

    /// Create a kill-on-close job and put this child in it.
    pub fn assign(child: &Child) -> io::Result<OwnedJob> {
        let pid = child
            .id()
            .ok_or_else(|| io::Error::other("the child has already exited"))?;

        // SAFETY: standard Win32 FFI. Null name and attributes, every returned
        // handle checked, and a zeroed struct of exactly the size declared.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let owned = OwnedJob(job);

            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                std::ptr::from_mut(&mut info).cast(),
                u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                    .expect("the struct is smaller than 4GB"),
            );
            if ok == 0 {
                return Err(io::Error::last_os_error());
            }

            let process = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
            if process.is_null() {
                return Err(io::Error::last_os_error());
            }
            let assigned = AssignProcessToJobObject(job, process);
            CloseHandle(process);
            if assigned == 0 {
                return Err(io::Error::last_os_error());
            }
            Ok(owned)
        }
    }
}

#[cfg(test)]
mod tests {
    use super::Containment;

    /// A process that would outlive us, contained, and then not outliving us.
    #[tokio::test]
    async fn dropping_the_containment_kills_the_tree() {
        // `cmd`/`sh` is on every machine this runs on; no toolchain needed.
        let mut command = if cfg!(windows) {
            let mut c = tokio::process::Command::new("cmd");
            c.args(["/C", "ping -n 600 127.0.0.1 > NUL"]);
            c
        } else {
            let mut c = tokio::process::Command::new("sh");
            c.args(["-c", "sleep 600"]);
            c
        };
        command.stdout(std::process::Stdio::null());
        command.stderr(std::process::Stdio::null());
        Containment::configure(&mut command);

        let mut child = command.spawn().expect("a shell starts");
        let containment = Containment::capture(&child).expect("it is contained");
        drop(containment);

        let exited = tokio::time::timeout(std::time::Duration::from_secs(10), async {
            loop {
                if let Ok(Some(status)) = child.try_wait() {
                    return status;
                }
                tokio::time::sleep(std::time::Duration::from_millis(50)).await;
            }
        })
        .await;
        assert!(exited.is_ok(), "the child outlived its containment");
    }
}
