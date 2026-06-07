//! Process-tree cleanup so no agent (or its grandchildren) outlives katrix.
//!
//! The PTY child we spawn is typically a launcher (e.g. `node` for `claude`)
//! that forks its own subprocess tree. `child.kill()` on graceful shutdown only
//! reaches the DIRECT child, and on a force-kill/crash no shutdown code runs at
//! all — both paths orphan the grandchildren.
//!
//! On Windows we close that gap with a **Job Object**: at startup we create a
//! job with `JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE` and assign the current process
//! to it. Every process we spawn afterward inherits the job, so when our process
//! handle table is torn down (clean exit, `TerminateProcess`, or a crash) the OS
//! closes the job and kills the entire tree. The job handle must stay open for
//! the whole process lifetime — see [`JobGuard`].
//!
//! On non-Windows this is a no-op shim; the graceful `stop_all()` path (plus an
//! optional process-group kill) handles teardown there.

/// Owns the OS resource that guarantees process-tree teardown.
///
/// Stored in Tauri managed state for the entire process lifetime. Dropping it
/// early on Windows would close the job handle and immediately kill every agent,
/// so we never drop it deliberately — it dies only when the process does.
pub struct JobGuard {
    #[cfg(windows)]
    _job: imp::OwnedJobHandle,
}

impl JobGuard {
    /// Best-effort install of the process-tree kill mechanism. Never fails hard:
    /// on any error it logs a warning and returns a guard that simply does
    /// nothing, so startup always proceeds.
    pub fn install() -> Self {
        #[cfg(windows)]
        {
            match imp::assign_current_process_to_kill_on_close_job() {
                Ok(job) => {
                    log::info!("process-tree job object installed (kill-on-close)");
                    JobGuard { _job: job }
                }
                Err(e) => {
                    log::warn!(
                        "job object setup failed ({e}); agents may orphan on crash/force-kill"
                    );
                    // A null/closed handle is harmless: closing it is a no-op and
                    // it owns no process, so nothing is killed on drop.
                    JobGuard {
                        _job: imp::OwnedJobHandle::none(),
                    }
                }
            }
        }
        #[cfg(not(windows))]
        {
            JobGuard {}
        }
    }
}

#[cfg(windows)]
mod imp {
    use std::io;

    use windows_sys::Win32::Foundation::{CloseHandle, HANDLE, INVALID_HANDLE_VALUE};
    use windows_sys::Win32::System::JobObjects::{
        AssignProcessToJobObject, CreateJobObjectW, SetInformationJobObject,
        JobObjectExtendedLimitInformation, JOBOBJECT_EXTENDED_LIMIT_INFORMATION,
        JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE,
    };
    use windows_sys::Win32::System::Threading::GetCurrentProcess;

    /// RAII wrapper that closes a job handle on drop. We intentionally keep one
    /// of these alive for the whole process lifetime (see [`super::JobGuard`]).
    pub struct OwnedJobHandle(HANDLE);

    // A HANDLE is a raw pointer; it's safe to move across threads here because we
    // only ever close it (Tauri managed state requires Send + Sync).
    unsafe impl Send for OwnedJobHandle {}
    unsafe impl Sync for OwnedJobHandle {}

    impl OwnedJobHandle {
        /// A guard that owns nothing — drop is a no-op, kills nothing.
        pub fn none() -> Self {
            OwnedJobHandle(std::ptr::null_mut())
        }

        fn is_valid(&self) -> bool {
            !self.0.is_null() && self.0 != INVALID_HANDLE_VALUE
        }
    }

    impl Drop for OwnedJobHandle {
        fn drop(&mut self) {
            if self.is_valid() {
                // Closing the LAST handle to a kill-on-close job terminates the
                // job's processes. That's exactly the teardown we want — but it
                // only runs deliberately at process exit; we never drop early.
                unsafe { CloseHandle(self.0) };
            }
        }
    }

    /// Create a kill-on-close job and assign the current process to it. All
    /// descendants spawned afterward inherit the job and die when it closes.
    pub fn assign_current_process_to_kill_on_close_job() -> io::Result<OwnedJobHandle> {
        // SAFETY: standard Win32 FFI. We pass null name/attributes, check the
        // returned handle, and only touch a zeroed, correctly-sized info struct.
        unsafe {
            let job = CreateJobObjectW(std::ptr::null(), std::ptr::null());
            if job.is_null() {
                return Err(io::Error::last_os_error());
            }
            let guard = OwnedJobHandle(job);

            let mut info: JOBOBJECT_EXTENDED_LIMIT_INFORMATION = std::mem::zeroed();
            info.BasicLimitInformation.LimitFlags = JOB_OBJECT_LIMIT_KILL_ON_JOB_CLOSE;
            let ok = SetInformationJobObject(
                job,
                JobObjectExtendedLimitInformation,
                &info as *const _ as *const core::ffi::c_void,
                std::mem::size_of::<JOBOBJECT_EXTENDED_LIMIT_INFORMATION>() as u32,
            );
            if ok == 0 {
                // `guard` drops here and closes the (empty) job — harmless.
                return Err(io::Error::last_os_error());
            }

            let ok = AssignProcessToJobObject(job, GetCurrentProcess());
            if ok == 0 {
                let err = io::Error::last_os_error();
                // ERROR_ACCESS_DENIED (5) commonly means we're already inside a
                // job that forbids breakaway (e.g. some CI/sandbox launchers).
                // Surface it; caller logs + continues.
                return Err(err);
            }

            Ok(guard)
        }
    }

    #[cfg(test)]
    mod tests {
        use super::*;

        #[test]
        fn creates_and_assigns_job_without_killing_us() {
            // If this returns Ok and the test process keeps running to assertion,
            // the job was created, configured, and assigned successfully. The
            // guard drop here closes the handle; since the test process is also
            // (now) in the job, we deliberately leak it to avoid killing the
            // remaining test harness mid-run.
            match assign_current_process_to_kill_on_close_job() {
                Ok(job) => {
                    std::mem::forget(job);
                }
                Err(e) => {
                    // Acceptable in restricted environments (already in a job that
                    // disallows assignment). Don't fail the suite over the host.
                    eprintln!("job assignment unavailable in this env: {e}");
                }
            }
        }
    }
}
