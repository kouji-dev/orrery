use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::{Arc, Mutex};

use portable_pty::{native_pty_system, ChildKiller, CommandBuilder, PtySize};
use tauri::{AppHandle, Emitter, Manager, Runtime};
use uuid::Uuid;

use crate::agents::adapters::{self, HookEnv};
use crate::agents::model::Agent;

pub mod jobobj;
pub(crate) mod output_batcher;
pub(crate) mod output_mux;

type ProcMap = Arc<Mutex<HashMap<Uuid, Proc>>>;

/// The process-wide output multiplexer all agents' batchers push into. One
/// drain thread (started lazily by the first `start()`) turns its pending map
/// into a single `agent://output` emit per ~16ms frame — see `output_mux`.
static MUX: std::sync::OnceLock<Arc<output_mux::OutputMux>> = std::sync::OnceLock::new();

fn mux() -> Arc<output_mux::OutputMux> {
    MUX.get_or_init(|| Arc::new(output_mux::OutputMux::new()))
        .clone()
}

/// Guards the drain-thread spawn. Module-level (NOT inside the generic fn —
/// a static in a generic fn is per-monomorphization, which could start one
/// drain thread per `R` and double-emit frames).
static DRAIN_STARTED: std::sync::Once = std::sync::Once::new();

/// Start the single global drain thread on the first agent launch. Lazy
/// because emitting needs an `AppHandle`, which only `start()` has; `Once`
/// because there must be exactly one emitter no matter how many agents launch.
fn ensure_drain_thread<R: Runtime>(app: &AppHandle<R>) {
    let app = app.clone();
    DRAIN_STARTED.call_once(move || {
        let mux = mux();
        std::thread::spawn(move || {
            mux.drain_loop(std::time::Duration::from_millis(16), move |frame| {
                emit_output_frame(&app, frame);
            });
        });
    });
}

/// Emit one multiplexed `agent://output` frame (array payload — one coalesced
/// entry per agent) and record its perf row. record_io: emit RATE + VOLUME
/// land as one perf-table row (`agent_output_emit` calls/10s ≤ ~625 — one per
/// 16ms frame regardless of agent count — is the mux's proof, bytes make it a
/// throughput row) under a single STATS lock.
fn emit_output_frame<R: Runtime>(app: &AppHandle<R>, frame: Vec<output_mux::OutputEntry>) {
    let bytes: u64 = frame.iter().map(|e| e.chunk.len() as u64).sum();
    let t = std::time::Instant::now();
    let _ = app.emit("agent://output", &frame);
    crate::perf::record_io("agent_output_emit", t.elapsed(), bytes);
}

/// Runs agent CLI tools in their worktrees over a PTY and streams their output.
pub struct RuntimeService {
    // Shared with each agent's WAIT thread so the thread can remove its own
    // entry once the child exits (for ANY reason). Removing the entry drops the
    // entry's `master` + `writer` Arcs, closing the PTY and letting the reader
    // thread end (an in-flight write/resize may keep its own handle alive a
    // beat longer — the close then happens when that call returns).
    //
    // LOCK RULE: this map's lock is held ONLY for map ops (insert/remove/
    // lookup/pids/is_running) — never across PTY I/O. Blocking I/O happens
    // under the per-agent `writer`/`master` locks after cloning the Arc.
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
    // keep the master alive so the PTY stays open + drives resize. Arc'd behind
    // its OWN lock so resize() can run the (blocking) ConPTY resize without
    // holding the procs-map lock — one wedged agent pipe must never stall
    // agent_input/agent_resize/is_running/pids for every other agent.
    master: Arc<Mutex<Box<dyn portable_pty::MasterPty + Send>>>,
    // stdin channel into the PTY (xterm keystrokes land here). Per-agent Arc'd
    // lock for the same reason as `master` — write_all/flush can block.
    writer: Arc<Mutex<Box<dyn Write + Send>>>,
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

