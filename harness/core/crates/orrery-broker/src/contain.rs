//! Keeping a spawned process — and everything it spawns — inside the box.
//!
//! Windows uses a per-call **Job Object** created with
//! `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE`: every process the child starts inherits
//! the job, so closing the handle kills the whole tree even if we crash. Ported
//! from `ade/src-tauri/src/runtime/jobobj.rs`, narrowed from one process-wide
//! job to one job per call so a single call's tree can be killed on its own.
//!
//! Unix puts the child in its own session with `setsid`, so a kill can address
//! the process **group** and reach grandchildren that `Child::kill` never would.
//!
//! # Memory ceilings are not the same everywhere
//!
//! Open question 4 of the plan, decided: **document it and report
//! [`MemoryEnforcement::Unenforced`]**, rather than refusing `memory_bytes`
//! grants on macOS or pretending the sampling is a limit. A refusal makes a
//! portable manifest unportable for a platform difference the manifest did not
//! cause; a lie is worse. The ledger says which one you got.

use crate::error::BrokerError;

/// Whether a memory ceiling is actually being enforced by the operating system.
#[non_exhaustive]
#[derive(Copy, Clone, Debug, PartialEq, Eq, Hash)]
pub enum MemoryEnforcement {
    /// No ceiling was asked for.
    NotRequested,
    /// The kernel will refuse the allocation. Windows job objects, Linux
    /// `RLIMIT_AS`.
    Enforced,
    /// Nothing enforces it; the broker samples and kills after the fact, which
    /// is a real gap and is reported as one. macOS.
    Unenforced,
}

/// What a containment is asked to do.
#[derive(Copy, Clone, Debug, Default)]
pub struct Containment {
    /// The per-process memory ceiling, when one was granted.
    pub memory_bytes: Option<u64>,
}

/// The platform's grip on one spawned process tree.
#[derive(Debug)]
pub struct Guard {
    enforcement: MemoryEnforcement,
    #[cfg(windows)]
    job: imp::OwnedJob,
    #[cfg(not(windows))]
    pgid: Option<i32>,
}

impl Guard {
    /// Whether the memory ceiling is real on this platform.
    #[must_use]
    pub fn memory_enforcement(&self) -> MemoryEnforcement {
        self.enforcement
    }

    /// Kill the process and everything it started.
    pub fn kill_tree(&mut self) {
        #[cfg(windows)]
        self.job.terminate();
        #[cfg(not(windows))]
        if let Some(pgid) = self.pgid {
            // SAFETY: `killpg` takes a process-group id and a signal and has no
            // memory-safety contract; a group that has already exited answers
            // ESRCH, which is exactly the outcome we want anyway.
            unsafe {
                libc::killpg(pgid, libc::SIGKILL);
            }
        }
    }
}

/// Everything the platform needs to do before and after the child starts.
///
/// # Errors
///
/// When the job object cannot be created or the child cannot be assigned to it.
pub fn contain(
    command: &mut tokio::process::Command,
    limits: Containment,
) -> Result<Prepared, BrokerError> {
    #[cfg(not(windows))]
    {
        prepare_unix(command, limits)
    }
    #[cfg(windows)]
    {
        let _ = command;
        imp::prepare(limits)
    }
}

/// The half of containment that has to happen after the child exists.
#[derive(Debug)]
pub struct Prepared {
    enforcement: MemoryEnforcement,
    #[cfg(windows)]
    job: Option<imp::OwnedJob>,
}

impl Prepared {
    /// Take hold of the child that was just started.
    ///
    /// # Errors
    ///
    /// When the child cannot be assigned to its job.
    pub fn attach(self, child: &tokio::process::Child) -> Result<Guard, BrokerError> {
        #[cfg(windows)]
        {
            let mut job = self.job.unwrap_or_else(imp::OwnedJob::none);
            if let Some(pid) = child.id() {
                job.assign(pid)?;
            }
            Ok(Guard {
                enforcement: self.enforcement,
                job,
            })
        }
        #[cfg(not(windows))]
        {
            Ok(Guard {
                enforcement: self.enforcement,
                pgid: child.id().and_then(|id| i32::try_from(id).ok()),
            })
        }
    }
}

#[cfg(not(windows))]
fn prepare_unix(
    command: &mut tokio::process::Command,
    limits: Containment,
) -> Result<Prepared, BrokerError> {
    let memory = limits.memory_bytes;
    // SAFETY: the closure runs between `fork` and `exec` in the child, where
    // only async-signal-safe calls are allowed. `setsid` and `setrlimit` are
    // both on that list, and nothing here allocates.
    unsafe {
        std::os::unix::process::CommandExt::pre_exec(command, move || {
            libc::setsid();
            if let Some(bytes) = memory {
                let limit = libc::rlimit {
                    rlim_cur: bytes as libc::rlim_t,
                    rlim_max: bytes as libc::rlim_t,
                };
                libc::setrlimit(libc::RLIMIT_AS, &limit);
            }
            Ok(())
        });
    }
    let enforcement = match (limits.memory_bytes, cfg!(target_os = "macos")) {
        (None, _) => MemoryEnforcement::NotRequested,
        // `RLIMIT_AS` on macOS does not bound resident memory the way it does on
        // Linux, so a ceiling there is sampling, not enforcement.
        (Some(_), true) => MemoryEnforcement::Unenforced,
        (Some(_), false) => MemoryEnforcement::Enforced,
    };
    Ok(Prepared { enforcement })
}

