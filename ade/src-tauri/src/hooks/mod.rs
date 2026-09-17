//! The hook bridge — the loopback HTTP server that the per-agent `orrery hook`
//! CLI calls into. The agents' native hooks fire it on status changes (working /
//! idle) and when the agent needs the user (permission / question); the bridge
//! relays those to the UI as events. This is the legit, hook-driven rebirth of
//! the removed `kat`/ipc. See docs/specs/2026-06-05-agent-hooks-permissions.md.
//!
//! Fire-and-forget (like stablyai/orca): every hook POST is acknowledged
//! immediately (no body), so the agent NEVER blocks on orrery — it keeps using
//! its own permission flow (you approve in its terminal). The remote allow/deny
//! round-trip is intentionally deferred; if we add it later, this is where the
//! held-request registry would live.

pub mod protocol;
pub mod transcript;

use std::collections::HashMap;
use std::io::{BufReader, Write};
use std::net::{TcpListener, TcpStream};
use std::path::PathBuf;
use std::sync::{Arc, Mutex};

use tauri::{AppHandle, Manager, Runtime};

use protocol::{parse, read_request, AgentEvent, HookEnvelope};
use transcript::latest_content;

/// Sink for events bound for the frontend — `app.emit` in production, a probe in
/// tests (keeps the server testable without a Tauri runtime).
type Emit = Arc<dyn Fn(&str, serde_json::Value) + Send + Sync>;

/// Per-agent last-emitted activity detail. Shared across all connections so we can
/// collapse the flood of identical-content hooks (the transcript re-read returns
/// the same latest message across many hooks) down to a single agent://activity
/// emit per distinct detail. Keyed by agentId.
type LastActivity = Arc<Mutex<HashMap<String, String>>>;

/// Per-agent last-emitted status state ("working" / "idle"). Shared across all
/// connections so consecutive same-state pings collapse: a flood of Working hook
/// events emits agent://status "working" only ONCE (on the transition), and an
/// "idle" after a "working" still emits (the state changed). Keyed by agentId.
/// The frontend persists hookState per agent (set once, stays until the next
/// ping), so a single "working" keeps the agent working until "idle" arrives —
/// deduping is safe. Keeps status emissions low-volume to match agent://activity.
type LastStatus = Arc<Mutex<HashMap<String, String>>>;

/// Sink for persisting a captured CLI session id — `(agent_id, session_id)`. In
/// production (`start`) this writes the DB and re-emits the agent as
/// `agent://updated`; tests pass a probe. Lets the bridge OWN session capture
/// without hard-wiring Tauri state into the testable `handle`.
type OnSession = Arc<dyn Fn(&str, &str) + Send + Sync>;

/// Per-agent last-seen CLI session id. A session id is stable for a whole run but
/// rides on EVERY hook, so we dedup on change: the hundreds of hooks in a session
/// collapse to a SINGLE persist + `agent://updated` emit. Keyed by agentId.
type LastSession = Arc<Mutex<HashMap<String, String>>>;

/// Managed Tauri state: the running loopback server. Holds the chosen port + the
/// shared token so the agent's hook can call back and be authenticated.
pub struct HookBridge {
    port: u16,
    token: String,
}

impl HookBridge {
    /// Bind a loopback server on an ephemeral port and start accepting hook
    /// callbacks. Each connection is handled on its own thread.
    pub fn start<R: Runtime>(app: AppHandle<R>) -> std::io::Result<Self> {
        let emit_app = app.clone();
        let emit: Emit = Arc::new(move |name: &str, payload: serde_json::Value| {
            let _ = crate::core::emit::emit_tracked(&emit_app, name, &payload);
        });
        // Persist a captured CLI session id DIRECTLY (the bridge owns this — no
        // frontend round-trip). After storing, re-emit the refreshed agent as
        // `agent://updated` so the UI learns a session exists (enabling "Continue
        // session") through the same channel as every other agent mutation.
        // Best-effort: a missing service / unparsable id / db error is ignored.
        let session_app = app.clone();
        let on_session: OnSession = Arc::new(move |agent_id: &str, session_id: &str| {
            use crate::agents::service::AgentService;
            use crate::core::events::{emit_entity, Change};
            let Ok(id) = uuid::Uuid::parse_str(agent_id) else {
                return;
            };
            let Some(svc) = session_app.try_state::<AgentService>() else {
                return;
            };
            if svc.set_session(id, session_id).is_err() {
                return;
            }
            if let Ok(agent) = svc.get(id) {
                emit_entity(&session_app, "agent", Change::Updated, agent);
            }
        });
        Self::serve_with_session(emit, on_session)
    }

    /// Bind + serve with an event sink only — session capture is a no-op. Kept for
    /// the tests that don't exercise session persistence.
    fn serve(emit: Emit) -> std::io::Result<Self> {
        Self::serve_with_session(emit, Arc::new(|_, _| {}))
    }

    /// Bind + serve with an event sink + a session-persist sink (production passes
    /// `app.emit` + the DB-writing closure built in `start`).
    fn serve_with_session(emit: Emit, on_session: OnSession) -> std::io::Result<Self> {
        let listener = TcpListener::bind("127.0.0.1:0")?;
        let port = listener.local_addr()?.port();
        let token = uuid::Uuid::new_v4().to_string();

        // Shared dedup state, threaded into every connection (cloned per-conn like
        // `emit`/`token`) so repeated identical activity collapses to one emit,
        // consecutive same-state status pings collapse to a single emit, and a
        // session id (which rides every hook) persists + emits only once per change.
        let last_activity: LastActivity = Arc::new(Mutex::new(HashMap::new()));
        let last_status: LastStatus = Arc::new(Mutex::new(HashMap::new()));
        let last_session: LastSession = Arc::new(Mutex::new(HashMap::new()));

        let token_l = token.clone();
        std::thread::spawn(move || {
            for stream in listener.incoming().flatten() {
                let emit = emit.clone();
                let on_session = on_session.clone();
                let token = token_l.clone();
                let last_activity = last_activity.clone();
                let last_status = last_status.clone();
                let last_session = last_session.clone();
                std::thread::spawn(move || {
                    handle(
                        stream,
                        &emit,
                        &on_session,
                        &token,
                        &last_activity,
                        &last_status,
                        &last_session,
                    )
                });
            }
        });

        log::info!("hook bridge listening on 127.0.0.1:{port}");
        Ok(Self { port, token })
    }