    /// True when at least one agent PTY is alive — drives the adaptive metrics
    /// cadence (full process sweeps are only worth their cost while agents run).
    pub fn any_running(&self) -> bool {
        !self.procs.lock().unwrap().is_empty()
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
    ///
    /// `approve_policy` is the user's per-tool autoApprove setting ("off" |
    /// "allowlist" | "everything") — mapped to the tool's skip-permissions flag
    /// by its adapter (see `AgentAdapter::auto_approve_args`).
    #[allow(clippy::too_many_arguments)]
    pub fn start<R: Runtime>(
        &self,
        app: AppHandle<R>,
        agent: &Agent,
        rows: u16,
        cols: u16,
        send_prompt: bool,
        hooks: Option<&HookEnv>,
        resume_session: Option<&str>,
        approve_policy: &str,
        program_override: Option<&str>,
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

        let cmd = tool_command(
            &agent.tool,
            &agent.task,
            &agent.model,
            agent.effort.as_deref(),
            send_prompt,
            resume_session,
            approve_policy,
        );
        // A user-set manual executable path (Settings → tool_paths) overrides
        // argv[0] so we launch exactly the binary they pointed us at.
        let cmd = apply_program_override(cmd, program_override);
        // Resolve argv[0] to something CreateProcessW can actually launch (Windows):
        // see resolve_program_command. No-op elsewhere (and a no-op when the
        // override is already an absolute path — that path is taken as-is).
        let mut cmd = resolve_program_command(cmd);
        cmd.cwd(worktree);

        // Hooks are installed GLOBALLY at app startup (merged into the user's real
        // config), so the runtime no longer installs them per-launch. It only
        // stamps the ORRERY_* env so a orrery-launched agent's hook brokers with
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

        // stream stdout/stderr → batcher → global mux → `agent://output`.
        // The batcher thread coalesces reads into ≤4ms / ≥16KB UTF-8-safe
        // chunks tagged with a cumulative byte seq (snapshot-dedup
        // foundation) and pushes them into the process-wide mux; the mux's
        // single drain thread emits ONE array-payload event per ~16ms frame
        // for ALL agents (see `output_mux` — each emit is a Win32 UI-thread
        // job, so per-agent emits made every invoke slower as agents scaled).
        // The batcher window dropped 8ms → 4ms: it no longer paces the IPC
        // (the mux frame does), it only bounds chunk size + fixes multibyte
        // chars split across read boundaries (from_utf8_lossy per read
        // corrupted them).
        // Neither thread emits `agent://exit`; both end once the master is
        // dropped (reader read fails → sender drops → batcher final-flushes).
        ensure_drain_thread(&app);
        let out_id = id.to_string();
        let mux_out = mux();
        let (batch_tx, batch_rx) = std::sync::mpsc::channel::<Vec<u8>>();
        let batcher = std::thread::spawn(move || {
            output_batcher::batch_loop(
                batch_rx,
                std::time::Duration::from_millis(4),
                16 * 1024,
                move |chunk, seq| mux_out.push(&out_id, chunk, seq),
            );
        });
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        // batcher gone (shutdown) → stop reading
                        if batch_tx.send(buf[..n].to_vec()).is_err() {
                            break;
                        }
                    }
                }
            }
        });

        self.procs.lock().unwrap().insert(
            id,
            Proc {
                killer,
                pid,
                master: Arc::new(Mutex::new(pair.master)),
                writer: Arc::new(Mutex::new(writer)),
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
        let mux_exit = mux();
        std::thread::spawn(move || {
            let _ = child.wait();
            // Remove our own entry if it's still present. stop()/stop_all() may
            // have already removed it (and killed us) — either way the map ends
            // up without this id, so `is_running()` is false afterwards. Bind
            // the removed entry so its drop (ConPTY teardown — a blocking OS
            // call) happens AFTER the map guard is released, never under it.
            // The return value is the authoritative natural-vs-stopped signal:
            // Some(_) means we were still registered (natural exit or crash),
            // None means stop()/stop_all() yanked the entry before the process
            // exited (user-initiated kill).
            let removed = procs.lock().unwrap().remove(&id);
            let naturally_exited = removed.is_some();
            drop(removed);
            // Removing the entry dropped `master`, so the reader hits EOF and
            // the batcher final-flushes into the mux, then returns. Join it so
            // EVERYTHING this agent printed is queued, then force-drain the
            // agent's pending ahead of `agent://exit` — otherwise its tail
            // would sit in the mux until the next frame and land AFTER exit.
            // This also cleans the agent's mux entry on stop/remove.
            let _ = batcher.join();
            if let Some(entry) = mux_exit.take(&exit_id) {
                emit_output_frame(&app_exit, vec![entry]);
            }
            // Ticket lifecycle: only complete the ticket on a natural exit
            // (agent ran to completion). When the user calls agent_stop → rt.stop(),
            // stop() removes the proc from the map before killing, so
            // `naturally_exited` is false here. A crash is indistinguishable from
            // a natural exit at the OS level (both leave the proc in the map), so
            // crash exits do complete the ticket — acceptable given that a crashed
            // agent still "finished" the work it was doing up to that point and
            // the ticket needs to be unblocked. If this ever needs refinement,
            // gate additionally on the persisted agent status.
            if naturally_exited {
                use crate::core::events::{emit_entity, Change};
                use crate::tickets::service::TicketService;
                if let Some(tickets) = app_exit.try_state::<TicketService>() {
                    match tickets.complete_for_agent(id) {
                        Ok(Some(ticket)) => {
                            emit_entity(&app_exit, "ticket", Change::Updated, ticket);
                        }
                        Ok(None) => {} // no ticket attached, or already done
                        Err(e) => {
                            log::warn!("complete_for_agent({id}) on exit failed: {e:?}");
                        }
                    }
                }
            }
            let _ = app_exit.emit("agent://exit", serde_json::json!({ "id": exit_id }));
        });

        Ok(())
    }

    pub fn stop(&self, id: Uuid) {
        // Take the entry under the map lock, but kill + drop it OUTSIDE — the
        // child signal and the ConPTY teardown on drop are blocking OS calls,
        // and the map lock is never held across I/O.
        let removed = self.procs.lock().unwrap().remove(&id);
        if let Some(mut p) = removed {
            kill_proc(&mut p);
        }
    }

    /// Kill every running PTY process (called on app shutdown so none orphan).
    /// Idempotent: a second call (e.g. ExitRequested then Exit) just drains an
    /// already-empty map and does nothing.
    pub fn stop_all(&self) {
        // Drain under the lock, kill + drop outside it (same rule as `stop()`).
        let drained: Vec<Proc> = self
            .procs
            .lock()
            .unwrap()
            .drain()
            .map(|(_, p)| p)
            .collect();
        for mut p in drained {
            kill_proc(&mut p);
        }
    }

    /// Mark which agent's terminal is focused in the UI (`None` = none). The
    /// output mux drains the focused agent every frame (keystroke echo stays
    /// snappy) and holds everyone else to the slow ~150ms cadence — see
    /// `output_mux::set_focus`.
    pub fn focus(&self, id: Option<Uuid>) {
        mux().set_focus(id.map(|u| u.to_string()));
    }

    /// Feed keystrokes from the UI terminal into the agent's PTY stdin.
    ///
    /// The procs-map lock is held only long enough to clone the per-agent
    /// writer Arc; the (possibly blocking) ConPTY write/flush then runs under
    /// the per-agent lock alone, so one wedged pipe can't stall map ops —
    /// is_running/pids/start/stop — for every other agent.
    pub fn write(&self, id: Uuid, data: &str) -> Result<(), String> {
        let writer = {
            let procs = self.procs.lock().unwrap();
            let proc = procs.get(&id).ok_or("process not running")?;
            Arc::clone(&proc.writer)
        };
        let mut writer = writer.lock().unwrap();
        writer
            .write_all(data.as_bytes())
            .map_err(|e| e.to_string())?;
        writer.flush().map_err(|e| e.to_string())
    }

    /// Resize the PTY so the agent's TUI reflows to the visible terminal.
    /// Same locking shape as `write()`: brief map lock to clone the per-agent
    /// master Arc, then the blocking resize under the per-agent lock only.
    pub fn resize(&self, id: Uuid, rows: u16, cols: u16) -> Result<(), String> {
        let master = {
            let procs = self.procs.lock().unwrap();
            let proc = procs.get(&id).ok_or("process not running")?;
            Arc::clone(&proc.master)
        };
        let master = master.lock().unwrap();
        master
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
///
/// `approve_policy`, `model`, and `effort` append the adapter's run flags on
/// BOTH the normal and the resume launch — they are properties of the agent run,
/// not of how the session was entered: the auto-approve flag ("everything" → the
/// tool's skip-permissions flag; else none), `--model <model>` (empty → omitted),
/// and the effort flag (only tools with an effort knob, e.g. codex's
/// `--config model_reasoning_effort`). Unknown tools get no flags (no adapter
/// knows their CLI).
/// Swap argv[0] for a user-set manual executable path, keeping every arg the
/// adapter appended. No-op when `program` is `None`/blank or the command somehow
/// has no argv. The replacement is an absolute path, so the later
/// `resolve_program_command` (Windows) takes it verbatim.
fn apply_program_override(cmd: CommandBuilder, program: Option<&str>) -> CommandBuilder {
    let Some(program) = program.map(str::trim).filter(|p| !p.is_empty()) else {
        return cmd;
    };
    let argv: Vec<String> = cmd
        .get_argv()
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    if argv.is_empty() {
        return cmd;
    }
    let mut out = CommandBuilder::new(program);
    for a in &argv[1..] {
        out.arg(a);
    }
    out
}

fn tool_command(
    tool: &str,
    task: &str,
    model: &str,
    effort: Option<&str>,
    send_prompt: bool,
    resume_session: Option<&str>,
    approve_policy: &str,
) -> CommandBuilder {
    let adapter = crate::agents::adapters::adapter_for(tool);
    // Flags that belong to the RUN, not the entry path — appended after the base
    // argv on BOTH a fresh launch and a resume: the auto-approve policy, the model
    // the user picked, and (for tools that support it) the reasoning effort. Model
    // + effort live on the agent record but were never forwarded before, so every
    // agent launched on its tool's default model regardless of the choice.
    let run_args: Vec<String> = adapter
        .as_ref()
        .map(|a| {
            let mut v = a.auto_approve_args(approve_policy);
            v.extend(a.model_args(model));
            if let Some(e) = effort.filter(|e| !e.is_empty()) {
                v.extend(a.effort_args(e));
            }
            v
        })
        .unwrap_or_default();
    let with_run_args = |mut cmd: CommandBuilder| {
        for a in &run_args {
            cmd.arg(a);
        }
        cmd
    };

    // Prefer a resume-by-id launch when a session id is present AND the adapter
    // supports it (no prompt — the session continues from where it left off).
    if let (Some(id), Some(adapter)) = (resume_session, adapter.as_ref()) {
        if let Some(cmd) = adapter.build_resume_command(id) {
            return with_run_args(cmd);
        }
    }

    let prompt = if send_prompt && !task.is_empty() {
        Some(task)
    } else {
        None
    };
    match adapter {
        Some(adapter) => with_run_args(adapter.build_command(prompt)),
        None => {
            let mut c = CommandBuilder::new(tool);
            if let Some(t) = prompt {
                c.arg(t);
            }
            c
        }
    }
}

/// Windows-safe argv[0] resolution for the PTY launch.
///
/// portable-pty hands argv[0] to `CreateProcessW`, which only starts real PE images
/// (`.exe`/`.com`) — a shell/script shim makes it fail with os error 193 ("not a
/// valid Win32 application"). Worse, portable-pty's own PATH search matches an
/// EXTENSIONLESS file in a dir before trying PATHEXT, so when `claude` is installed
/// by BOTH npm (an extensionless Node shim plus `claude.cmd`/`.ps1`, in a PATH dir
/// that precedes winget's) AND winget (a real `claude.exe`), it picks the npm shell
/// script and the spawn dies with 193.
///
/// We resolve argv[0] ourselves before spawning: a real executable anywhere on PATH
/// beats a script shim anywhere, and `.cmd`/`.bat`/`.ps1` shims are wrapped in their
/// interpreter so they actually run. No-op when argv[0] already names a path, when
/// nothing resolves, or off Windows.
#[cfg(windows)]
fn resolve_program_command(cmd: CommandBuilder) -> CommandBuilder {
    let argv: Vec<String> = cmd
        .get_argv()
        .iter()
        .map(|s| s.to_string_lossy().into_owned())
        .collect();
    if argv.is_empty() {
        return cmd;
    }
    let resolved = resolve_program(argv);
    let mut out = CommandBuilder::new(&resolved[0]);
    for a in &resolved[1..] {
        out.arg(a);
    }
    out
}

#[cfg(not(windows))]
fn resolve_program_command(cmd: CommandBuilder) -> CommandBuilder {
    cmd
}

/// Rewrite `argv` so argv[0] is something `CreateProcessW` can launch. See
/// [`resolve_program_command`]. Returns the input unchanged when argv[0] already
/// names a path or nothing resolves on PATH.
#[cfg(windows)]
fn resolve_program(argv: Vec<String>) -> Vec<String> {
    let prog = &argv[0];
    // An explicit path (a manual `tool_paths` override) — still wrap a script or
    // extensionless shim so CreateProcessW can launch it (npm's bare `claude` →
    // `cmd.exe /c call claude.cmd`). A real `.exe` passes through unchanged.
    if prog.contains('/') || prog.contains('\\') {
        let (program, mut head) = crate::agents::adapters::launch_prefix(prog);
        head.extend_from_slice(&argv[1..]);
        let mut out = vec![program];
        out.extend(head);
        return out;
    }
    let dirs: Vec<std::path::PathBuf> = std::env::var_os("PATH")
        .map(|p| std::env::split_paths(&p).collect())
        .unwrap_or_default();
    match find_program(prog, &dirs) {
        Some(found) => wrap_for_ext(&found, &argv[1..]),
        None => argv, // nothing better to offer; let portable-pty try
    }
}

/// Locate `prog` on `dirs`, preferring a real executable over a script shim and a
/// script shim over an extensionless file. ACROSS extensions a higher-priority kind
/// anywhere beats a lower one in an earlier dir (so winget's `claude.exe` wins over
/// npm's bare `claude`); within one extension the earliest PATH dir wins.
#[cfg(windows)]
fn find_program(prog: &str, dirs: &[std::path::PathBuf]) -> Option<std::path::PathBuf> {
    for ext in ["exe", "com", "cmd", "bat", "ps1", ""] {
        for dir in dirs {
            let cand = if ext.is_empty() {
                dir.join(prog)
            } else {
                dir.join(format!("{prog}.{ext}"))
            };
            if cand.is_file() {
                return Some(cand);
            }
        }
    }
    None
}

/// Build the launch argv for a resolved program path: a real executable runs
/// directly; `.cmd`/`.bat` go through `cmd.exe /c`; `.ps1` through PowerShell;
/// anything else (incl. an extensionless file) is handed back as-is, best effort.
#[cfg(windows)]
fn wrap_for_ext(found: &std::path::Path, args_tail: &[String]) -> Vec<String> {
    let full = found.to_string_lossy().into_owned();
    let ext = found
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.to_ascii_lowercase());
    let mut out = match ext.as_deref() {
        // `cmd.exe /c call <shim> <args>` — NOT `/c <shim>`. The npm/pnpm shim often
        // lives in a spaced dir (e.g. `C:\Program Files\nodejs\`), so portable-pty
        // quotes its path; with a spaced task prompt that is 4 quote chars total, and
        // cmd's quote rule then strips the OUTER pair and mangles the path into
        // `C:\Program …`. Prefixing `call` makes the first token after `/c` a
        // non-quote, which suppresses that stripping, so both the quoted shim path
        // and quoted args survive intact.
        Some("cmd") | Some("bat") => vec!["cmd.exe".into(), "/c".into(), "call".into(), full],
        Some("ps1") => vec![
            "powershell.exe".into(),
            "-NoProfile".into(),
            "-ExecutionPolicy".into(),
            "Bypass".into(),
            "-File".into(),
            full,
        ],
        _ => vec![full],
    };
    out.extend(args_tail.iter().cloned());
    out
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
    fn argv_of(cmd: &CommandBuilder) -> Vec<String> {
        cmd.get_argv()
            .iter()
            .map(|s| s.to_string_lossy().to_string())
            .collect()
    }

    #[test]
    fn tool_command_resume_builds_claude_resume_without_prompt() {
        let cmd = tool_command("claude", "fix the bug", "", None, true, Some("sess-123"), "off");
        assert_eq!(argv_of(&cmd), vec!["claude", "--resume", "sess-123"]);
    }

    // Without a resume session id, the normal launch is built: claude + the task
    // prompt on the first launch (send_prompt true).
    #[test]
    fn tool_command_normal_launch_sends_prompt_on_first_run() {
        let cmd = tool_command("claude", "fix the bug", "", None, true, None, "off");
        assert_eq!(argv_of(&cmd), vec!["claude", "fix the bug"]);
    }

    // A resume session id for a tool WITHOUT a resume-by-id flow (gemini) falls
    // back to the normal launch (no prompt on a resume — send_prompt false).
    #[test]
    fn tool_command_resume_falls_back_when_adapter_has_no_resume() {
        let cmd = tool_command("gemini", "fix the bug", "", None, false, Some("sess-123"), "off");
        assert_eq!(argv_of(&cmd), vec!["gemini"], "no resume_argv → bare launch");
    }

    // autoApprove "everything" appends the adapter's bypass flag AFTER the normal
    // argv — on the fresh launch AND on a resume (the policy belongs to the run,
    // not the entry path). "off"/"allowlist" leave the argv untouched.
    #[test]
    fn tool_command_appends_auto_approve_flag_for_everything() {
        let cmd = tool_command("claude", "fix the bug", "", None, true, None, "everything");
        assert_eq!(
            argv_of(&cmd),
            vec!["claude", "fix the bug", "--dangerously-skip-permissions"]
        );
        let cmd = tool_command(
            "claude",
            "fix the bug",
            "",
            None,
            true,
            Some("sess-123"),
            "everything",
        );
        assert_eq!(
            argv_of(&cmd),
            vec![
                "claude",
                "--resume",
                "sess-123",
                "--dangerously-skip-permissions"
            ],
            "resume launches honor the policy too"
        );
    }

    #[test]
    fn tool_command_off_and_allowlist_add_no_flags() {
        for policy in ["off", "allowlist"] {
            let cmd = tool_command("claude", "fix the bug", "", None, true, None, policy);
            assert_eq!(
                argv_of(&cmd),
                vec!["claude", "fix the bug"],
                "policy '{policy}' must not alter the launch"
            );
        }
        // gemini has no bypass wired — "everything" is honestly a no-op for it.
        let cmd = tool_command("gemini", "t", "", None, false, None, "everything");
        assert_eq!(argv_of(&cmd), vec!["gemini"]);
    }

    // The selected MODEL is forwarded as `--model <model>` after the prompt, for
    // every tool (claude/codex/cursor/gemini all take `--model`). This is the bug
    // fix: the picked model used to be dropped and every agent ran on the default.
    #[test]
    fn tool_command_forwards_model_for_each_tool() {
        // (tool id, launch binary, model) — cursor's binary is `cursor-agent`.
        let cases: &[(&str, &str, &str)] = &[
            ("claude", "claude", "opus"),
            ("codex", "codex", "gpt-5.1-codex"),
            ("cursor", "cursor-agent", "composer-1"),
            ("gemini", "gemini", "gemini-2.5-pro"),
        ];
        for (tool, bin, model) in cases {
            let cmd = tool_command(tool, "t", model, None, true, None, "off");
            assert_eq!(
                argv_of(&cmd),
                vec![*bin, "t", "--model", model],
                "{tool} must launch with --model {model}"
            );
        }
    }

    // An empty model (record never carried one) forwards nothing — the agent runs
    // on the tool's own default.
    #[test]
    fn tool_command_empty_model_omits_model_flag() {
        let cmd = tool_command("claude", "t", "", None, true, None, "off");
        assert_eq!(argv_of(&cmd), vec!["claude", "t"]);
    }

    // Codex is the one tool with a reasoning-effort knob: effort is forwarded as a
    // TOML config override AFTER the model. Tools without an effort flag
    // (claude/cursor/gemini) silently ignore the effort value.
    #[test]
    fn tool_command_codex_forwards_model_and_effort() {
        let cmd = tool_command("codex", "t", "gpt-5.1-codex", Some("high"), true, None, "off");
        assert_eq!(
            argv_of(&cmd),
            vec![
                "codex",
                "t",
                "--model",
                "gpt-5.1-codex",
                "--config",
                "model_reasoning_effort=high"
            ]
        );
    }

    #[test]
    fn tool_command_effort_ignored_for_tools_without_an_effort_flag() {
        // claude has `effort: false` in the catalog — even if an effort slips
        // through, it adds nothing (only --model is forwarded).
        let cmd = tool_command("claude", "t", "opus", Some("high"), true, None, "off");
        assert_eq!(argv_of(&cmd), vec!["claude", "t", "--model", "opus"]);
    }

    // Model + effort are part of the run, so they ride a resume launch too (after
    // the resume args + any approve flag).
    #[test]
    fn tool_command_forwards_model_on_resume() {
        let cmd = tool_command("claude", "t", "opus", None, true, Some("sess-1"), "off");
        assert_eq!(
            argv_of(&cmd),
            vec!["claude", "--resume", "sess-1", "--model", "opus"]
        );
    }

    #[test]
    fn stop_removes_entry_and_clears_is_running() {
        let svc = RuntimeService::new();
        let id = Uuid::new_v4();

        // Simulate a running proc by inserting a sentinel killer (no real child).
        svc.procs.lock().unwrap().insert(id, fake_proc());
        assert!(svc.is_running(id), "should be running once inserted");

        svc.stop(id);
        assert!(
            !svc.is_running(id),
            "stop() must remove the entry so the agent is restartable"
        );
    }

    #[test]
    fn any_running_tracks_the_proc_map() {
        let svc = RuntimeService::new();
        assert!(!svc.any_running(), "empty map → nothing running");
        let id = Uuid::new_v4();
        svc.procs.lock().unwrap().insert(id, fake_proc());
        assert!(svc.any_running(), "one live proc → running");
        svc.stop(id);
        assert!(!svc.any_running(), "stop drains the map");
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
        proc_with(pair.master, writer)
    }

    /// Like `fake_proc`, but with an injected stdin writer — lets tests stand in
    /// a writer that blocks, mimicking a wedged ConPTY pipe.
    fn fake_proc_with_writer(writer: Box<dyn Write + Send>) -> Proc {
        let pair = native_pty_system()
            .openpty(PtySize::default())
            .expect("open pty");
        proc_with(pair.master, writer)
    }

    fn proc_with(
        master: Box<dyn portable_pty::MasterPty + Send>,
        writer: Box<dyn Write + Send>,
    ) -> Proc {
        Proc {
            killer: Box::new(NoopKiller),
            pid: None,
            master: Arc::new(Mutex::new(master)),
            writer: Arc::new(Mutex::new(writer)),
        }
    }

    /// A stdin writer that parks inside `write()` until the test releases it —
    /// stands in for a wedged ConPTY pipe. `entered` fires once the writer is
    /// actually blocked (i.e. the per-agent write is mid-I/O). If the test
    /// panics first, dropping `release_tx` unparks it so threads unwind.
    struct BlockingWriter {
        entered: std::sync::mpsc::Sender<()>,
        release: std::sync::mpsc::Receiver<()>,
    }
    impl Write for BlockingWriter {
        fn write(&mut self, buf: &[u8]) -> std::io::Result<usize> {
            let _ = self.entered.send(());
            let _ = self.release.recv(); // parked until released (or sender dropped)
            Ok(buf.len())
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }

    /// Service with one fake agent whose stdin writer parks until released.
    /// Returns (svc, id, entered_rx, release_tx).
    fn blocked_write_fixture() -> (
        Arc<RuntimeService>,
        Uuid,
        std::sync::mpsc::Receiver<()>,
        std::sync::mpsc::Sender<()>,
    ) {
        let svc = Arc::new(RuntimeService::new());
        let id = Uuid::new_v4();
        let (entered_tx, entered_rx) = std::sync::mpsc::channel();
        let (release_tx, release_rx) = std::sync::mpsc::channel();
        svc.procs.lock().unwrap().insert(
            id,
            fake_proc_with_writer(Box::new(BlockingWriter {
                entered: entered_tx,
                release: release_rx,
            })),
        );
        (svc, id, entered_rx, release_tx)
    }

    // THE B6 hazard: a wedged ConPTY write on ONE agent must not stall the procs
    // map for every agent (agent_input/agent_resize/is_running/pids). Park a
    // write inside the injected blocking writer, then prove map ops still
    // answer. The map ops run on a helper thread behind a timeout so a
    // regression (map guard held across I/O) fails the test instead of hanging it.
    #[test]
    fn map_ops_stay_responsive_while_one_agents_write_blocks() {
        let (svc, id, entered_rx, release_tx) = blocked_write_fixture();

        let svc_w = Arc::clone(&svc);
        let write_thread = std::thread::spawn(move || svc_w.write(id, "stuck"));
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("write never reached the writer");

        let svc_m = Arc::clone(&svc);
        let (done_tx, done_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            let _ = done_tx.send((svc_m.is_running(id), svc_m.any_running(), svc_m.pids()));
        });
        let (running, any, pids) = done_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("map ops stalled behind a blocked per-agent write");
        assert!(running, "agent stays in the map while its write is stuck");
        assert!(any);
        assert!(pids.is_empty(), "fake proc has no captured pid");

        release_tx.send(()).expect("release the parked writer");
        write_thread
            .join()
            .unwrap()
            .expect("the parked write completes once released");
    }

    // Exit/stop soundness during in-flight I/O: stop() removes the entry while a
    // write still holds the per-agent writer. The Arc keeps the handle alive so
    // the in-flight write completes; a SUBSEQUENT write errors exactly like a
    // never-started agent.
    #[test]
    fn stop_during_blocked_write_is_sound_and_later_write_errors() {
        let (svc, id, entered_rx, release_tx) = blocked_write_fixture();

        let svc_w = Arc::clone(&svc);
        let write_thread = std::thread::spawn(move || svc_w.write(id, "stuck"));
        entered_rx
            .recv_timeout(std::time::Duration::from_secs(5))
            .expect("write never reached the writer");

        // stop() takes only the map lock + signals the killer — it must never
        // need the per-agent writer lock. Timeout so a deadlock fails fast.
        let svc_s = Arc::clone(&svc);
        let (stopped_tx, stopped_rx) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            svc_s.stop(id);
            let _ = stopped_tx.send(());
        });
        stopped_rx
            .recv_timeout(std::time::Duration::from_secs(2))
            .expect("stop() deadlocked against the blocked write");
        assert!(!svc.is_running(id), "stop removed the entry");

        release_tx.send(()).expect("release the parked writer");
        write_thread
            .join()
            .unwrap()
            .expect("in-flight write finishes against the Arc-kept writer");

        assert_eq!(
            svc.write(id, "after"),
            Err("process not running".into()),
            "post-stop writes fail like an unknown agent"
        );
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

    // The 193 fix: when `claude` is installed by both npm (extensionless Node shim +
    // .cmd/.ps1) and winget (real .exe), the real .exe MUST win even though its PATH
    // dir comes LATER and a bare `claude` sits in an earlier dir. This is the exact
    // shadowing that handed portable-pty a shell script and produced os error 193.
    #[cfg(windows)]
    #[test]
    fn find_program_prefers_real_exe_over_npm_shims() {
        use std::fs;
        use std::path::PathBuf;
        let npm = tempfile::tempdir().unwrap();
        let winget = tempfile::tempdir().unwrap();
        // npm dir (earlier on PATH): the extensionless shim + the .cmd/.ps1 shims.
        fs::write(npm.path().join("claude"), b"#!/bin/sh\n").unwrap();
        fs::write(npm.path().join("claude.cmd"), b"@echo off\n").unwrap();
        fs::write(npm.path().join("claude.ps1"), b"").unwrap();
        // winget dir (later on PATH): the real executable.
        fs::write(winget.path().join("claude.exe"), b"MZ").unwrap();
        let dirs: Vec<PathBuf> = vec![npm.path().into(), winget.path().into()];
        assert_eq!(
            find_program("claude", &dirs),
            Some(winget.path().join("claude.exe")),
            "real .exe beats the bare npm shim in an earlier dir"
        );
    }

    // No real .exe anywhere → the .cmd shim is chosen over the extensionless file
    // (CreateProcessW can't run either directly, but a .cmd we can wrap via cmd.exe).
    #[cfg(windows)]
    #[test]
    fn find_program_falls_back_to_cmd_over_extensionless() {
        use std::fs;
        use std::path::PathBuf;
        let npm = tempfile::tempdir().unwrap();
        fs::write(npm.path().join("claude"), b"#!/bin/sh\n").unwrap();
        fs::write(npm.path().join("claude.cmd"), b"@echo off\n").unwrap();
        let dirs: Vec<PathBuf> = vec![npm.path().into()];
        assert_eq!(
            find_program("claude", &dirs),
            Some(npm.path().join("claude.cmd")),
            ".cmd shim beats the extensionless file"
        );
    }

    // A real .exe launches directly (args preserved); .cmd/.bat route through
    // cmd.exe /c; .ps1 through PowerShell -File. This is what keeps CreateProcessW
    // from ever receiving a non-PE program.
    #[cfg(windows)]
    #[test]
    fn wrap_for_ext_runs_exe_directly_and_wraps_scripts() {
        use std::path::Path;
        assert_eq!(
            wrap_for_ext(
                Path::new(r"C:\winget\claude.exe"),
                &["--resume".into(), "id".into()]
            ),
            vec![
                r"C:\winget\claude.exe".to_string(),
                "--resume".into(),
                "id".into()
            ]
        );
        // `call` is prefixed so cmd.exe does not strip the quotes around a spaced
        // shim path + spaced prompt (the C:\Program Files\nodejs case).
        assert_eq!(
            wrap_for_ext(
                Path::new(r"C:\Program Files\nodejs\claude.cmd"),
                &["fix bug".into()]
            ),
            vec![
                "cmd.exe".to_string(),
                "/c".into(),
                "call".into(),
                r"C:\Program Files\nodejs\claude.cmd".into(),
                "fix bug".into()
            ]
        );
        assert_eq!(
            wrap_for_ext(Path::new(r"C:\npm\claude.ps1"), &[]),
            vec![
                "powershell.exe".to_string(),
                "-NoProfile".into(),
                "-ExecutionPolicy".into(),
                "Bypass".into(),
                "-File".into(),
                r"C:\npm\claude.ps1".into()
            ]
        );
    }

    // An argv[0] that is already a path is taken as-is — we don't second-guess an
    // explicit absolute/relative program the caller chose.
    #[cfg(windows)]
    #[test]
    fn resolve_program_passes_through_explicit_paths() {
        let argv = vec![r"C:\custom\claude.exe".to_string(), "task".into()];
        assert_eq!(resolve_program(argv.clone()), argv);
    }
}