#[cfg(windows)]
mod imp {
    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
        JOB_OBJECT_LIMIT_PROCESS_MEMORY, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JobObjectExtendedLimitInformation, SetInformationJobObject, TerminateJobObject,
    };
    use windows_sys::Win32::System::Threading::{
        OpenProcess, PROCESS_SET_QUOTA, PROCESS_TERMINATE,
    };

    use super::{Containment, MemoryEnforcement, Prepared};
    use crate::error::BrokerError;

    /// A job handle that kills everything in it when it closes.
    #[derive(Debug)]
    pub struct OwnedJob(HANDLE);

    // SAFETY: a HANDLE is a raw pointer with no thread affinity; this type only
    // ever assigns, terminates and closes it.
    unsafe impl Send for OwnedJob {}
    unsafe impl Sync for OwnedJob {}

    impl OwnedJob {
        pub fn none() -> Self {
            OwnedJob(std::ptr::null_mut())
        }

        fn is_valid(&self) -> bool {
            !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE
        }

        pub fn assign(&mut self, pid: u32) -> Result<(), BrokerError> {
            if !self.is_valid() {
                return Ok(());
            }
            // SAFETY: `pid` names a process we just started; a handle we fail to
            // open is reported rather than used, and one we do open is closed.
            unsafe {
                let handle = OpenProcess(PROCESS_SET_QUOTA | PROCESS_TERMINATE, 0, pid);
                if handle.is_null() {
                    return Err(BrokerError::Unsupported(format!(
                        "could not open process {pid} to contain it: {}",
                        std::io::Error::last_os_error()
                    )));
                }
                let ok = AssignProcessToJobObject(self.0, handle);
                CloseHandle(handle);
                if ok == 0 {
                    return Err(BrokerError::Unsupported(format!(
                        "could not assign process {pid} to its job: {}",
                        std::io::Error::last_os_error()
                    )));
                }
            }
            Ok(())
        }

        pub fn terminate(&mut self) {
            if !self.is_valid() {
                return;
            }
            // SAFETY: the handle is ours and still open; terminating a job that
            // is already empty is a no-op.
            unsafe {
                TerminateJobObject(self.0, 1);
            }
        }
    }

    impl Drop for OwnedJob {
        fn drop(&mut self) {
            if self.is_valid() {
                // SAFETY: the handle is ours and is closed exactly once. Closing
                // it is what kills the tree, by KILL_ON_JOB_CLOSE.
                unsafe {
                    CloseHandle(self.0);
                }
            }
            self.0 = std::ptr::null_mut();
        }
    }

    pub fn prepare(limits: Containment) -> Result<Prepared, BrokerError> {
        // SAFETY: an unnamed job object; the handle is wrapped immediately and
        // the limit struct is zeroed C layout, which is what the API expects.
        let job = unsafe { CreateJobObjectW(std::ptr::null(), std::ptr::null()) };
        if job.is_null() {
            return Err(BrokerError::Unsupported(format!(
                "could not create a job object: {}",
                std::io::Error::last_os_error()
            )));
        }
        let job = OwnedJob(job);

        let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = unsafe { std::mem::zeroed() };
        info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
        if let Some(bytes) = limits.memory_bytes {
            info.BasicLimitInformation.LimitFlags |= JOB_OBJECT_LIMIT_PROCESS_MEMORY;
            info.ProcessMemoryLimit = usize::try_from(bytes).unwrap_or(usize::MAX);
        }
        // SAFETY: `info` is a correctly sized, correctly typed limit struct for
        // `JobObjectExtendedLimitInformation`, and the job handle is valid.
        let ok = unsafe {
            SetInformationJobObject(
                job.0,
                JobObjectExtendedLimitInformation,
                std::ptr::from_ref(&info).cast(),
                u32::try_from(std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>())
                    .unwrap_or(0),
            )
        };
        if ok == 0 {
            return Err(BrokerError::Unsupported(format!(
                "could not set job limits: {}",
                std::io::Error::last_os_error()
            )));
        }

        Ok(Prepared {
            enforcement: if limits.memory_bytes.is_some() {
                MemoryEnforcement::Enforced
            } else {
                MemoryEnforcement::NotRequested
            },
            job: Some(job),
        })
    }
}
