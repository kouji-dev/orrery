use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, PtySize};
use tauri::{AppHandle, Emitter, Runtime};
use uuid::Uuid;

use crate::agents::adapters::{self, HookEnv};
use crate::agents::model::Agent;

pub mod jobobj;

type ProcMap = Arc<Mutex<HashMap<Uuid, Proc>>>;

/// Runs agent CLI tools in their worktrees over a PTY and streams their output.
pub struct RuntimeService {
    // Shared with each agent's WAIT thread so the thread can remove its own
    // entry once the child exits (for ANY reason). Removing the entry drops
    // `master` + `writer`, closing the PTY and letting the reader thread end.
    procs: ProcMap,
}

struct Proc {
    // Signals the child to terminate from `stop()`/`stop_all()` without owning
    // the `Child` (which lives in the WAIT thread, blocked in `child.wait()`).
    killer: Box<dyn ChildKiller + Send + Sync>,
    // child pid, captured at spawn so the Unix kill path can `killpg` the whole
    // process group even though the `Child` itself moved to the wait thread.
    #[cfg_attr(not(unix), allow(dead_code))]
    pid: Option<u32>,
    // keep the master alive so the PTY stays open + drives resize
    master: Box<dyn portable_pty::MasterPty + Send>,
    // stdin channel into the PTY (xterm keystrokes land here)
    writer: Mutex<Box<dyn Write + Send>>,
}

impl Default for RuntimeService {
    fn default() -> Self {
        Self::new()
    }
}

impl RuntimeService {
    pub fn new() -> Self {
        Self {
            procs: Arc::new(Mutex::new(HashMap::new())),
        }
    }

    pub fn is_running(&self, id: Uuid) -> bool {
        self.procs.lock().unwrap().contains_key(&id)
    }

    /// `(id, pid)` for every running agent that has a captured child pid. Used by
    /// the metrics push loop to sample each agent's process subtree. Agents whose
    /// `pid` was never captured (None) are skipped.
    pub fn pids(&self) -> Vec<(Uuid, u32)> {
        self.procs
            .lock()
            .unwrap()
            .iter()
            .filter_map(|(id, p)| p.pid.map(|pid| (*id, pid)))
            .collect()
    }

