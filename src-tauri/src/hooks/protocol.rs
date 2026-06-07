//! Pure, dependency-light parsing for the hook bridge: the minimal HTTP read,
//! the envelope `kat-hook` POSTs, and a generalized, structured cross-agent event
//! taxonomy (`AgentEvent`). Kept free of Tauri/sockets so it is trivially
//! unit-tested.
//!
//! `parse()` maps the raw `(hook name, payload)` an agent's native hook fires into
//! a tagged `AgentEvent` variant, pulling out the structured fields we care about
//! (tool name/input, permission detail + suggestions, assistant message text, …).
//! The original `env.payload` is NEVER dropped — anything we don't map stays
//! reachable via the envelope, and unmapped hook names fall through to
//! `AgentEvent::Unknown`. `AgentEvent::activity_detail()` renders the one-line
//! preview we surface in the activity feed (or `None` to show nothing).

use std::io::{self, BufRead};

use serde::Deserialize;
use serde_json::Value;

/// What `kat-hook` forwards: which agent, which tool, which hook fired, and the
/// tool's own request payload (as the agent handed it to the hook on stdin).
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HookEnvelope {
    pub agent_id: String,
    pub tool: String,
    pub event: String,
    #[serde(default)]
    pub payload: Value,
}

/// The tool request detail, normalized across agents. The most-telling fields are
/// lifted to named slots; `raw` keeps the agent's full, un-mapped tool input so no
/// data is lost (the frontend / future phases can dig deeper).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct ToolInput {
    pub command: Option<String>,
    pub description: Option<String>,
    pub file_path: Option<String>,
    pub raw: Value,
}

impl ToolInput {
    /// The single most-telling line for a preview: command → file_path →
    /// description. `None` when the input carries no human-readable hint.
    fn hint(&self) -> Option<&str> {
        self.command
            .as_deref()
            .or(self.file_path.as_deref())
            .or(self.description.as_deref())
    }

    /// A human-readable summary of WHAT this tool call is — richer than `hint()`,
    /// the line the activity feed / permission card should show. Priority:
    ///
    /// 1. The named slots (command / file_path / description) via `hint()` — these
    ///    are the most telling when present (Bash/Edit/Write/Read …).
    /// 2. Tool-specific content dug out of `raw` for tools that carry NEITHER a
    ///    command NOR a file_path — ESPECIALLY `AskUserQuestion`, whose real ask
    ///    lives in `raw.questions[].question` / `.header` (this is the user's
    ///    complaint: the card showed only the bare tool name).
    /// 3. A generic fallback: the most salient string field in `raw`.
    ///
    /// `None` only when the input is completely opaque (no string content at all).
    pub fn summary(&self) -> Option<String> {
        if let Some(h) = self.hint() {
            return Some(h.to_string());
        }
        // AskUserQuestion: pull the actual question(s)/header(s) from the payload.
        if let Some(s) = ask_user_question_summary(&self.raw) {
            return Some(s);
        }
        // Generic: the most salient string field anywhere in the input.
        salient_field(&self.raw)
    }

    /// The structured questions of an `AskUserQuestion` tool input, if present —
    /// each `{question, header, options:[{label, description}…]}` parsed from
    /// `raw.questions[]`. Empty when this input is not an AskUserQuestion (so
    /// callers can branch on `is_empty()`). Lets the frontend render the ask as a
    /// real choice card with per-option descriptions.
    pub fn questions(&self) -> Vec<Question> {
        parse_questions(&self.raw)
    }
}

/// One clarifying question from Claude's `AskUserQuestion` tool input —
/// `{question, header, options:[{label, description}…], multiSelect}` (see
/// code.claude.com/docs/en/agent-sdk/user-input). `options` carries each choice's
/// label AND its description (so the frontend can show both); `header` is the
/// short chip label; `multi_select` is whether more than one option may be picked
/// (Claude's `multiSelect` flag, default false). NOTE: Claude's TUI auto-appends
/// an "Other" free-text choice AFTER these concrete `options` — it is NOT in the
/// payload, so the frontend synthesizes it (Other's number = options.len() + 1).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct Question {
    pub question: String,
    pub header: Option<String>,
    pub options: Vec<QuestionOption>,
    /// Whether multiple options may be selected for this question (Claude's
    /// `multiSelect`; default false → single-select). Best-effort to answer over
    /// the PTY: single-select is reliable, multi-select is officially buggy.
    pub multi_select: bool,
}

/// One selectable option of an `AskUserQuestion` question — `{label, description}`.
/// `label` is what the user picks; `description` is the explanatory blurb the agent
/// attached (empty string when the option was a bare label).
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct QuestionOption {
    pub label: String,
    pub description: String,
}

/// Outcome of a finished tool call.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ToolStatus {
    Ok,
    Failed,
    Denied,
}

/// One "always allow / always deny" entry the agent offered alongside a permission
/// request — Claude's `permission_suggestions`, flattened to a flat shape the
/// frontend can render directly. `behavior` is "allow"/"deny" (best-effort);
/// `rule` is the rule text (e.g. `Bash(npm test:*)`); `description` is a label.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Suggestion {
    pub behavior: String,
    pub rule: String,
    pub description: String,
}

/// The generalized, structured cross-agent event taxonomy. `parse()` produces one
/// of these from a `HookEnvelope`; `handle()` decides what to emit per variant.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum AgentEvent {
    /// A session/turn-bearing process came up. `source` = how it started.
    SessionStart { source: Option<String> },
    /// The user submitted a prompt (turn start).
    UserPrompt { text: String },
    /// Streamed assistant output (Claude MessageDisplay / Cursor
    /// afterAgentResponse / Codex last_assistant_message).
    AgentMessage { text: String },
    /// A tool is about to run.
    ToolStart { tool: String, input: ToolInput },
    /// A tool finished.
    ToolEnd {
        tool: String,
        status: ToolStatus,
        result: Option<String>,
    },
    /// The agent is asking the user to approve a tool (FULL detail — replaces the
    /// old squeezed `NeedsInput`).
    PermissionRequest {
        tool: String,
        input: ToolInput,
        mode: Option<String>,
        suggestions: Vec<Suggestion>,
    },
    /// A best-effort notification (Claude `Notification`: permission_prompt /
    /// idle_prompt / …). `kind` = notification_type.
    Notification {
        kind: Option<String>,
        message: String,
    },
    /// The agent finished a turn (Stop).
    TurnEnd,
    /// The session ended.
    SessionEnd { reason: Option<String> },
    /// Context compaction lifecycle (PreCompact / …).
    Compact { phase: Option<String> },
    /// An error surfaced by the agent.
    Error {
        kind: Option<String>,
        message: String,
    },
    /// A hook name we don't (yet) map — acknowledged, no action. The raw payload is
    /// still reachable via the envelope.
    Unknown,
}