    pub fn endpoint(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }
    pub fn token(&self) -> &str {
        &self.token
    }
}

/// The executable agents invoke for their hooks — the orrery app itself, re-run
/// as `orrery hook --event <EVENT>` (see cli::hook). Always co-located with the
/// running app, so no separate sidecar to bundle. `None` only if the OS can't
/// report our own path, in which case the agent launches without hooks (PTY
/// parsing fallback).
pub fn hook_binary() -> Option<PathBuf> {
    std::env::current_exe().ok()
}

fn handle(
    stream: TcpStream,
    emit: &Emit,
    on_session: &OnSession,
    token: &str,
    last_activity: &LastActivity,
    last_status: &LastStatus,
    last_session: &LastSession,
) {
    let mut reader = BufReader::new(match stream.try_clone() {
        Ok(s) => s,
        Err(_) => return,
    });
    let mut out = stream;

    let req = match read_request(&mut reader) {
        Ok(r) => r,
        Err(_) => return,
    };
    if req.auth.as_deref() != Some(token) {
        let _ = write_response(&mut out, 401);
        return;
    }
    let env: HookEnvelope = match serde_json::from_str(&req.body) {
        Ok(e) => e,
        Err(_) => {
            let _ = write_response(&mut out, 204);
            return;
        }
    };

    // Parse the raw hook into the structured cross-agent taxonomy. Every emit
    // decision below branches on the `AgentEvent` variant, not the raw hook name.
    let event = parse(&env);

    // Capture the agent's CLI session id when the hook payload carries one (claude
    // includes `session_id` on every hook). The bridge persists it DIRECTLY via
    // `on_session` (which stores it + re-emits `agent://updated`) — no frontend
    // round-trip. Deduped on change: the session id is stable for a whole run but
    // rides every hook, so this collapses to ONE persist + emit per agent/session.
    if let Some(session_id) = env
        .payload
        .get("session_id")
        .and_then(serde_json::Value::as_str)
    {
        let mut map = last_session.lock().unwrap();
        if map.get(&env.agent_id).map(String::as_str) != Some(session_id) {
            map.insert(env.agent_id.clone(), session_id.to_string());
            drop(map);
            on_session(&env.agent_id, session_id);
        }
    }

    // Verification log: for the two events the user can't otherwise inspect — the
    // permission ask + the notification — dump the COMPACT raw payload (truncated)
    // so the orrery log shows EXACTLY what each agent sent. This is how we confirm
    // which keys an agent really uses (e.g. AskUserQuestion's questions[]) without
    // guessing. Cheap: only fires for these two low-volume event variants.
    if matches!(
        event,
        AgentEvent::PermissionRequest { .. } | AgentEvent::Notification { .. }
    ) {
        let raw = env.payload.to_string();
        let raw: String = if raw.chars().count() > 500 {
            format!("{}…", raw.chars().take(499).collect::<String>())
        } else {
            raw
        };
        log::debug!(
            "hook raw payload: event={} agent={} tool={} payload={}",
            env.event,
            env.agent_id,
            env.tool,
            raw
        );
    }

    // Mirror the terminal: prefer the REAL latest message content scraped from the
    // agent's transcript (claude's `transcript_path`), so the card shows assistant
    // prose / thinking / tool use — exactly what the user sees. Falls back to the
    // variant's own structured `activity_detail()` for agents/events that carry no
    // transcript (gemini, cursor's own hook shapes, …). Computed once and reused by
    // every arm so the preview reflects the LATEST state regardless of which hook
    // fired.
    let content = latest_content(&env.payload);

    // fire-and-forget: relay to the UI, acknowledge immediately (204). The agent
    // is never held — it proceeds with its own flow. Every meaningful arm ALSO
    // refreshes the preview (agent://activity) so the card is a live state mirror,
    // not frozen on the last tool call while the agent waits. We compute the
    // activity detail (if any) per variant, then emit it once below.
    //
    // Activity-line rule: PREFER transcript content (richer); fall back to the
    // variant's structured `activity_detail()`. PermissionRequest is special — its
    // ✋ line must reflect the REQUEST (not whatever the transcript last showed), so
    // it ignores transcript content.
    let activity: Option<String> = match &event {
        // The dedicated permission signal carries the FULL structured detail. This
        // is the NEW contract for the frontend phase — see the payload doc on
        // `emit_permission` below. We ALSO refresh the preview with the ✋ line.
        AgentEvent::PermissionRequest {
            tool,
            input,
            mode,
            suggestions,
        } => {
            emit_permission(
                emit,
                &env.agent_id,
                tool,
                input,
                mode.as_deref(),
                suggestions,
            );
            event.activity_detail()
        }

        // Streamed assistant output / tool lifecycle / notifications / prompts: a
        // working-ish status plus an activity line. Transcript content wins when
        // present (it's the live mirror); else the variant's structured line.
        AgentEvent::AgentMessage { .. }
        | AgentEvent::ToolStart { .. }
        | AgentEvent::ToolEnd { .. }
        | AgentEvent::Notification { .. }
        | AgentEvent::UserPrompt { .. }
        | AgentEvent::SessionStart { .. } => {
            emit_status(emit, last_status, &env.agent_id, "working");
            content.clone().or_else(|| event.activity_detail())
        }

        // Turn finished → idle, and refresh the preview to a terminal state ("✓ done"
        // or the latest transcript content) so the card stops mirroring the last
        // tool call.
        AgentEvent::TurnEnd => {
            emit_status(emit, last_status, &env.agent_id, "idle");
            content.clone().or_else(|| event.activity_detail())
        }

        // Session ended → idle, no preview line (the card keeps its prior state).
        AgentEvent::SessionEnd { .. } => {
            emit_status(emit, last_status, &env.agent_id, "idle");
            None
        }

        // An error surfaces in the feed but doesn't move the working/idle status.
        AgentEvent::Error { .. } => event.activity_detail(),

        // Compaction / unmapped events: acknowledged, no emit.
        AgentEvent::Compact { .. } | AgentEvent::Unknown => None,
    };

    // Dedup + non-empty gate: emit agent://activity ONLY when the detail is
    // non-empty AND differs from the last detail emitted for this agent. The
    // transcript re-read returns the same latest message across many consecutive
    // hooks, so without this the bridge floods the UI with 1000+ identical msgs;
    // this collapses each run of identical content to a single emit. The payload
    // also carries the precise hook `event` (so the frontend can log/branch on it)
    // AND a semantic `kind` (user/agent/tool/success/error/question/info) so the
    // feed can color each entry without re-parsing the detail. The dedup key stays
    // the detail string only — `kind` is derived from the same event, so it never
    // changes independently. (permission/status emits above are low-volume, kept as-is.)
    let kind = event.kind();
    let activity_emitted = match activity {
        Some(d) if !d.trim().is_empty() => {
            let mut map = last_activity.lock().unwrap();
            let changed = map.get(&env.agent_id).map(String::as_str) != Some(d.as_str());
            if changed {
                map.insert(env.agent_id.clone(), d.clone());
                drop(map);
                emit(
                    "agent://activity",
                    serde_json::json!({
                        "agentId": env.agent_id,
                        "tool": env.tool,
                        "event": env.event,
                        "kind": kind,
                        "detail": d,
                    }),
                );
            }
            changed
        }
        _ => false,
    };

    log::info!(
        "hook recv: event={} agent={} transcript={} -> activity={}",
        env.event,
        env.agent_id,
        content.is_some(),
        activity_emitted
    );

    let _ = write_response(&mut out, 204);
}