    /// Launch the agent's tool in its worktree over a PTY, streaming output as
    /// `agent://output` events. The initial task prompt is passed only on the
    /// first launch (`send_prompt`); resumes start a bare interactive session.
    ///
    /// When `resume_session` is `Some(id)` AND the tool has a resume-by-id flow
    /// (`AgentAdapter::resume_argv`), the tool is relaunched into that CLI session
    /// (e.g. `claude --resume <id>`) and the prompt is never re-sent. Otherwise it
    /// falls back to the normal launch (honoring `send_prompt`).
    pub fn start<R: Runtime>(
        &self,
        app: AppHandle<R>,
        agent: &Agent,
        rows: u16,
        cols: u16,
        send_prompt: bool,
        hooks: Option<&HookEnv>,
        resume_session: Option<&str>,
    ) -> Result<(), String> {
        let id = agent.id;
        if self.is_running(id) {
            return Ok(());
        }
        let worktree = Path::new(&agent.worktree);
        if !worktree.is_dir() {
            return Err("worktree not found".into());
        }

        // open the PTY at the UI terminal's actual size so the agent's TUI is not
        // truncated; fall back to a sane default when the caller doesn't know it yet.
        let pty = native_pty_system();
        let pair = pty
            .openpty(PtySize {
                rows: if rows > 0 { rows } else { 30 },
                cols: if cols > 0 { cols } else { 120 },
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())?;

        let mut cmd = tool_command(&agent.tool, &agent.task, send_prompt, resume_session);
        cmd.cwd(worktree);

        // Hooks are installed GLOBALLY at app startup (merged into the user's real
        // config), so the runtime no longer installs them per-launch. It only
        // stamps the KATRIX_* env so a katrix-launched agent's hook brokers with
        // the bridge; absent that env the global hook is a harmless no-op.
        if let Some(env) = hooks {
            if let Some(adapter) = adapters::adapter_for(&agent.tool) {
                if adapter.supports_hooks() {
                    for (k, v) in adapter.env(worktree, env) {
                        cmd.env(k, v);
                    }
                }
            }
        }

        let mut child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        // Split a killer out of the Child BEFORE we move the Child into the wait
        // thread, so stop()/stop_all() can signal it independently while the wait
        // thread is blocked in `child.wait()`.
        let killer = child.clone_killer();
        let pid = child.process_id();

        // stream stdout/stderr → `agent://output` on a background thread. This
        // thread DOES NOT emit `agent://exit`; it just ends cleanly once the
        // master is dropped (when the wait thread removes the proc from the map).
        let app_out = app.clone();
        let out_id = id.to_string();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let chunk = String::from_utf8_lossy(&buf[..n]).to_string();
                        let _ = app_out.emit(
                            "agent://output",
                            serde_json::json!({ "id": out_id, "chunk": chunk }),
                        );
                    }
                }
            }
        });

        self.procs.lock().unwrap().insert(
            id,
            Proc {
                killer,
                pid,
                master: pair.master,
                writer: Mutex::new(writer),
            },
        );

        // WAIT thread: owns the Child and blocks on `child.wait()`, which returns
        // when the process exits for ANY reason — natural completion, a crash, a
        // user Ctrl+C, or our own kill(). Then it removes the proc from the shared
        // map (dropping `master`+`writer`, which closes the PTY so the reader
        // thread ends) and emits `agent://exit` EXACTLY ONCE. This is the single
        // source of exit detection, so `is_running()` flips false (restartable)
        // and the UI flips the agent to idle.
        let procs = Arc::clone(&self.procs);
        let app_exit = app.clone();
        let exit_id = id.to_string();
        std::thread::spawn(move || {
            let _ = child.wait();
            // Remove our own entry if it's still present. stop()/stop_all() may
            // have already removed it (and killed us) — either way the map ends
            // up without this id, so `is_running()` is false afterwards.
            procs.lock().unwrap().remove(&id);
            let _ = app_exit.emit("agent://exit", serde_json::json!({ "id": exit_id }));
        });

        Ok(())
    }

    pub fn stop(&self, id: Uuid) {
        if let Some(mut p) = self.procs.lock().unwrap().remove(&id) {
            kill_proc(&mut p);
        }
    }

    /// Kill every running PTY process (called on app shutdown so none orphan).
    /// Idempotent: a second call (e.g. ExitRequested then Exit) just drains an
    /// already-empty map and does nothing.
    pub fn stop_all(&self) {
        let mut procs = self.procs.lock().unwrap();
        for (_, mut p) in procs.drain() {
            kill_proc(&mut p);
        }
    }

    /// Feed keystrokes from the UI terminal into the agent's PTY stdin.
    pub fn write(&self, id: Uuid, data: &str) -> Result<(), String> {
        let procs = self.procs.lock().unwrap();
        let proc = procs.get(&id).ok_or("process not running")?;
        let mut writer = proc.writer.lock().unwrap();
        writer
            .write_all(data.as_bytes())
            .map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())
    }

    /// Resize the PTY so the agent's TUI reflows to the visible terminal.
    pub fn resize(&self, id: Uuid, rows: u16, cols: u16) -> Result<(), String> {
        let procs = self.procs.lock().unwrap();
        let proc = procs.get(&id).ok_or("process not running")?;
        proc.master
            .resize(PtySize {
                rows,
                cols,
                pixel_width: 0,
                pixel_height: 0,
            })
            .map_err(|e| e.to_string())
    }
}

/// Tear down a single PTY process and (on Unix) its whole process group.
///
/// portable_pty makes each PTY child a session/group leader via `setsid()`, so
/// the agent's grandchildren share its process group. `child.kill()` alone only
/// signals the direct child; we additionally `killpg` the group so the entire
/// subtree dies. On Windows the Job Object (see `jobobj`) is the real backstop —
/// `child.kill()` here is the graceful-path best effort.
fn kill_proc(p: &mut Proc) {
    #[cfg(unix)]
    {
        // SAFETY: pure libc signal calls; we only read the child pid (captured at
        // spawn) and signal its group. Best-effort — errors (already-dead group)
        // are ignored.
        if let Some(pid) = p.pid {
            unsafe {
                let pgid = libc::getpgid(pid as libc::pid_t);
                if pgid > 0 {
                    libc::killpg(pgid, libc::SIGKILL);
                }
            }
        }
    }
    // Signal the child directly. The WAIT thread (blocked in `child.wait()`)
    // observes the exit, removes the entry, and emits `agent://exit`.
    let _ = p.killer.kill();
}