/// Map a `HookEnvelope` (hook name + raw payload) into a structured `AgentEvent`.
///
/// Maps the event NAMES of all four agents into the shared taxonomy:
///
/// * **Claude** — `PreToolUse`/`PostToolUse`/`PostToolUseFailure`/`PermissionRequest`/
///   `Notification`/`UserPromptSubmit`/`MessageDisplay`/`Stop`/`SessionStart`/
///   `SessionEnd`/`PreCompact`.
/// * **Codex** — same capitalized names as Claude (array-table schema:
///   developers.openai.com/codex/hooks) plus `PostCompact`/`SubagentStop`. Failure
///   is detected from the `tool_response` (no separate failure event).
/// * **Cursor** — camelCase: `beforeShellExecution`/`beforeMCPExecution`/`preToolUse`
///   → ToolStart; `afterShellExecution`/`afterMCPExecution`/`postToolUse`/
///   `afterFileEdit` → ToolEnd(Ok); `postToolUseFailure` → ToolEnd(Failed);
///   `afterAgentResponse`/`afterAgentThought` → AgentMessage; `beforeSubmitPrompt`
///   → UserPrompt; `stop` → TurnEnd; `sessionStart`/`sessionEnd` (cursor.com/docs/hooks).
///   Cursor has NO dedicated permission EVENT (it is returned inline from the
///   before* hooks), so katrix surfaces no PermissionRequest for cursor.
/// * **Gemini** — `BeforeTool`/`AfterTool`/`BeforeAgent`/`AfterAgent`/`AfterModel`/
///   `Notification`/`SessionStart`/`SessionEnd` (geminicli.com/docs/hooks). Gemini
///   has NO permission "ask" event, so a tool denial only ever surfaces as a
///   ToolEnd(Denied) when the result payload makes it detectable.
///
/// The field extraction is written generically (tool_name /
/// tool_input.{command,description,file_path}, message text fields, …). Unmapped
/// hook names → `AgentEvent::Unknown`; the raw `env.payload` is never consumed, so
/// nothing is lost.
pub fn parse(env: &HookEnvelope) -> AgentEvent {
    let p = &env.payload;
    match env.event.as_str() {
        // Claude / Codex / Gemini (BeforeAgent) session start.
        "SessionStart" | "sessionStart" | "BeforeAgent" => AgentEvent::SessionStart {
            source: string_field(p, "source"),
        },

        // Claude/Codex UserPromptSubmit; Cursor beforeSubmitPrompt.
        "UserPromptSubmit" | "beforeSubmitPrompt" => AgentEvent::UserPrompt {
            text: string_field(p, "prompt").unwrap_or_default(),
        },

        // Streamed / final assistant text. Claude MessageDisplay's new chunk is in
        // `delta`; Cursor afterAgentResponse carries `text`; Gemini AfterModel /
        // Codex SubagentStop carry `last_assistant_message`/`message`.
        "MessageDisplay" | "afterAgentResponse" | "AfterModel" => AgentEvent::AgentMessage {
            text: assistant_text(p).unwrap_or_default(),
        },

        // Cursor afterAgentThought is assistant *thinking* — surface it as an
        // AgentMessage but tag it with the 💭 prefix so the feed reads as a thought,
        // not final prose (the variant carries only text, so we bake it in here).
        "afterAgentThought" => AgentEvent::AgentMessage {
            text: match assistant_text(p) {
                Some(t) if !t.trim().is_empty() => format!("💭 {}", t.trim()),
                _ => String::new(),
            },
        },

        // Tool about to run. Claude PreToolUse; Cursor before*/preToolUse; Codex
        // PreToolUse; Gemini BeforeTool.
        "PreToolUse" | "preToolUse" | "beforeShellExecution" | "beforeMCPExecution"
        | "BeforeTool" => AgentEvent::ToolStart {
            tool: tool_name(env),
            input: parse_tool_input(p),
        },

        // Tool finished OK (status is still re-derived from the payload, so a
        // result that carries an error/non-zero exit flips to Failed). Claude
        // PostToolUse; Cursor after*/postToolUse; Codex PostToolUse; Gemini AfterTool.
        "PostToolUse" | "postToolUse" | "afterShellExecution" | "afterMCPExecution"
        | "afterFileEdit" | "AfterTool" => {
            let (status, result) = parse_tool_result(p);
            AgentEvent::ToolEnd {
                tool: tool_name(env),
                status,
                result,
            }
        }

        // An explicit tool-failure event. Claude PostToolUseFailure; Cursor
        // postToolUseFailure ("fires when a tool fails, times out, or is denied").
        "PostToolUseFailure" | "postToolUseFailure" => AgentEvent::ToolEnd {
            tool: tool_name(env),
            status: ToolStatus::Failed,
            result: string_field(p, "error_message")
                .or_else(|| string_field(p, "error"))
                .or_else(|| string_field(p, "message")),
        },

        // A dedicated permission ask carries the FULL request detail. Claude +
        // Codex PermissionRequest. (Cursor/Gemini have no permission event.)
        "PermissionRequest" => AgentEvent::PermissionRequest {
            tool: tool_name(env),
            input: parse_tool_input(p),
            mode: string_field(p, "permission_mode"),
            suggestions: parse_suggestions(p),
        },

        // `Notification` (Claude + Gemini share the name). For Claude a
        // `permission_prompt` is really a permission ask — promote it so the card
        // shows the request, not a generic notice. (The current Claude payload only
        // carries `message` for it, so the tool/input detail is best-effort and
        // usually empty until Claude enriches it.) Gemini has NO permission ask, so
        // its Notifications always stay generic.
        "Notification" => {
            let kind = string_field(p, "notification_type").or_else(|| string_field(p, "type"));
            if env.tool == "claude" && kind.as_deref() == Some("permission_prompt") {
                AgentEvent::PermissionRequest {
                    tool: tool_name(env),
                    input: parse_tool_input(p),
                    mode: string_field(p, "permission_mode"),
                    suggestions: parse_suggestions(p),
                }
            } else {
                AgentEvent::Notification {
                    kind,
                    message: string_field(p, "message").unwrap_or_default(),
                }
            }
        }

        // Turn finished. Claude/Codex Stop; Cursor stop; Gemini AfterAgent.
        "Stop" | "stop" | "AfterAgent" => AgentEvent::TurnEnd,

        // Codex SubagentStop: surface the subagent's final message as AgentMessage
        // when it carries text, else acknowledge with Unknown.
        "SubagentStop" => match assistant_text(p) {
            Some(text) if !text.trim().is_empty() => AgentEvent::AgentMessage { text },
            _ => AgentEvent::Unknown,
        },

        // Claude/Codex SessionEnd; Cursor sessionEnd.
        "SessionEnd" | "sessionEnd" => AgentEvent::SessionEnd {
            // Claude's SessionEnd matcher key is `why_session_ended`; accept the
            // older `reason` as a fallback.
            reason: string_field(p, "why_session_ended").or_else(|| string_field(p, "reason")),
        },

        // Claude PreCompact; Codex PreCompact/PostCompact.
        "PreCompact" | "PostCompact" | "Compact" => AgentEvent::Compact {
            phase: string_field(p, "phase").or_else(|| string_field(p, "trigger")),
        },

        "Error" => AgentEvent::Error {
            kind: string_field(p, "error_type").or_else(|| string_field(p, "kind")),
            message: string_field(p, "message")
                .or_else(|| string_field(p, "error"))
                .unwrap_or_default(),
        },

        _ => AgentEvent::Unknown,
    }
}