/// Emit the dedicated `agent://permission` signal with the FULL structured request
/// detail. THIS IS THE NEW FRONTEND CONTRACT (for the later frontend phase) — the
/// payload shape is:
///
/// ```jsonc
/// {
///   "agentId": "a1",            // which agent is asking
///   "tool":    "Bash",          // the tool name (PostToolUse-style tool_name, or the agent label)
///   "mode":    "default",       // permission_mode, or null
///   "summary": "git push",      // human one-liner of WHAT is being asked (ToolInput::summary):
///                               //   command/file_path/description when present, else the real
///                               //   AskUserQuestion question, else the most salient input field.
///                               //   The frontend can show this directly as the card title. May be null.
///   "command":     "git push",  // tool_input.command, or null
///   "description": "…",         // tool_input.description, or null
///   "filePath":    "src/foo.rs",// tool_input.file_path, or null
///   "questions": [              // ONLY for the AskUserQuestion tool — the parsed clarifying
///                               //   questions so the frontend can render a real choice card.
///                               //   Empty/omitted for ordinary tool permission asks. Each
///                               //   option carries BOTH its label AND its description blurb;
///                               //   `multiSelect` says whether many options may be picked.
///     { "question": "Which DB?", "header": "Database", "multiSelect": false, "options": [
///         { "label": "Postgres", "description": "Relational, ACID" },
///         { "label": "MySQL",    "description": "Relational, widely hosted" }
///     ] }
///   ],
///   "suggestions": [            // flattened permission_suggestions (empty when absent)
///     { "behavior": "allow", "rule": "Bash(git push:*)", "description": "Always allow git push" }
///   ]
/// }
/// ```
///
/// The frontend renders an approve/deny card from this; `summary` is the headline,
/// `questions` lets it render AskUserQuestion as a multiple-choice card, and
/// `suggestions` are the "always allow/deny" rule options. (Today this is still
/// fire-and-forget — the remote allow/deny round-trip is deferred — so the agent
/// is not held.)
fn emit_permission(
    emit: &Emit,
    agent_id: &str,
    tool: &str,
    input: &protocol::ToolInput,
    mode: Option<&str>,
    suggestions: &[protocol::Suggestion],
) {
    let suggestions: Vec<serde_json::Value> = suggestions
        .iter()
        .map(|s| {
            serde_json::json!({
                "behavior": s.behavior,
                "rule": s.rule,
                "description": s.description,
            })
        })
        .collect();
    // The parsed AskUserQuestion choices (empty for ordinary tool asks) so the
    // frontend can list the actual question + options, not just the tool name. Each
    // option carries BOTH its label (what the user picks) and its description blurb.
    let questions: Vec<serde_json::Value> = input
        .questions()
        .iter()
        .map(|q| {
            let options: Vec<serde_json::Value> = q
                .options
                .iter()
                .map(|o| {
                    serde_json::json!({
                        "label": o.label,
                        "description": o.description,
                    })
                })
                .collect();
            serde_json::json!({
                "question": q.question,
                "header": q.header,
                "options": options,
                // Claude's multiSelect flag — lets the frontend render checkboxes
                // (multi) vs radios (single). Default false (single-select).
                "multiSelect": q.multi_select,
            })
        })
        .collect();
    emit(
        "agent://permission",
        serde_json::json!({
            "agentId": agent_id,
            "tool": tool,
            "mode": mode,
            "summary": input.summary(),
            "command": input.command,
            "description": input.description,
            "filePath": input.file_path,
            "questions": questions,
            "suggestions": suggestions,
        }),
    );
}

/// Emit `agent://status` ONLY on a state CHANGE for this agent. The bridge receives
/// a flood of Working hook events per turn; without this gate it would re-emit
/// "working" on each one. We keep a per-agent last-emitted state and skip the emit
/// when it's unchanged, then record the new state. Consecutive "working" pings thus
/// collapse to one; an "idle" after a "working" still emits (the state differs).
/// Safe because the frontend persists hookState per agent until the next ping.
fn emit_status(emit: &Emit, last_status: &LastStatus, agent_id: &str, state: &str) {
    {
        let mut map = last_status.lock().unwrap();
        if map.get(agent_id).map(String::as_str) == Some(state) {
            return; // same state as last emitted → collapse (no re-emit).
        }
        map.insert(agent_id.to_string(), state.to_string());
    }
    emit(
        "agent://status",
        serde_json::json!({ "id": agent_id, "state": state }),
    );
}