/// Map a tool id → its CLI invocation via its adapter.
///
/// When `resume_session` is `Some(id)` and the tool's adapter offers a
/// resume-by-id flow, the relaunch command (e.g. `claude --resume <id>`) is built
/// and the prompt is never sent. Otherwise the normal launch is built: the task
/// prompt is passed only on the first launch (`send_prompt`); resumes open the
/// tool bare. Unknown tools fall back to invoking the id verbatim.
fn tool_command(
    tool: &str,
    task: &str,
    send_prompt: bool,
    resume_session: Option<&str>,
) -> CommandBuilder {
    let adapter = crate::agents::adapters::adapter_for(tool);

    // Prefer a resume-by-id launch when a session id is present AND the adapter
    // supports it (no prompt — the session continues from where it left off).
    if let (Some(id), Some(adapter)) = (resume_session, adapter.as_ref()) {
        if let Some(cmd) = adapter.build_resume_command(id) {
            return cmd;
        }
    }

    let prompt = if send_prompt && !task.is_empty() { Some(task) } else { None };
    match adapter {
        Some(adapter) => adapter.build_command(prompt),
        None => {
            let mut c = CommandBuilder::new(tool);
            if let Some(t) = prompt {
                c.arg(t);
            }
            c
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // Spawning a real PTY needs a Tauri AppHandle (and a real agent CLI), which
    // isn't available in a `--lib` unit test, so we exercise the running-map
    // contract directly: `is_running()` tracks map membership, and `stop()` on an
    // id removes the entry so the agent becomes restartable. This mirrors what the
    // WAIT thread does on natural exit (remove → is_running() false).

    // The pure command builder: a resume session id + a resume-capable adapter
    // (claude) builds `claude --resume <id>` and NEVER appends the task prompt,
    // even when send_prompt is true. We assert the argv straight off CommandBuilder
    // (no PTY spawned).
    #[test]
    fn tool_command_resume_builds_claude_resume_without_prompt() {
        let cmd = tool_command("claude", "fix the bug", true, Some("sess-123"));
        let argv: Vec<String> = cmd
            .get_argv()
            .iter()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert_eq!(argv, vec!["claude", "--resume", "sess-123"]);
    }

    // Without a resume session id, the normal launch is built: claude + the task
    // prompt on the first launch (send_prompt true).
    #[test]
    fn tool_command_normal_launch_sends_prompt_on_first_run() {
        let cmd = tool_command("claude", "fix the bug", true, None);
        let argv: Vec<String> = cmd
            .get_argv()
            .iter()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert_eq!(argv, vec!["claude", "fix the bug"]);
    }

    // A resume session id for a tool WITHOUT a resume-by-id flow (gemini) falls
    // back to the normal launch (no prompt on a resume — send_prompt false).
    #[test]
    fn tool_command_resume_falls_back_when_adapter_has_no_resume() {
        let cmd = tool_command("gemini", "fix the bug", false, Some("sess-123"));
        let argv: Vec<String> = cmd
            .get_argv()
            .iter()
            .map(|s| s.to_string_lossy().to_string())
            .collect();
        assert_eq!(argv, vec!["gemini"], "no resume_argv → bare launch");
    }

    #[test]
    fn stop_removes_entry_and_clears_is_running() {
        let svc = RuntimeService::new();
        let id = Uuid::new_v4();

        // Simulate a running proc by inserting a sentinel killer (no real child).
        svc.procs
            .lock()
            .unwrap()
            .insert(id, fake_proc());
        assert!(svc.is_running(id), "should be running once inserted");

        svc.stop(id);
        assert!(
            !svc.is_running(id),
            "stop() must remove the entry so the agent is restartable"
        );
    }

    #[test]
    fn stop_on_absent_id_is_noop() {
        let svc = RuntimeService::new();
        // Must not panic / must stay not-running for an id that was never started
        // (the case where the WAIT thread already removed it on natural exit).
        svc.stop(Uuid::new_v4());
        assert!(!svc.is_running(Uuid::new_v4()));
    }

    /// A `Proc` backed by a REAL PTY master/writer (cheap to open, no child
    /// spawned) and an inert killer — enough to satisfy the map-membership
    /// contract that `stop()`/the WAIT thread rely on, without launching a process.
    fn fake_proc() -> Proc {
        let pair = native_pty_system()
            .openpty(PtySize::default())
            .expect("open pty");
        let writer = pair.master.take_writer().expect("take writer");
        Proc {
            killer: Box::new(NoopKiller),
            pid: None,
            master: pair.master,
            writer: Mutex::new(writer),
        }
    }

    #[derive(Debug)]
    struct NoopKiller;
    impl ChildKiller for NoopKiller {
        fn kill(&mut self) -> std::io::Result<()> {
            Ok(())
        }
        fn clone_killer(&self) -> Box<dyn ChildKiller + Send + Sync> {
            Box::new(NoopKiller)
        }
    }
}