impl AgentEvent {
    /// The one-line preview to surface in the activity feed for this event, or
    /// `None` to show nothing. Callers PREFER richer transcript content where
    /// available (see `transcript::latest_content`) — this is the structured
    /// fallback / the canonical line for variants with inline detail.
    pub fn activity_detail(&self) -> Option<String> {
        match self {
            AgentEvent::PermissionRequest { tool, input, .. } => Some(match input.summary() {
                Some(h) => format!("✋ {tool}: {h}"),
                None => format!("✋ {tool}"),
            }),
            AgentEvent::AgentMessage { text } => non_empty(text),
            AgentEvent::ToolStart { tool, input } => Some(match input.summary() {
                Some(h) => format!("▸ {tool}: {h}"),
                None => format!("▸ {tool}"),
            }),
            AgentEvent::ToolEnd { tool, status, .. } => Some(match status {
                ToolStatus::Ok => format!("{tool} ✓"),
                ToolStatus::Failed => format!("{tool} ✗"),
                ToolStatus::Denied => format!("{tool} ⃠"),
            }),
            AgentEvent::UserPrompt { text } => Some(non_empty(text).unwrap_or_else(|| "working…".into())),
            AgentEvent::SessionStart { .. } => Some("● started".into()),
            AgentEvent::TurnEnd => Some("✓ done".into()),
            AgentEvent::Notification { message, .. } => non_empty(message),
            AgentEvent::Error { message, .. } => Some(format!("✗ {message}")),
            // No inline preview for these — the card stays on its prior state.
            AgentEvent::SessionEnd { .. } | AgentEvent::Compact { .. } | AgentEvent::Unknown => None,
        }
    }

    /// A coarse SEMANTIC class for this event, for preview coloring on the frontend.
    /// One of: "user" | "agent" | "tool" | "success" | "error" | "question" |
    /// "info". The frontend maps each to a color (e.g. user→accent, error→red,
    /// success→green, question→amber). This is emitted alongside the activity line
    /// (`agent://activity.kind`) so the feed can tint each entry without re-parsing
    /// the detail string.
    pub fn kind(&self) -> &'static str {
        match self {
            AgentEvent::UserPrompt { .. } => "user",
            AgentEvent::AgentMessage { .. } => "agent",
            AgentEvent::ToolStart { .. } => "tool",
            AgentEvent::ToolEnd { status, .. } => match status {
                ToolStatus::Ok => "success",
                ToolStatus::Failed | ToolStatus::Denied => "error",
            },
            AgentEvent::PermissionRequest { .. } => "question",
            // A needs-input / permission_prompt notification is really a question;
            // any other notification kind is informational.
            AgentEvent::Notification { kind, .. } => match kind.as_deref() {
                Some("permission_prompt") | Some("needs_input") | Some("idle_prompt") => "question",
                _ => "info",
            },
            AgentEvent::TurnEnd => "success",
            AgentEvent::SessionStart { .. }
            | AgentEvent::SessionEnd { .. }
            | AgentEvent::Compact { .. } => "info",
            AgentEvent::Error { .. } => "error",
            AgentEvent::Unknown => "info",
        }
    }
}

/// `Some(trimmed)` when `s` is non-blank, else `None`.
fn non_empty(s: &str) -> Option<String> {
    let t = s.trim();
    (!t.is_empty()).then(|| t.to_string())
}

/// A string field off a JSON object, if present and a string.
fn string_field(p: &Value, key: &str) -> Option<String> {
    p.get(key).and_then(Value::as_str).map(str::to_string)
}

/// Best-effort assistant/message text across agents' assorted shapes: Claude's
/// streamed `delta`, Cursor's `text`, Codex's `last_assistant_message`, a plain
/// `message` fallback, and Gemini's nested `llm_response.candidates[].content
/// .parts[]` (its AfterModel hook carries the model output there — NOT in any of
/// the flat fields). `None` when none are present.
fn assistant_text(p: &Value) -> Option<String> {
    string_field(p, "delta")
        .or_else(|| string_field(p, "text"))
        .or_else(|| string_field(p, "last_assistant_message"))
        .or_else(|| string_field(p, "message"))
        .or_else(|| gemini_model_text(p))
}

/// Pull Gemini's model output text out of `llm_response.candidates[].content
/// .parts[]` (geminicli.com/docs/hooks/reference). `parts` are text fragments;
/// we join the first candidate's parts. `None` when the shape isn't present.
fn gemini_model_text(p: &Value) -> Option<String> {
    let parts = p
        .get("llm_response")?
        .get("candidates")?
        .as_array()?
        .first()?
        .get("content")?
        .get("parts")?
        .as_array()?;
    let joined: String = parts
        .iter()
        .filter_map(|part| {
            // A part is either a bare string or `{text: "…"}`.
            part.as_str()
                .map(str::to_string)
                .or_else(|| string_field(part, "text"))
        })
        .collect::<Vec<_>>()
        .join("");
    non_empty(&joined)
}

/// The tool name for this event: the payload's `tool_name` if present (Claude /
/// Codex), else the envelope's `tool` (Cursor's before/after* shapes name no tool
/// in-payload, so we fall back to the agent label).
fn tool_name(env: &HookEnvelope) -> String {
    string_field(&env.payload, "tool_name").unwrap_or_else(|| env.tool.clone())
}

/// Normalize a tool request payload into `ToolInput`. Claude/Codex/Gemini nest it
/// under `tool_input`; Cursor's before* shapes put `command` at the top level. We
/// keep the full nested object (or the whole payload) as `raw` so nothing is lost.
fn parse_tool_input(p: &Value) -> ToolInput {
    let input = p.get("tool_input").unwrap_or(p);
    ToolInput {
        command: string_field(input, "command"),
        description: string_field(input, "description"),
        file_path: string_field(input, "file_path"),
        raw: input.clone(),
    }
}

/// Parse Claude's `AskUserQuestion` tool input — `raw.questions[]`, each
/// `{question, header, options:[{label, description}…]}` — into `Vec<Question>`.
/// Empty when the input has no `questions` array (i.e. not an AskUserQuestion).
/// Each option keeps BOTH its label (what the user picks) and its description.
fn parse_questions(raw: &Value) -> Vec<Question> {
    let arr = match raw.get("questions").and_then(Value::as_array) {
        Some(a) => a,
        None => return Vec::new(),
    };
    arr.iter()
        .filter_map(|q| {
            let question = string_field(q, "question")?;
            let options = q
                .get("options")
                .and_then(Value::as_array)
                .map(|opts| {
                    opts.iter()
                        .filter_map(|o| {
                            // An option is usually `{label, description}`; tolerate
                            // a bare string option (label only, no description).
                            if let Some(label) = string_field(o, "label") {
                                Some(QuestionOption {
                                    label,
                                    description: string_field(o, "description")
                                        .unwrap_or_default(),
                                })
                            } else {
                                o.as_str().map(|s| QuestionOption {
                                    label: s.to_string(),
                                    description: String::new(),
                                })
                            }
                        })
                        .collect()
                })
                .unwrap_or_default();
            Some(Question {
                question,
                header: string_field(q, "header"),
                options,
                // Claude's `multiSelect` flag (camelCase in tool_input); absent or
                // non-bool → false (the common single-select case).
                multi_select: q.get("multiSelect").and_then(Value::as_bool).unwrap_or(false),
            })
        })
        .collect()
}

/// A human line for an `AskUserQuestion` tool input pulled from `raw.questions[]`:
/// the first question's text (prefixed with its header chip when present), plus a
/// "(+N more)" tail when there are several. `None` when there are no questions —
/// so the generic fallback can take over. This is the fix for the user's
/// complaint: the card showed "✋ AskUserQuestion" instead of the actual ask.
fn ask_user_question_summary(raw: &Value) -> Option<String> {
    let qs = parse_questions(raw);
    let first = qs.first()?;
    let head = match &first.header {
        Some(h) if !h.trim().is_empty() => format!("[{}] ", h.trim()),
        _ => String::new(),
    };
    let extra = qs.len().saturating_sub(1);
    let tail = if extra > 0 {
        format!(" (+{extra} more)")
    } else {
        String::new()
    };
    Some(format!("{head}{}{tail}", first.question.trim()))
}

