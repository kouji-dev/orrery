use std::collections::HashMap;
use std::io::{Read, Write};
use std::path::Path;
use std::sync::Mutex;

use portable_pty::{native_pty_system, CommandBuilder, PtySize};
use tauri::{AppHandle, Emitter, Runtime};
use uuid::Uuid;

use crate::agents::model::Agent;

/// Runs agent CLI tools in their worktrees over a PTY and streams their output.
pub struct RuntimeService {
    procs: Mutex<HashMap<Uuid, Proc>>,
}

struct Proc {
    child: Box<dyn portable_pty::Child + Send + Sync>,
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
            procs: Mutex::new(HashMap::new()),
        }
    }

    pub fn is_running(&self, id: Uuid) -> bool {
        self.procs.lock().unwrap().contains_key(&id)
    }

    /// Launch the agent's tool in its worktree over a PTY, streaming output as
    /// `agent://output` events. The initial task prompt is passed only on the
    /// first launch (`send_prompt`); resumes start a bare interactive session.
    pub fn start<R: Runtime>(
        &self,
        app: AppHandle<R>,
        agent: &Agent,
        rows: u16,
        cols: u16,
        send_prompt: bool,
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

        let mut cmd = tool_command(&agent.tool, &agent.task, send_prompt);
        cmd.cwd(worktree);

        let child = pair.slave.spawn_command(cmd).map_err(|e| e.to_string())?;
        drop(pair.slave);
        let mut reader = pair.master.try_clone_reader().map_err(|e| e.to_string())?;
        let writer = pair.master.take_writer().map_err(|e| e.to_string())?;

        // stream stdout/stderr → events on a background thread
        let app2 = app.clone();
        let agent_id = id.to_string();
        std::thread::spawn(move || {
            let mut buf = [0u8; 4096];
            loop {
                match reader.read(&mut buf) {
                    Ok(0) | Err(_) => break,
                    Ok(n) => {
                        let chunk = String::from_utf8_lossy(&buf[..n]).to_string();
                        let _ = app2.emit(
                            "agent://output",
                            serde_json::json!({ "id": agent_id, "chunk": chunk }),
                        );
                    }
                }
            }
            let _ = app2.emit("agent://exit", serde_json::json!({ "id": agent_id }));
        });

        self.procs.lock().unwrap().insert(
            id,
            Proc {
                child,
                master: pair.master,
                writer: Mutex::new(writer),
            },
        );
        Ok(())
    }

    pub fn stop(&self, id: Uuid) {
        if let Some(mut p) = self.procs.lock().unwrap().remove(&id) {
            let _ = p.child.kill();
        }
    }

    /// Kill every running PTY process (called on app shutdown so none orphan).
    pub fn stop_all(&self) {
        let mut procs = self.procs.lock().unwrap();
        for (_, mut p) in procs.drain() {
            let _ = p.child.kill();
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

/// Map a tool id → its CLI invocation. The task prompt is appended only on the
/// first launch; resumes open the tool bare. NOTE: exact flags need live tuning.
fn tool_command(tool: &str, task: &str, send_prompt: bool) -> CommandBuilder {
    let mut c = match tool {
        "claude" => CommandBuilder::new("claude"),
        "codex" => CommandBuilder::new("codex"),
        "cursor" => CommandBuilder::new("cursor-agent"),
        "gemini" => CommandBuilder::new("gemini"),
        other => CommandBuilder::new(other),
    };
    if send_prompt && !task.is_empty() {
        c.arg(task);
    }
    c
}