fn write_response(out: &mut TcpStream, status: u16) -> std::io::Result<()> {
    let reason = match status {
        204 => "No Content",
        401 => "Unauthorized",
        _ => "OK",
    };
    let resp =
        format!("HTTP/1.1 {status} {reason}\r\nContent-Length: 0\r\nConnection: close\r\n\r\n");
    out.write_all(resp.as_bytes())?;
    out.flush()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Read;
    use std::sync::mpsc;
    use std::time::Duration;

    /// Minimal `orrery hook`-style POST: returns (status_line, body).
    fn post(endpoint: &str, token: &str, body: &str) -> (String, String) {
        let addr = endpoint.trim_start_matches("http://");
        let mut s = TcpStream::connect(addr).unwrap();
        let req = format!(
            "POST / HTTP/1.1\r\nHost: {addr}\r\nAuthorization: Bearer {token}\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
            body.len()
        );
        s.write_all(req.as_bytes()).unwrap();
        s.flush().unwrap();
        let mut resp = String::new();
        s.read_to_string(&mut resp).unwrap();
        let status = resp.lines().next().unwrap_or("").to_string();
        let body = resp.split("\r\n\r\n").nth(1).unwrap_or("").to_string();
        (status, body)
    }

    // A PermissionRequest hook emits agent://permission with the FULL structured
    // detail (the new frontend contract: tool, mode, command/description/filePath,
    // and flattened suggestions) and returns immediately (204), never holding the
    // agent. It ALSO refreshes the preview (agent://activity) with a ✋ line so the
    // card mirrors the waiting state instead of freezing on the last tool call.
    #[test]
    fn permission_request_emits_full_detail_and_activity_and_acks_immediately() {
        let (tx, rx) = mpsc::channel::<(String, serde_json::Value)>();
        let probe = Arc::new(move |name: &str, payload: serde_json::Value| {
            if name == "agent://permission" || name == "agent://activity" {
                let _ = tx.send((name.to_string(), payload));
            }
        });
        let bridge = HookBridge::serve(probe).unwrap();
        let (status, body) = post(
            &bridge.endpoint(),
            bridge.token(),
            r#"{"agentId":"a1","tool":"claude","event":"PermissionRequest","payload":{"tool_name":"Bash","tool_input":{"command":"git push"},"permission_mode":"default","permission_suggestions":[{"behavior":"allow","rule":"Bash(git push:*)","description":"Always allow git push"}]}}"#,
        );
        assert!(status.contains("204"), "status: {status}");
        assert_eq!(body, "", "fire-and-forget: no decision body");

        // Collect both emits (order between permission/activity is not guaranteed).
        let mut perm = None;
        let mut act = None;
        for _ in 0..2 {
            let (name, payload) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
            match name.as_str() {
                "agent://permission" => perm = Some(payload),
                "agent://activity" => act = Some(payload),
                _ => {}
            }
        }
        // The permission payload carries the FULL structured detail (new contract).
        let perm = perm.expect("agent://permission emitted");
        assert_eq!(perm["agentId"], "a1");
        assert_eq!(perm["tool"], "Bash");
        assert_eq!(perm["mode"], "default");
        assert_eq!(perm["command"], "git push");
        let sugg = perm["suggestions"].as_array().expect("suggestions array");
        assert_eq!(sugg.len(), 1);
        assert_eq!(sugg[0]["behavior"], "allow");
        assert_eq!(sugg[0]["rule"], "Bash(git push:*)");
        assert_eq!(sugg[0]["description"], "Always allow git push");
        // …and the preview gets the ✋ line built from tool + command.
        assert_eq!(
            act.expect("agent://activity emitted")["detail"].as_str(),
            Some("✋ Bash: git push")
        );
    }

    // An AskUserQuestion permission ask ENRICHES the agent://permission payload so
    // the frontend can render the real ask: `summary` carries the question line and
    // `questions[]` carries the parsed {question, header, options}. The ✋ activity
    // line also shows the question, NOT a bare "✋ AskUserQuestion". (The user's
    // complaint: the actual question lived in the payload but was never surfaced.)
    #[test]
    fn ask_user_question_permission_surfaces_question_and_options() {
        let (tx, rx) = mpsc::channel::<(String, serde_json::Value)>();
        let probe = Arc::new(move |name: &str, payload: serde_json::Value| {
            if name == "agent://permission" || name == "agent://activity" {
                let _ = tx.send((name.to_string(), payload));
            }
        });
        let bridge = HookBridge::serve(probe).unwrap();
        let (status, _) = post(
            &bridge.endpoint(),
            bridge.token(),
            r#"{"agentId":"a1","tool":"claude","event":"PermissionRequest","payload":{"tool_name":"AskUserQuestion","tool_input":{"questions":[{"question":"Which database should I use?","header":"Database","options":[{"label":"Postgres","description":"x"},{"label":"MySQL","description":"y"}],"multiSelect":false},{"question":"Which features?","header":"Features","options":[{"label":"Auth","description":"z"}],"multiSelect":true}]}}}"#,
        );
        assert!(status.contains("204"), "status: {status}");

        let mut perm = None;
        let mut act = None;
        for _ in 0..2 {
            let (name, payload) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
            match name.as_str() {
                "agent://permission" => perm = Some(payload),
                "agent://activity" => act = Some(payload),
                _ => {}
            }
        }
        let perm = perm.expect("agent://permission emitted");
        assert_eq!(perm["tool"], "AskUserQuestion");
        // The new `summary` headline carries the real question (+ header chip).
        assert_eq!(
            perm["summary"],
            "[Database] Which database should I use? (+1 more)"
        );
        // The new `questions[]` carries the parsed ask for a choice card.
        let qs = perm["questions"].as_array().expect("questions array");
        assert_eq!(qs.len(), 2);
        assert_eq!(qs[0]["question"], "Which database should I use?");
        assert_eq!(qs[0]["header"], "Database");
        // Each option is now an object carrying BOTH label AND description.
        let opts = qs[0]["options"].as_array().unwrap();
        assert_eq!(opts.len(), 2);
        assert_eq!(opts[0]["label"], "Postgres");
        assert_eq!(opts[0]["description"], "x");
        assert_eq!(opts[1]["label"], "MySQL");
        assert_eq!(opts[1]["description"], "y");
        // …and each question carries its `multiSelect` flag (single then multi).
        assert_eq!(qs[0]["multiSelect"], false);
        assert_eq!(qs[1]["multiSelect"], true);
        // …and the ✋ preview shows the (first) question, not the bare tool name.
        assert_eq!(
            act.expect("agent://activity emitted")["detail"].as_str(),
            Some("✋ AskUserQuestion: [Database] Which database should I use? (+1 more)")
        );
    }

    // An ordinary (non-AskUserQuestion) permission ask carries an empty questions[]
    // and a summary built from the command — the contract stays backward-compatible.
    #[test]
    fn ordinary_permission_has_summary_and_empty_questions() {
        let (tx, rx) = mpsc::channel::<serde_json::Value>();
        let probe = Arc::new(move |name: &str, payload: serde_json::Value| {
            if name == "agent://permission" {
                let _ = tx.send(payload);
            }
        });
        let bridge = HookBridge::serve(probe).unwrap();
        let (status, _) = post(
            &bridge.endpoint(),
            bridge.token(),
            r#"{"agentId":"a1","tool":"claude","event":"PermissionRequest","payload":{"tool_name":"Bash","tool_input":{"command":"git push"}}}"#,
        );
        assert!(status.contains("204"), "status: {status}");
        let perm = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(perm["summary"], "git push");
        assert!(perm["questions"].as_array().unwrap().is_empty());
    }

    // A Notification surfaces its actual `message` as the activity detail (not a
    // bare "✋ claude"): the message lives in the payload and is now extracted.
    #[test]
    fn notification_surfaces_message_text_in_activity() {
        let (tx, rx) = mpsc::channel::<String>();
        let probe = Arc::new(move |name: &str, payload: serde_json::Value| {
            if name == "agent://activity" {
                let _ = tx.send(payload["detail"].as_str().unwrap().to_string());
            }
        });
        let bridge = HookBridge::serve(probe).unwrap();
        let (status, _) = post(
            &bridge.endpoint(),
            bridge.token(),
            r#"{"agentId":"a1","tool":"claude","event":"Notification","payload":{"notification_type":"idle_prompt","message":"Claude is waiting for your input"}}"#,
        );
        assert!(status.contains("204"), "status: {status}");
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            "Claude is waiting for your input"
        );
    }

    // A Notification with notification_type=permission_prompt is promoted to a
    // permission ask: it emits agent://permission (tool falls back to the agent
    // label, suggestions empty) — NOT a generic notification.
    #[test]
    fn notification_permission_prompt_emits_permission() {
        let (tx, rx) = mpsc::channel::<serde_json::Value>();
        let probe = Arc::new(move |name: &str, payload: serde_json::Value| {
            if name == "agent://permission" {
                let _ = tx.send(payload);
            }
        });
        let bridge = HookBridge::serve(probe).unwrap();
        let (status, _) = post(
            &bridge.endpoint(),
            bridge.token(),
            r#"{"agentId":"a1","tool":"claude","event":"Notification","payload":{"notification_type":"permission_prompt","message":"Claude needs your permission to use Bash"}}"#,
        );
        assert!(status.contains("204"), "status: {status}");
        let perm = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(perm["tool"], "claude");
        assert!(perm["suggestions"].as_array().unwrap().is_empty());
    }

    // A MessageDisplay hook (streamed assistant output) emits agent://activity with
    // the message text as the detail (and a working status ping). No transcript →
    // the AgentMessage{text} drives the line directly.
    #[test]
    fn message_display_emits_activity_with_message_text() {
        let (tx, rx) = mpsc::channel::<String>();
        let probe = Arc::new(move |name: &str, payload: serde_json::Value| {
            if name == "agent://activity" {
                let _ = tx.send(payload["detail"].as_str().unwrap().to_string());
            }
        });
        let bridge = HookBridge::serve(probe).unwrap();
        let (status, _) = post(
            &bridge.endpoint(),
            bridge.token(),
            r#"{"agentId":"a1","tool":"claude","event":"MessageDisplay","payload":{"delta":"refactoring the parser now"}}"#,
        );
        assert!(status.contains("204"), "status: {status}");
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            "refactoring the parser now"
        );
    }

    // A status ping (UserPromptSubmit) emits agent://status and acks immediately.
    #[test]
    fn status_ping_emits_working() {
        let (tx, rx) = mpsc::channel::<String>();
        let probe = Arc::new(move |name: &str, payload: serde_json::Value| {
            if name == "agent://status" {
                let _ = tx.send(payload["state"].as_str().unwrap().to_string());
            }
        });
        let bridge = HookBridge::serve(probe).unwrap();
        let (status, _) = post(
            &bridge.endpoint(),
            bridge.token(),
            r#"{"agentId":"a1","tool":"claude","event":"UserPromptSubmit","payload":null}"#,
        );
        assert!(status.contains("204"));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "working");
    }

    // A PreToolUse hook (ToolStart) emits agent://activity carrying the structured
    // tool detail ("▸ <tool>: <command>") in addition to the working status ping,
    // and acks immediately.
    #[test]
    fn pre_tool_use_emits_activity() {
        let (tx, rx) = mpsc::channel::<String>();
        let probe = Arc::new(move |name: &str, payload: serde_json::Value| {
            if name == "agent://activity" {
                let _ = tx.send(payload["detail"].as_str().unwrap().to_string());
            }
        });
        let bridge = HookBridge::serve(probe).unwrap();
        let (status, _) = post(
            &bridge.endpoint(),
            bridge.token(),
            r#"{"agentId":"a1","tool":"claude","event":"PreToolUse","payload":{"tool_name":"Bash","tool_input":{"command":"npm test"}}}"#,
        );
        assert!(status.contains("204"), "status: {status}");
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            "▸ Bash: npm test"
        );
    }

    // A PostToolUse hook emits agent://activity carrying a RESULT-style detail
    // ("Edit ✓"), distinct from the matching PreToolUse request line, and acks
    // immediately. This is what makes the feed show progress, not just intent.
    #[test]
    fn post_tool_use_emits_result_activity() {
        let (tx, rx) = mpsc::channel::<String>();
        let probe = Arc::new(move |name: &str, payload: serde_json::Value| {
            if name == "agent://activity" {
                let _ = tx.send(payload["detail"].as_str().unwrap().to_string());
            }
        });
        let bridge = HookBridge::serve(probe).unwrap();
        let (status, _) = post(
            &bridge.endpoint(),
            bridge.token(),
            r#"{"agentId":"a1","tool":"claude","event":"PostToolUse","payload":{"tool_name":"Edit","tool_input":{"file_path":"src/foo.rs"},"tool_response":{"filePath":"src/foo.rs","success":true}}}"#,
        );
        assert!(status.contains("204"), "status: {status}");
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "Edit ✓");
    }

    // A Stop hook (turn finished) emits agent://status idle AND refreshes the
    // preview (agent://activity) with "✓ done" when there's no transcript — so the
    // card shows the terminal state instead of freezing on the last tool call.
    #[test]
    fn stop_emits_idle_status_and_done_activity() {
        let (tx, rx) = mpsc::channel::<(String, String)>();
        let probe = Arc::new(move |name: &str, payload: serde_json::Value| match name {
            "agent://status" => {
                let _ = tx.send((
                    name.to_string(),
                    payload["state"].as_str().unwrap().to_string(),
                ));
            }
            "agent://activity" => {
                let _ = tx.send((
                    name.to_string(),
                    payload["detail"].as_str().unwrap().to_string(),
                ));
            }
            _ => {}
        });
        let bridge = HookBridge::serve(probe).unwrap();
        let (status, _) = post(
            &bridge.endpoint(),
            bridge.token(),
            r#"{"agentId":"a1","tool":"claude","event":"Stop","payload":null}"#,
        );
        assert!(status.contains("204"), "status: {status}");

        let mut got_status = None;
        let mut got_activity = None;
        for _ in 0..2 {
            let (name, val) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
            match name.as_str() {
                "agent://status" => got_status = Some(val),
                "agent://activity" => got_activity = Some(val),
                _ => {}
            }
        }
        assert_eq!(got_status.as_deref(), Some("idle"));
        assert_eq!(got_activity.as_deref(), Some("✓ done"));
    }

    // A lifecycle SessionStart pings working AND now seeds the preview with a
    // meaningful fallback ("● started") so a freshly-launched agent's card is not
    // empty until the first tool call — while still raising NO permission.
    #[test]
    fn session_start_pings_working_and_seeds_started_activity() {
        let (tx, rx) = mpsc::channel::<(String, String)>();
        let probe = Arc::new(move |name: &str, payload: serde_json::Value| match name {
            "agent://status" => {
                let _ = tx.send((
                    name.to_string(),
                    payload["state"].as_str().unwrap().to_string(),
                ));
            }
            "agent://activity" => {
                let _ = tx.send((
                    name.to_string(),
                    payload["detail"].as_str().unwrap().to_string(),
                ));
            }
            "agent://permission" => {
                let _ = tx.send((name.to_string(), String::new()));
            }
            _ => {}
        });
        let bridge = HookBridge::serve(probe).unwrap();
        let (status, _) = post(
            &bridge.endpoint(),
            bridge.token(),
            r#"{"agentId":"a1","tool":"claude","event":"SessionStart","payload":{"source":"startup"}}"#,
        );
        assert!(status.contains("204"));

        let mut got_status = None;
        let mut got_activity = None;
        for _ in 0..2 {
            let (name, val) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
            match name.as_str() {
                "agent://status" => got_status = Some(val),
                "agent://activity" => got_activity = Some(val),
                "agent://permission" => panic!("SessionStart must not raise permission"),
                _ => {}
            }
        }
        assert_eq!(got_status.as_deref(), Some("working"));
        assert_eq!(got_activity.as_deref(), Some("● started"));
    }

    // UserPromptSubmit pings working AND now seeds the preview with "working…" when
    // there's no transcript, so the card populates immediately on the first prompt.
    #[test]
    fn user_prompt_submit_seeds_working_activity() {
        let (tx, rx) = mpsc::channel::<String>();
        let probe = Arc::new(move |name: &str, payload: serde_json::Value| {
            if name == "agent://activity" {
                let _ = tx.send(payload["detail"].as_str().unwrap().to_string());
            }
        });
        let bridge = HookBridge::serve(probe).unwrap();
        let (status, _) = post(
            &bridge.endpoint(),
            bridge.token(),
            r#"{"agentId":"a1","tool":"claude","event":"UserPromptSubmit","payload":null}"#,
        );
        assert!(status.contains("204"));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "working…");
    }

    // When the payload carries a transcript_path, activity shows the REAL latest
    // message content pulled from that transcript — not the structured summarize().
    #[test]
    fn activity_uses_transcript_content_when_path_present() {
        use std::io::Write as _;

        let mut tf = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            tf,
            r#"{{"type":"assistant","message":{{"role":"assistant","content":[{{"type":"text","text":"refactoring the parser now"}}]}}}}"#
        )
        .unwrap();
        tf.flush().unwrap();
        let path = tf.path().to_str().unwrap().replace('\\', "\\\\");

        let (tx, rx) = mpsc::channel::<String>();
        let probe = Arc::new(move |name: &str, payload: serde_json::Value| {
            if name == "agent://activity" {
                let _ = tx.send(payload["detail"].as_str().unwrap().to_string());
            }
        });
        let bridge = HookBridge::serve(probe).unwrap();
        // A PostToolUse whose payload includes the transcript_path. Without the
        // transcript wiring this would summarize to "Edit ✓"; with it, the real
        // assistant text wins.
        let body = format!(
            r#"{{"agentId":"a1","tool":"claude","event":"PostToolUse","payload":{{"transcript_path":"{path}","tool_name":"Edit","tool_input":{{"file_path":"src/foo.rs"}},"tool_response":{{"success":true}}}}}}"#
        );
        let (status, _) = post(&bridge.endpoint(), bridge.token(), &body);
        assert!(status.contains("204"), "status: {status}");
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            "refactoring the parser now"
        );
    }

    // Without a transcript_path, activity falls back to the variant's structured
    // activity_detail() ("▸ <tool>: <command>").
    #[test]
    fn activity_falls_back_to_structured_detail_without_transcript() {
        let (tx, rx) = mpsc::channel::<String>();
        let probe = Arc::new(move |name: &str, payload: serde_json::Value| {
            if name == "agent://activity" {
                let _ = tx.send(payload["detail"].as_str().unwrap().to_string());
            }
        });
        let bridge = HookBridge::serve(probe).unwrap();
        let (status, _) = post(
            &bridge.endpoint(),
            bridge.token(),
            r#"{"agentId":"a1","tool":"claude","event":"PreToolUse","payload":{"tool_name":"Bash","tool_input":{"command":"npm test"}}}"#,
        );
        assert!(status.contains("204"), "status: {status}");
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            "▸ Bash: npm test"
        );
    }

    // A turn-start ping (UserPromptSubmit) that carries a transcript_path now feeds
    // the activity feed with the latest message content (broadened emission), where
    // before it was suppressed as status-only.
    #[test]
    fn user_prompt_submit_with_transcript_emits_activity() {
        use std::io::Write as _;

        let mut tf = tempfile::NamedTempFile::new().unwrap();
        writeln!(
            tf,
            r#"{{"type":"assistant","message":{{"content":[{{"type":"thinking","thinking":"planning the change"}}]}}}}"#
        )
        .unwrap();
        tf.flush().unwrap();
        let path = tf.path().to_str().unwrap().replace('\\', "\\\\");

        let (tx, rx) = mpsc::channel::<String>();
        let probe = Arc::new(move |name: &str, payload: serde_json::Value| {
            if name == "agent://activity" {
                let _ = tx.send(payload["detail"].as_str().unwrap().to_string());
            }
        });
        let bridge = HookBridge::serve(probe).unwrap();
        let body = format!(
            r#"{{"agentId":"a1","tool":"claude","event":"UserPromptSubmit","payload":{{"transcript_path":"{path}"}}}}"#
        );
        let (status, _) = post(&bridge.endpoint(), bridge.token(), &body);
        assert!(status.contains("204"), "status: {status}");
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            "💭 planning the change"
        );
    }

    // Dedup: posting the SAME detail twice (the transcript re-read returns the same
    // latest message across consecutive hooks) emits agent://activity only ONCE —
    // the second identical detail is collapsed. This is the volume fix.
    #[test]
    fn duplicate_detail_emits_activity_once() {
        let (tx, rx) = mpsc::channel::<String>();
        let probe = Arc::new(move |name: &str, payload: serde_json::Value| {
            if name == "agent://activity" {
                let _ = tx.send(payload["detail"].as_str().unwrap().to_string());
            }
        });
        let bridge = HookBridge::serve(probe).unwrap();
        // Two identical PreToolUse hooks → identical detail "▸ Bash: npm test".
        let body = r#"{"agentId":"a1","tool":"claude","event":"PreToolUse","payload":{"tool_name":"Bash","tool_input":{"command":"npm test"}}}"#;
        let (s1, _) = post(&bridge.endpoint(), bridge.token(), body);
        let (s2, _) = post(&bridge.endpoint(), bridge.token(), body);
        assert!(s1.contains("204") && s2.contains("204"));

        // Exactly ONE activity emit for the agent (the dupe is collapsed).
        assert_eq!(
            rx.recv_timeout(Duration::from_secs(5)).unwrap(),
            "▸ Bash: npm test"
        );
        assert!(
            rx.recv_timeout(Duration::from_millis(300)).is_err(),
            "duplicate detail must not emit a second agent://activity"
        );
    }

    // Status dedup: two consecutive working-status hooks emit agent://status only
    // ONCE (the second "working" is collapsed — same state), and an "idle" after a
    // "working" still emits (the state changed). This is the throughput fix: a flood
    // of Working hook events no longer re-pings "working" on every event.
    #[test]
    fn consecutive_working_status_emits_once_then_idle_emits() {
        let (tx, rx) = mpsc::channel::<String>();
        let probe = Arc::new(move |name: &str, payload: serde_json::Value| {
            if name == "agent://status" {
                let _ = tx.send(payload["state"].as_str().unwrap().to_string());
            }
        });
        let bridge = HookBridge::serve(probe).unwrap();

        // Two identical working-status hooks (UserPromptSubmit) → "working" once.
        let working =
            r#"{"agentId":"a1","tool":"claude","event":"UserPromptSubmit","payload":null}"#;
        let (s1, _) = post(&bridge.endpoint(), bridge.token(), working);
        let (s2, _) = post(&bridge.endpoint(), bridge.token(), working);
        assert!(s1.contains("204") && s2.contains("204"));

        // Exactly ONE "working" emit (the second is collapsed — same state).
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "working");
        assert!(
            rx.recv_timeout(Duration::from_millis(300)).is_err(),
            "consecutive working status must not re-emit agent://status"
        );

        // An idle (Stop) after working CHANGES state → it still emits.
        let (s3, _) = post(
            &bridge.endpoint(),
            bridge.token(),
            r#"{"agentId":"a1","tool":"claude","event":"Stop","payload":null}"#,
        );
        assert!(s3.contains("204"));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "idle");
    }

    // Non-empty gate: an event whose computed detail is empty/whitespace emits NO
    // agent://activity at all (it still acks). PreCompact parses to AgentEvent::Compact,
    // whose activity_detail() is None — verifying the gate path stays silent.
    #[test]
    fn empty_detail_emits_no_activity() {
        let (tx, rx) = mpsc::channel::<String>();
        let probe = Arc::new(move |name: &str, _payload: serde_json::Value| {
            if name == "agent://activity" {
                let _ = tx.send("activity".to_string());
            }
        });
        let bridge = HookBridge::serve(probe).unwrap();
        // Compact (PreCompact) produces `activity == None` → no emit.
        let (status, _) = post(
            &bridge.endpoint(),
            bridge.token(),
            r#"{"agentId":"a1","tool":"claude","event":"PreCompact","payload":null}"#,
        );
        assert!(status.contains("204"));
        assert!(
            rx.recv_timeout(Duration::from_millis(300)).is_err(),
            "an empty/absent detail must not emit agent://activity"
        );
    }

    // The agent://activity payload now carries the precise hook `event` (metadata)
    // alongside agentId / tool / detail, so the frontend can log/branch on it.
    #[test]
    fn activity_payload_includes_event_metadata() {
        let (tx, rx) = mpsc::channel::<(String, String)>();
        let probe = Arc::new(move |name: &str, payload: serde_json::Value| {
            if name == "agent://activity" {
                let _ = tx.send((
                    payload["event"].as_str().unwrap().to_string(),
                    payload["detail"].as_str().unwrap().to_string(),
                ));
            }
        });
        let bridge = HookBridge::serve(probe).unwrap();
        let (status, _) = post(
            &bridge.endpoint(),
            bridge.token(),
            r#"{"agentId":"a1","tool":"claude","event":"PreToolUse","payload":{"tool_name":"Bash","tool_input":{"command":"npm test"}}}"#,
        );
        assert!(status.contains("204"), "status: {status}");
        let (event, detail) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(event, "PreToolUse");
        assert_eq!(detail, "▸ Bash: npm test");
    }

    // The agent://activity payload now ALSO carries a semantic `kind` (for preview
    // coloring) alongside event/detail. A PreToolUse → ToolStart → kind "tool"; a
    // PostToolUse OK → ToolEnd(Ok) → kind "success". (See AgentEvent::kind.)
    #[test]
    fn activity_payload_includes_semantic_kind() {
        let (tx, rx) = mpsc::channel::<(String, String, String)>();
        let probe = Arc::new(move |name: &str, payload: serde_json::Value| {
            if name == "agent://activity" {
                let _ = tx.send((
                    payload["event"].as_str().unwrap().to_string(),
                    payload["kind"].as_str().unwrap().to_string(),
                    payload["detail"].as_str().unwrap().to_string(),
                ));
            }
        });
        let bridge = HookBridge::serve(probe).unwrap();

        // ToolStart → kind "tool".
        let (status, _) = post(
            &bridge.endpoint(),
            bridge.token(),
            r#"{"agentId":"a1","tool":"claude","event":"PreToolUse","payload":{"tool_name":"Bash","tool_input":{"command":"npm test"}}}"#,
        );
        assert!(status.contains("204"), "status: {status}");
        let (event, kind, detail) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(event, "PreToolUse");
        assert_eq!(kind, "tool");
        assert_eq!(detail, "▸ Bash: npm test");

        // ToolEnd(Ok) → kind "success".
        let (status, _) = post(
            &bridge.endpoint(),
            bridge.token(),
            r#"{"agentId":"a1","tool":"claude","event":"PostToolUse","payload":{"tool_name":"Edit","tool_input":{"file_path":"src/foo.rs"},"tool_response":{"success":true}}}"#,
        );
        assert!(status.contains("204"), "status: {status}");
        let (event, kind, detail) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(event, "PostToolUse");
        assert_eq!(kind, "success");
        assert_eq!(detail, "Edit ✓");
    }

    // A hook payload carrying a `session_id` is persisted via the `on_session`
    // sink — the bridge OWNS capture now (no agent://session event, no frontend
    // round-trip). The probe stands in for the DB-writing closure `start` builds.
    #[test]
    fn session_id_in_payload_invokes_on_session() {
        let (tx, rx) = mpsc::channel::<(String, String)>();
        let on_session: OnSession = Arc::new(move |agent_id: &str, session_id: &str| {
            let _ = tx.send((agent_id.to_string(), session_id.to_string()));
        });
        let bridge = HookBridge::serve_with_session(Arc::new(|_, _| {}), on_session).unwrap();
        let (status, _) = post(
            &bridge.endpoint(),
            bridge.token(),
            r#"{"agentId":"a1","tool":"claude","event":"SessionStart","payload":{"source":"startup","session_id":"sess-abc-123"}}"#,
        );
        assert!(status.contains("204"), "status: {status}");
        let (agent_id, session_id) = rx.recv_timeout(Duration::from_secs(5)).unwrap();
        assert_eq!(agent_id, "a1");
        assert_eq!(session_id, "sess-abc-123");
    }

    // A hook payload WITHOUT a session_id never invokes `on_session` (capture is
    // gated on the field being present).
    #[test]
    fn missing_session_id_does_not_invoke_on_session() {
        let (tx, rx) = mpsc::channel::<()>();
        let on_session: OnSession = Arc::new(move |_a: &str, _s: &str| {
            let _ = tx.send(());
        });
        let bridge = HookBridge::serve_with_session(Arc::new(|_, _| {}), on_session).unwrap();
        let (status, _) = post(
            &bridge.endpoint(),
            bridge.token(),
            r#"{"agentId":"a1","tool":"claude","event":"PreToolUse","payload":{"tool_name":"Bash","tool_input":{"command":"ls"}}}"#,
        );
        assert!(status.contains("204"), "status: {status}");
        assert!(
            rx.recv_timeout(Duration::from_millis(300)).is_err(),
            "no session_id in payload must not invoke on_session"
        );
    }

    // The SAME session id riding two consecutive hooks persists ONCE — the second
    // is collapsed by the per-agent dedup, so we don't write the DB + re-emit
    // agent://updated on every one of a session's hundreds of hooks.
    #[test]
    fn duplicate_session_id_invokes_on_session_once() {
        let (tx, rx) = mpsc::channel::<String>();
        let on_session: OnSession = Arc::new(move |_a: &str, session_id: &str| {
            let _ = tx.send(session_id.to_string());
        });
        let bridge = HookBridge::serve_with_session(Arc::new(|_, _| {}), on_session).unwrap();
        let body = r#"{"agentId":"a1","tool":"claude","event":"SessionStart","payload":{"session_id":"sess-dup"}}"#;
        let (s1, _) = post(&bridge.endpoint(), bridge.token(), body);
        let (s2, _) = post(&bridge.endpoint(), bridge.token(), body);
        assert!(s1.contains("204") && s2.contains("204"));
        assert_eq!(rx.recv_timeout(Duration::from_secs(5)).unwrap(), "sess-dup");
        assert!(
            rx.recv_timeout(Duration::from_millis(300)).is_err(),
            "duplicate session id must not persist twice"
        );
    }

    // A wrong token is rejected (the server only honours our own hooks).
    #[test]
    fn bad_token_is_unauthorized() {
        let bridge = HookBridge::serve(Arc::new(|_, _| {})).unwrap();
        let (status, _) = post(
            &bridge.endpoint(),
            "wrong-token",
            r#"{"agentId":"a","tool":"claude","event":"Notification","payload":null}"#,
        );
        assert!(status.contains("401"), "status: {status}");
    }

    #[test]
    fn endpoint_is_loopback() {
        let bridge = HookBridge {
            port: 1234,
            token: "t".into(),
        };
        assert_eq!(bridge.endpoint(), "http://127.0.0.1:1234");
    }
}