/// Generic fallback summary: the most salient short string field in an arbitrary
/// tool input object, best-effort across tools we don't special-case (Grep's
/// `pattern`, WebFetch's `url`, a Task's `prompt`, a skill's `skill`/`name`, …).
/// `None` when no usable string field is present.
fn salient_field(raw: &Value) -> Option<String> {
    let obj = raw.as_object()?;
    for key in [
        "command",
        "file_path",
        "path",
        "pattern",
        "query",
        "url",
        "prompt",
        "skill",
        "name",
        "content",
        "message",
        "description",
    ] {
        if let Some(s) = obj.get(key).and_then(Value::as_str) {
            let s = s.split_whitespace().collect::<Vec<_>>().join(" ");
            if !s.is_empty() {
                let clipped: String = s.chars().take(120).collect();
                return Some(if s.chars().count() > 120 {
                    format!("{}…", clipped.trim_end())
                } else {
                    clipped
                });
            }
        }
    }
    None
}

/// Derive `(status, result)` from a finished-tool payload, best-effort across the
/// loosely-documented per-tool `tool_response` shapes plus Cursor's flat after*
/// shapes. An explicit `success:false`, a non-zero `exit_code`, or an `error`
/// field flips it to Failed; a permission-style "denied" marks Denied.
fn parse_tool_result(p: &Value) -> (ToolStatus, Option<String>) {
    let resp = p.get("tool_response").unwrap_or(p);

    // Explicit denial (some agents surface a permission decision on the result).
    let denied = resp
        .get("decision")
        .and_then(Value::as_str)
        .map(|d| d.eq_ignore_ascii_case("deny") || d.eq_ignore_ascii_case("denied"))
        .unwrap_or(false)
        || resp.get("permissionDenied").and_then(Value::as_bool) == Some(true);

    let failed = resp.get("success").and_then(Value::as_bool) == Some(false)
        || resp
            .get("exit_code")
            .and_then(Value::as_i64)
            .is_some_and(|c| c != 0)
        || resp.get("error").is_some();

    let status = if denied {
        ToolStatus::Denied
    } else if failed {
        ToolStatus::Failed
    } else {
        ToolStatus::Ok
    };

    // A short result hint: an explicit exit code, an error string, else Cursor's
    // flat `output` (afterShellExecution) / `result_json` (afterMCPExecution) so a
    // successful tool still carries a peek at what it returned.
    let result = match resp.get("exit_code").and_then(Value::as_i64) {
        Some(code) if code != 0 => Some(format!("exit {code}")),
        _ => resp
            .get("error")
            .and_then(Value::as_str)
            .or_else(|| p.get("output").and_then(Value::as_str))
            .or_else(|| p.get("result_json").and_then(Value::as_str))
            .map(str::to_string),
    };

    (status, result)
}

/// Flatten Claude's `permission_suggestions` into `Vec<Suggestion>`. Claude's real
/// shape is loosely documented and evolving (entries keyed by `type` / `behavior`
/// with rule text under `rules` / `ruleValue` / `rule`), so we read defensively:
/// pull a behavior ("allow"/"deny"), a rule string, and a description from whatever
/// keys are present; an array of rule strings is expanded into one entry each.
/// Empty `Vec` when absent — the full original stays reachable via the raw payload.
fn parse_suggestions(p: &Value) -> Vec<Suggestion> {
    let arr = match p.get("permission_suggestions").and_then(Value::as_array) {
        Some(a) => a,
        None => return Vec::new(),
    };
    let mut out = Vec::new();
    for entry in arr {
        let behavior = string_field(entry, "behavior")
            .or_else(|| string_field(entry, "type"))
            .unwrap_or_else(|| "allow".into());
        let description = string_field(entry, "description")
            .or_else(|| string_field(entry, "title"))
            .unwrap_or_default();

        // The rule text may be a single string (`rule`/`ruleValue`/`content`) or an
        // array of rule strings (`rules`). An array → one Suggestion per rule.
        if let Some(rules) = entry.get("rules").and_then(Value::as_array) {
            for r in rules {
                if let Some(rule) = r.as_str() {
                    out.push(Suggestion {
                        behavior: behavior.clone(),
                        rule: rule.to_string(),
                        description: description.clone(),
                    });
                }
            }
            continue;
        }
        let rule = string_field(entry, "rule")
            .or_else(|| string_field(entry, "ruleValue"))
            .or_else(|| string_field(entry, "content"))
            .unwrap_or_default();
        out.push(Suggestion {
            behavior,
            rule,
            description,
        });
    }
    out
}

/// A parsed inbound HTTP request — only the bits the bridge cares about.
#[derive(Debug, Clone)]
pub struct HttpRequest {
    /// Bearer token from the `Authorization` header, if any.
    pub auth: Option<String>,
    pub body: String,
}

/// Read one HTTP/1.1 request: skip the request line, collect headers until the
/// blank line, then read exactly `Content-Length` body bytes.
pub fn read_request<R: BufRead>(r: &mut R) -> io::Result<HttpRequest> {
    let mut line = String::new();
    r.read_line(&mut line)?; // request line — ignored (always POST /)

    let mut len = 0usize;
    let mut auth = None;
    loop {
        line.clear();
        let n = r.read_line(&mut line)?;
        if n == 0 {
            break;
        }
        let t = line.trim_end();
        if t.is_empty() {
            break; // end of headers
        }
        if let Some((name, value)) = t.split_once(':') {
            match name.trim().to_ascii_lowercase().as_str() {
                "content-length" => len = value.trim().parse().unwrap_or(0),
                "authorization" => {
                    auth = Some(value.trim().trim_start_matches("Bearer ").trim().to_string())
                }
                _ => {}
            }
        }
    }

    let mut buf = vec![0u8; len];
    r.read_exact(&mut buf)?;
    Ok(HttpRequest {
        auth,
        body: String::from_utf8_lossy(&buf).to_string(),
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    fn env(event: &str, payload: Value) -> HookEnvelope {
        HookEnvelope {
            agent_id: "a".into(),
            tool: "claude".into(),
            event: event.into(),
            payload,
        }
    }

    // --- parse(): variant + structured field extraction ---------------------

    #[test]
    fn parses_pre_tool_use_into_tool_start_with_command() {
        let e = env(
            "PreToolUse",
            serde_json::json!({ "tool_name": "Bash", "tool_input": { "command": "npm test" } }),
        );
        match parse(&e) {
            AgentEvent::ToolStart { tool, input } => {
                assert_eq!(tool, "Bash");
                assert_eq!(input.command.as_deref(), Some("npm test"));
            }
            other => panic!("expected ToolStart, got {other:?}"),
        }
    }

    #[test]
    fn parses_post_tool_use_into_tool_end_ok_and_failed() {
        let ok = env(
            "PostToolUse",
            serde_json::json!({
                "tool_name": "Edit",
                "tool_input": { "file_path": "src/foo.rs" },
                "tool_response": { "success": true }
            }),
        );
        match parse(&ok) {
            AgentEvent::ToolEnd { tool, status, .. } => {
                assert_eq!(tool, "Edit");
                assert_eq!(status, ToolStatus::Ok);
            }
            other => panic!("expected ToolEnd, got {other:?}"),
        }

        let bad = env(
            "PostToolUse",
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": { "command": "npm test" },
                "tool_response": { "exit_code": 1 }
            }),
        );
        match parse(&bad) {
            AgentEvent::ToolEnd { status, result, .. } => {
                assert_eq!(status, ToolStatus::Failed);
                assert_eq!(result.as_deref(), Some("exit 1"));
            }
            other => panic!("expected ToolEnd, got {other:?}"),
        }
    }

    #[test]
    fn parses_message_display_into_agent_message() {
        // MessageDisplay's new chunk is in `delta` (confirmed: the input carries
        // the new text in delta).
        let e = env(
            "MessageDisplay",
            serde_json::json!({ "delta": "refactoring the parser now" }),
        );
        assert_eq!(
            parse(&e),
            AgentEvent::AgentMessage { text: "refactoring the parser now".into() }
        );
    }

    #[test]
    fn parses_permission_request_with_suggestions() {
        let e = env(
            "PermissionRequest",
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": { "command": "git push" },
                "permission_mode": "default",
                "permission_suggestions": [
                    { "behavior": "allow", "rule": "Bash(git push:*)", "description": "Always allow git push" }
                ]
            }),
        );
        match parse(&e) {
            AgentEvent::PermissionRequest { tool, input, mode, suggestions } => {
                assert_eq!(tool, "Bash");
                assert_eq!(input.command.as_deref(), Some("git push"));
                assert_eq!(mode.as_deref(), Some("default"));
                assert_eq!(suggestions.len(), 1);
                assert_eq!(suggestions[0].behavior, "allow");
                assert_eq!(suggestions[0].rule, "Bash(git push:*)");
                assert_eq!(suggestions[0].description, "Always allow git push");
            }
            other => panic!("expected PermissionRequest, got {other:?}"),
        }
    }

    #[test]
    fn parses_permission_suggestions_rules_array_into_one_entry_each() {
        // A `type`/`rules[]`-shaped entry (Claude's evolving shape) flattens to one
        // Suggestion per rule, carrying the entry's behavior/type.
        let e = env(
            "PermissionRequest",
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": { "command": "rm -rf x" },
                "permission_suggestions": [
                    { "type": "deny", "rules": ["Bash(rm:*)", "Bash(rmdir:*)"] }
                ]
            }),
        );
        match parse(&e) {
            AgentEvent::PermissionRequest { suggestions, .. } => {
                assert_eq!(suggestions.len(), 2);
                assert!(suggestions.iter().all(|s| s.behavior == "deny"));
                assert_eq!(suggestions[0].rule, "Bash(rm:*)");
                assert_eq!(suggestions[1].rule, "Bash(rmdir:*)");
            }
            other => panic!("expected PermissionRequest, got {other:?}"),
        }
    }

    #[test]
    fn parses_permission_request_with_no_suggestions_into_empty_vec() {
        let e = env(
            "PermissionRequest",
            serde_json::json!({ "tool_name": "Bash", "tool_input": { "command": "ls" } }),
        );
        match parse(&e) {
            AgentEvent::PermissionRequest { suggestions, .. } => assert!(suggestions.is_empty()),
            other => panic!("expected PermissionRequest, got {other:?}"),
        }
    }

    #[test]
    fn parses_notification_permission_prompt_as_permission_request() {
        // notification_type permission_prompt is really a permission ask (confirmed
        // notification_type values include permission_prompt / idle_prompt).
        let e = env(
            "Notification",
            serde_json::json!({
                "notification_type": "permission_prompt",
                "message": "Claude needs your permission to use Bash"
            }),
        );
        match parse(&e) {
            AgentEvent::PermissionRequest { tool, .. } => assert_eq!(tool, "claude"),
            other => panic!("expected PermissionRequest, got {other:?}"),
        }
    }

    #[test]
    fn parses_generic_notification_into_notification() {
        let e = env(
            "Notification",
            serde_json::json!({ "notification_type": "idle_prompt", "message": "waiting on you" }),
        );
        assert_eq!(
            parse(&e),
            AgentEvent::Notification {
                kind: Some("idle_prompt".into()),
                message: "waiting on you".into()
            }
        );
    }

    #[test]
    fn parses_stop_into_turn_end() {
        assert_eq!(parse(&env("Stop", Value::Null)), AgentEvent::TurnEnd);
    }

    #[test]
    fn parses_session_lifecycle() {
        assert_eq!(
            parse(&env("SessionStart", serde_json::json!({ "source": "startup" }))),
            AgentEvent::SessionStart { source: Some("startup".into()) }
        );
        assert_eq!(
            parse(&env("SessionEnd", serde_json::json!({ "why_session_ended": "logout" }))),
            AgentEvent::SessionEnd { reason: Some("logout".into()) }
        );
    }

    #[test]
    fn parses_user_prompt_submit() {
        assert_eq!(
            parse(&env("UserPromptSubmit", serde_json::json!({ "prompt": "fix the bug" }))),
            AgentEvent::UserPrompt { text: "fix the bug".into() }
        );
    }

    #[test]
    fn unknown_event_parses_to_unknown() {
        assert_eq!(parse(&env("WhoKnows", Value::Null)), AgentEvent::Unknown);
    }

    // --- cross-agent event name mapping (codex / cursor / gemini) -----------

    /// An envelope tagged with a specific agent tool (the default `env()` is claude).
    fn env_for(tool: &str, event: &str, payload: Value) -> HookEnvelope {
        HookEnvelope {
            agent_id: "a".into(),
            tool: tool.into(),
            event: event.into(),
            payload,
        }
    }

    #[test]
    fn parses_codex_events() {
        // Codex uses Claude's capitalized event names (array-table schema). The
        // tool-lifecycle + lifecycle arms are shared, but PermissionRequest carries
        // no suggestions (codex doesn't offer them) and SubagentStop folds to text.
        let start = parse(&env_for("codex", "SessionStart", serde_json::json!({ "source": "cli" })));
        assert_eq!(start, AgentEvent::SessionStart { source: Some("cli".into()) });

        let prompt = parse(&env_for("codex", "UserPromptSubmit", serde_json::json!({ "prompt": "ship it" })));
        assert_eq!(prompt, AgentEvent::UserPrompt { text: "ship it".into() });

        let pre = env_for("codex", "PreToolUse", serde_json::json!({ "tool_name": "Bash", "tool_input": { "command": "ls" } }));
        match parse(&pre) {
            AgentEvent::ToolStart { tool, input } => {
                assert_eq!(tool, "Bash");
                assert_eq!(input.command.as_deref(), Some("ls"));
            }
            other => panic!("expected ToolStart, got {other:?}"),
        }

        // PostToolUse with an error in tool_response → Failed.
        let post = env_for("codex", "PostToolUse", serde_json::json!({
            "tool_name": "Bash",
            "tool_response": { "error": "boom" }
        }));
        match parse(&post) {
            AgentEvent::ToolEnd { status, result, .. } => {
                assert_eq!(status, ToolStatus::Failed);
                assert_eq!(result.as_deref(), Some("boom"));
            }
            other => panic!("expected ToolEnd, got {other:?}"),
        }

        // Codex PermissionRequest: full detail, never any suggestions.
        let perm = env_for("codex", "PermissionRequest", serde_json::json!({
            "tool_name": "Bash",
            "tool_input": { "command": "rm x", "description": "delete x" },
            "permission_mode": "ask"
        }));
        match parse(&perm) {
            AgentEvent::PermissionRequest { tool, input, mode, suggestions } => {
                assert_eq!(tool, "Bash");
                assert_eq!(input.command.as_deref(), Some("rm x"));
                assert_eq!(mode.as_deref(), Some("ask"));
                assert!(suggestions.is_empty(), "codex offers no suggestions");
            }
            other => panic!("expected PermissionRequest, got {other:?}"),
        }

        assert_eq!(parse(&env_for("codex", "Stop", Value::Null)), AgentEvent::TurnEnd);
        assert_eq!(
            parse(&env_for("codex", "PostCompact", serde_json::json!({ "trigger": "auto" }))),
            AgentEvent::Compact { phase: Some("auto".into()) }
        );

        // SubagentStop with a final message → AgentMessage; without → Unknown.
        assert_eq!(
            parse(&env_for("codex", "SubagentStop", serde_json::json!({ "last_assistant_message": "done" }))),
            AgentEvent::AgentMessage { text: "done".into() }
        );
        assert_eq!(parse(&env_for("codex", "SubagentStop", Value::Null)), AgentEvent::Unknown);
    }

    #[test]
    fn parses_cursor_events() {
        // Cursor camelCase names. before*/preToolUse → ToolStart; after*/postToolUse
        // → ToolEnd(Ok); postToolUseFailure → ToolEnd(Failed); afterAgentResponse /
        // afterAgentThought → AgentMessage; beforeSubmitPrompt → UserPrompt; the
        // session lifecycle maps too.
        let pre = env_for("cursor", "beforeShellExecution", serde_json::json!({ "command": "cargo build" }));
        match parse(&pre) {
            AgentEvent::ToolStart { tool, input } => {
                assert_eq!(tool, "cursor"); // no tool_name → falls back to the agent label
                assert_eq!(input.command.as_deref(), Some("cargo build"));
            }
            other => panic!("expected ToolStart, got {other:?}"),
        }

        assert!(matches!(
            parse(&env_for("cursor", "preToolUse", serde_json::json!({ "tool_input": { "command": "ls" } }))),
            AgentEvent::ToolStart { .. }
        ));

        // afterMCPExecution / postToolUse → ToolEnd(Ok).
        match parse(&env_for("cursor", "afterMCPExecution", serde_json::json!({ "success": true }))) {
            AgentEvent::ToolEnd { status, .. } => assert_eq!(status, ToolStatus::Ok),
            other => panic!("expected ToolEnd, got {other:?}"),
        }
        assert!(matches!(
            parse(&env_for("cursor", "postToolUse", Value::Null)),
            AgentEvent::ToolEnd { status: ToolStatus::Ok, .. }
        ));

        // postToolUseFailure → ToolEnd(Failed) carrying the error_message.
        match parse(&env_for("cursor", "postToolUseFailure", serde_json::json!({ "error_message": "timed out" }))) {
            AgentEvent::ToolEnd { status, result, .. } => {
                assert_eq!(status, ToolStatus::Failed);
                assert_eq!(result.as_deref(), Some("timed out"));
            }
            other => panic!("expected ToolEnd, got {other:?}"),
        }

        assert_eq!(
            parse(&env_for("cursor", "afterAgentResponse", serde_json::json!({ "text": "all set" }))),
            AgentEvent::AgentMessage { text: "all set".into() }
        );
        // afterAgentThought → AgentMessage tagged with the 💭 prefix.
        assert_eq!(
            parse(&env_for("cursor", "afterAgentThought", serde_json::json!({ "text": "let me think" }))),
            AgentEvent::AgentMessage { text: "💭 let me think".into() }
        );

        assert_eq!(
            parse(&env_for("cursor", "beforeSubmitPrompt", serde_json::json!({ "prompt": "go" }))),
            AgentEvent::UserPrompt { text: "go".into() }
        );
        assert_eq!(parse(&env_for("cursor", "stop", Value::Null)), AgentEvent::TurnEnd);
        assert_eq!(
            parse(&env_for("cursor", "sessionStart", serde_json::json!({ "source": "cli" }))),
            AgentEvent::SessionStart { source: Some("cli".into()) }
        );
        assert_eq!(
            parse(&env_for("cursor", "sessionEnd", serde_json::json!({ "reason": "quit" }))),
            AgentEvent::SessionEnd { reason: Some("quit".into()) }
        );
    }

    #[test]
    fn parses_gemini_events() {
        // Gemini's own event names: BeforeTool/AfterTool → tool lifecycle, BeforeAgent
        // → SessionStart, AfterAgent → TurnEnd, AfterModel → AgentMessage, Notification
        // stays generic (gemini has NO permission ask), SessionStart/SessionEnd map.
        let before = env_for("gemini", "BeforeTool", serde_json::json!({ "tool_name": "run_shell_command", "tool_input": { "command": "ls" } }));
        match parse(&before) {
            AgentEvent::ToolStart { tool, input } => {
                assert_eq!(tool, "run_shell_command");
                assert_eq!(input.command.as_deref(), Some("ls"));
            }
            other => panic!("expected ToolStart, got {other:?}"),
        }

        match parse(&env_for("gemini", "AfterTool", serde_json::json!({ "tool_name": "write_file", "tool_response": { "success": true } }))) {
            AgentEvent::ToolEnd { tool, status, .. } => {
                assert_eq!(tool, "write_file");
                assert_eq!(status, ToolStatus::Ok);
            }
            other => panic!("expected ToolEnd, got {other:?}"),
        }

        // A gemini block that surfaces a denial on the result → ToolEnd(Denied)
        // (the only way a gemini denial is detectable — no permission event).
        match parse(&env_for("gemini", "AfterTool", serde_json::json!({ "tool_name": "run_shell_command", "tool_response": { "decision": "deny" } }))) {
            AgentEvent::ToolEnd { status, .. } => assert_eq!(status, ToolStatus::Denied),
            other => panic!("expected ToolEnd, got {other:?}"),
        }

        assert_eq!(
            parse(&env_for("gemini", "BeforeAgent", serde_json::json!({ "source": "interactive" }))),
            AgentEvent::SessionStart { source: Some("interactive".into()) }
        );
        assert_eq!(parse(&env_for("gemini", "AfterAgent", Value::Null)), AgentEvent::TurnEnd);
        assert_eq!(
            parse(&env_for("gemini", "AfterModel", serde_json::json!({ "last_assistant_message": "here you go" }))),
            AgentEvent::AgentMessage { text: "here you go".into() }
        );

        // Gemini Notification stays a generic notification — never promoted to a
        // permission ask even if it somehow carried a permission_prompt type.
        match parse(&env_for("gemini", "Notification", serde_json::json!({ "type": "permission_prompt", "message": "heads up" }))) {
            AgentEvent::Notification { kind, message } => {
                assert_eq!(kind.as_deref(), Some("permission_prompt"));
                assert_eq!(message, "heads up");
            }
            other => panic!("expected generic Notification for gemini, got {other:?}"),
        }

        assert_eq!(
            parse(&env_for("gemini", "SessionStart", serde_json::json!({ "source": "startup" }))),
            AgentEvent::SessionStart { source: Some("startup".into()) }
        );
        assert_eq!(
            parse(&env_for("gemini", "SessionEnd", serde_json::json!({ "reason": "done" }))),
            AgentEvent::SessionEnd { reason: Some("done".into()) }
        );
    }

    #[test]
    fn parses_post_tool_use_failure_event_into_failed_tool_end() {
        // Claude's dedicated PostToolUseFailure event → ToolEnd(Failed) with the
        // error message as the result (distinct from a PostToolUse whose result
        // happens to carry an error).
        let e = env(
            "PostToolUseFailure",
            serde_json::json!({ "tool_name": "Bash", "error_message": "exit 127" }),
        );
        match parse(&e) {
            AgentEvent::ToolEnd { tool, status, result } => {
                assert_eq!(tool, "Bash");
                assert_eq!(status, ToolStatus::Failed);
                assert_eq!(result.as_deref(), Some("exit 127"));
            }
            other => panic!("expected ToolEnd, got {other:?}"),
        }
    }

    // --- AskUserQuestion: surface the real ask (the user's complaint) --------

    #[test]
    fn ask_user_question_summary_pulls_question_and_header() {
        // Claude's AskUserQuestion tool input: questions[].{question,header,options}.
        // The summary must surface the actual question + header chip, not the bare
        // tool name. (Schema: code.claude.com/docs/en/agent-sdk/user-input.)
        let input = parse_tool_input(&serde_json::json!({
            "tool_input": {
                "questions": [
                    {
                        "question": "How should I format the output?",
                        "header": "Format",
                        "options": [
                            { "label": "Summary", "description": "Brief" },
                            { "label": "Detailed", "description": "Full" }
                        ],
                        "multiSelect": false
                    },
                    {
                        "question": "Which sections?",
                        "header": "Sections",
                        "options": [{ "label": "Intro", "description": "x" }],
                        "multiSelect": true
                    }
                ]
            }
        }));
        assert_eq!(
            input.summary().as_deref(),
            Some("[Format] How should I format the output? (+1 more)")
        );
        // …and the structured questions are parsed (for the permission card), each
        // option carrying BOTH its label and its description.
        let qs = input.questions();
        assert_eq!(qs.len(), 2);
        assert_eq!(qs[0].question, "How should I format the output?");
        assert_eq!(qs[0].header.as_deref(), Some("Format"));
        assert_eq!(
            qs[0].options,
            vec![
                QuestionOption { label: "Summary".into(), description: "Brief".into() },
                QuestionOption { label: "Detailed".into(), description: "Full".into() },
            ]
        );
        assert_eq!(
            qs[1].options,
            vec![QuestionOption { label: "Intro".into(), description: "x".into() }]
        );
        // …and each question carries its `multiSelect` flag (false then true).
        assert!(!qs[0].multi_select);
        assert!(qs[1].multi_select);
    }

    #[test]
    fn ask_user_question_multi_select_defaults_false_when_absent_or_non_bool() {
        // `multiSelect` absent → false; a non-bool value → false too (defensive).
        let input = parse_tool_input(&serde_json::json!({
            "tool_input": { "questions": [
                { "question": "Pick one", "options": [{ "label": "A" }] },
                { "question": "Pick many", "options": [{ "label": "B" }], "multiSelect": true },
                { "question": "Bad flag", "options": [{ "label": "C" }], "multiSelect": "yes" }
            ] }
        }));
        let qs = input.questions();
        assert!(!qs[0].multi_select, "absent multiSelect → false");
        assert!(qs[1].multi_select, "multiSelect:true → true");
        assert!(!qs[2].multi_select, "non-bool multiSelect → false");
    }

    #[test]
    fn ask_user_question_tolerates_bare_string_options() {
        // A bare string option (no {label,description} object) → label set, empty
        // description (backward-compatible with agents that send plain strings).
        let input = parse_tool_input(&serde_json::json!({
            "tool_input": { "questions": [{ "question": "Pick one", "options": ["A", "B"] }] }
        }));
        let qs = input.questions();
        assert_eq!(
            qs[0].options,
            vec![
                QuestionOption { label: "A".into(), description: String::new() },
                QuestionOption { label: "B".into(), description: String::new() },
            ]
        );
    }

    #[test]
    fn ask_user_question_single_question_has_no_more_tail() {
        let input = parse_tool_input(&serde_json::json!({
            "tool_input": { "questions": [{ "question": "Proceed?", "header": "Confirm", "options": [] }] }
        }));
        assert_eq!(input.summary().as_deref(), Some("[Confirm] Proceed?"));
    }

    #[test]
    fn ask_user_question_event_surfaces_in_activity_detail() {
        // The end-to-end fix: a PreToolUse for AskUserQuestion → activity_detail()
        // shows the real question, NOT "▸ AskUserQuestion".
        let e = env(
            "PreToolUse",
            serde_json::json!({
                "tool_name": "AskUserQuestion",
                "tool_input": { "questions": [{ "question": "Which DB?", "header": "Database", "options": [{ "label": "Postgres" }] }] }
            }),
        );
        match parse(&e) {
            AgentEvent::ToolStart { tool, input } => {
                assert_eq!(tool, "AskUserQuestion");
                assert_eq!(input.summary().as_deref(), Some("[Database] Which DB?"));
                assert_eq!(
                    AgentEvent::ToolStart { tool, input }.activity_detail().as_deref(),
                    Some("▸ AskUserQuestion: [Database] Which DB?")
                );
            }
            other => panic!("expected ToolStart, got {other:?}"),
        }

        // As a PermissionRequest the ✋ line carries the question too.
        let perm = env(
            "PermissionRequest",
            serde_json::json!({
                "tool_name": "AskUserQuestion",
                "tool_input": { "questions": [{ "question": "Which DB?", "header": "Database", "options": [] }] }
            }),
        );
        assert_eq!(
            parse(&perm).activity_detail().as_deref(),
            Some("✋ AskUserQuestion: [Database] Which DB?")
        );
    }

    #[test]
    fn generic_input_summary_falls_back_to_salient_field() {
        // A tool with no command/file_path/description and no questions → the
        // generic salient-field fallback picks the most telling string.
        let grep = parse_tool_input(&serde_json::json!({ "tool_input": { "pattern": "TODO", "glob": "*.rs" } }));
        assert_eq!(grep.summary().as_deref(), Some("TODO"));

        let fetch = parse_tool_input(&serde_json::json!({ "tool_input": { "url": "https://x.dev" } }));
        assert_eq!(fetch.summary().as_deref(), Some("https://x.dev"));

        // Completely opaque input → no summary.
        let opaque = parse_tool_input(&serde_json::json!({ "tool_input": { "n": 5, "flag": true } }));
        assert_eq!(opaque.summary(), None);
    }

    #[test]
    fn summary_prefers_command_over_questions() {
        // When a command exists it wins over the AskUserQuestion path (hint first).
        let input = parse_tool_input(&serde_json::json!({ "tool_input": { "command": "ls" } }));
        assert_eq!(input.summary().as_deref(), Some("ls"));
    }

    // --- Gemini AfterModel: model text lives in llm_response, not flat fields --

    #[test]
    fn gemini_after_model_reads_llm_response_parts() {
        // Gemini AfterModel's text is nested in llm_response.candidates[].content
        // .parts[] (geminicli.com/docs/hooks/reference) — NOT last_assistant_message.
        let e = env_for(
            "gemini",
            "AfterModel",
            serde_json::json!({
                "llm_response": {
                    "candidates": [
                        { "content": { "parts": [{ "text": "here is " }, { "text": "your answer" }] } }
                    ]
                }
            }),
        );
        assert_eq!(
            parse(&e),
            AgentEvent::AgentMessage { text: "here is your answer".into() }
        );
    }

    // --- Cursor after* result hint (output / result_json) --------------------

    #[test]
    fn cursor_after_shell_surfaces_output_as_result() {
        match parse(&env_for(
            "cursor",
            "afterShellExecution",
            serde_json::json!({ "command": "echo hi", "output": "hi", "duration": 5 }),
        )) {
            AgentEvent::ToolEnd { status, result, .. } => {
                assert_eq!(status, ToolStatus::Ok);
                assert_eq!(result.as_deref(), Some("hi"));
            }
            other => panic!("expected ToolEnd, got {other:?}"),
        }
    }

    #[test]
    fn cursor_after_mcp_surfaces_result_json() {
        match parse(&env_for(
            "cursor",
            "afterMCPExecution",
            serde_json::json!({ "tool_name": "x", "result_json": "{\"ok\":1}" }),
        )) {
            AgentEvent::ToolEnd { result, .. } => {
                assert_eq!(result.as_deref(), Some("{\"ok\":1}"));
            }
            other => panic!("expected ToolEnd, got {other:?}"),
        }
    }

    #[test]
    fn tool_input_keeps_raw_intact() {
        // An un-mapped tool_input field stays reachable via `raw` (never dropped).
        let e = env(
            "PreToolUse",
            serde_json::json!({
                "tool_name": "Bash",
                "tool_input": { "command": "ls", "timeout": 5000, "weird": { "x": 1 } }
            }),
        );
        match parse(&e) {
            AgentEvent::ToolStart { input, .. } => {
                assert_eq!(input.raw["timeout"], serde_json::json!(5000));
                assert_eq!(input.raw["weird"]["x"], serde_json::json!(1));
            }
            other => panic!("expected ToolStart, got {other:?}"),
        }
    }

    // --- activity_detail(): the preview line per variant --------------------

    #[test]
    fn activity_detail_per_variant() {
        let perm = AgentEvent::PermissionRequest {
            tool: "Bash".into(),
            input: ToolInput { command: Some("npm test".into()), ..Default::default() },
            mode: None,
            suggestions: vec![],
        };
        assert_eq!(perm.activity_detail().as_deref(), Some("✋ Bash: npm test"));

        let msg = AgentEvent::AgentMessage { text: "hello there".into() };
        assert_eq!(msg.activity_detail().as_deref(), Some("hello there"));

        let start = AgentEvent::ToolStart {
            tool: "Edit".into(),
            input: ToolInput { file_path: Some("src/foo.rs".into()), ..Default::default() },
        };
        assert_eq!(start.activity_detail().as_deref(), Some("▸ Edit: src/foo.rs"));

        let end_ok = AgentEvent::ToolEnd { tool: "Edit".into(), status: ToolStatus::Ok, result: None };
        assert_eq!(end_ok.activity_detail().as_deref(), Some("Edit ✓"));
        let end_bad = AgentEvent::ToolEnd { tool: "Bash".into(), status: ToolStatus::Failed, result: None };
        assert_eq!(end_bad.activity_detail().as_deref(), Some("Bash ✗"));

        let prompt = AgentEvent::UserPrompt { text: String::new() };
        assert_eq!(prompt.activity_detail().as_deref(), Some("working…"));
        let prompt2 = AgentEvent::UserPrompt { text: "do the thing".into() };
        assert_eq!(prompt2.activity_detail().as_deref(), Some("do the thing"));

        assert_eq!(
            AgentEvent::SessionStart { source: None }.activity_detail().as_deref(),
            Some("● started")
        );
        assert_eq!(AgentEvent::TurnEnd.activity_detail().as_deref(), Some("✓ done"));

        let notif = AgentEvent::Notification { kind: None, message: "heads up".into() };
        assert_eq!(notif.activity_detail().as_deref(), Some("heads up"));

        let err = AgentEvent::Error { kind: None, message: "boom".into() };
        assert_eq!(err.activity_detail().as_deref(), Some("✗ boom"));

        // No-preview variants.
        assert!(AgentEvent::SessionEnd { reason: None }.activity_detail().is_none());
        assert!(AgentEvent::Compact { phase: None }.activity_detail().is_none());
        assert!(AgentEvent::Unknown.activity_detail().is_none());
    }

    // --- kind(): the semantic class per variant (for preview coloring) -------

    #[test]
    fn kind_per_variant() {
        assert_eq!(AgentEvent::UserPrompt { text: "hi".into() }.kind(), "user");
        assert_eq!(AgentEvent::AgentMessage { text: "yo".into() }.kind(), "agent");
        assert_eq!(
            AgentEvent::ToolStart { tool: "Bash".into(), input: ToolInput::default() }.kind(),
            "tool"
        );

        // ToolEnd: Ok → success; Failed/Denied → error.
        assert_eq!(
            AgentEvent::ToolEnd { tool: "Edit".into(), status: ToolStatus::Ok, result: None }.kind(),
            "success"
        );
        assert_eq!(
            AgentEvent::ToolEnd { tool: "Bash".into(), status: ToolStatus::Failed, result: None }.kind(),
            "error"
        );
        assert_eq!(
            AgentEvent::ToolEnd { tool: "Bash".into(), status: ToolStatus::Denied, result: None }.kind(),
            "error"
        );

        assert_eq!(
            AgentEvent::PermissionRequest {
                tool: "Bash".into(),
                input: ToolInput::default(),
                mode: None,
                suggestions: vec![],
            }
            .kind(),
            "question"
        );

        // Notification: permission/needs-input kinds → question; others → info.
        assert_eq!(
            AgentEvent::Notification { kind: Some("permission_prompt".into()), message: String::new() }.kind(),
            "question"
        );
        assert_eq!(
            AgentEvent::Notification { kind: Some("idle_prompt".into()), message: String::new() }.kind(),
            "question"
        );
        assert_eq!(
            AgentEvent::Notification { kind: Some("info".into()), message: String::new() }.kind(),
            "info"
        );
        assert_eq!(
            AgentEvent::Notification { kind: None, message: String::new() }.kind(),
            "info"
        );

        assert_eq!(AgentEvent::TurnEnd.kind(), "success");
        assert_eq!(AgentEvent::SessionStart { source: None }.kind(), "info");
        assert_eq!(AgentEvent::SessionEnd { reason: None }.kind(), "info");
        assert_eq!(AgentEvent::Compact { phase: None }.kind(), "info");
        assert_eq!(AgentEvent::Error { kind: None, message: "boom".into() }.kind(), "error");
        assert_eq!(AgentEvent::Unknown.kind(), "info");
    }

    // --- read_request ------------------------------------------------------

    #[test]
    fn reads_an_http_post_with_auth_and_body() {
        let body = r#"{"agentId":"a1","tool":"claude","event":"Notification","payload":null}"#;
        let raw = format!(
            "POST / HTTP/1.1\r\nHost: x\r\nAuthorization: Bearer secret\r\nContent-Length: {}\r\n\r\n{}",
            body.len(),
            body
        );
        let mut c = Cursor::new(raw.into_bytes());
        let req = read_request(&mut c).unwrap();
        assert_eq!(req.auth.as_deref(), Some("secret"));
        let env: HookEnvelope = serde_json::from_str(&req.body).unwrap();
        assert_eq!(env.agent_id, "a1");
        // A bare Notification (no type) parses to a generic Notification.
        assert!(matches!(parse(&env), AgentEvent::Notification { .. }));
    }
}
