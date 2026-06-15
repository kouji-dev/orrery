use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};

/// The persisted app settings — the plan's schema, one flat document.
///
/// Every field is `#[serde(default)]`-covered (via the container attribute), so
/// loading tolerates both missing keys (older document) and unknown keys (newer
/// document): forward/backward compatibility without migrations. String enums
/// (`channel`, `update_policy`, the `auto_approve` policies) stay plain strings
/// — the consumers treat anything unrecognized as the safe default, so a value
/// written by a future version can't brick a downgrade.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Settings {
    /// Update channel: "stable" | "beta".
    pub channel: String,
    /// Startup update behavior: "auto" | "notify" | "manual".
    pub update_policy: String,
    /// Spawn-modal prefill: tool id ("claude" | "codex" | …); "" = none saved.
    pub default_tool: String,
    /// Per-tool model prefill (tool id → model id).
    pub tool_model: BTreeMap<String, String>,
    /// Per-tool effort prefill (tool id → effort id).
    pub tool_effort: BTreeMap<String, String>,
    /// Per-tool manual executable path override (tool id → absolute path). Set
    /// when auto-detection on PATH is wrong or a tool was found-but-couldn't-run.
    /// A non-empty entry wins over the PATH lookup for both detection and launch.
    pub tool_paths: BTreeMap<String, String>,
    /// Branch name template; tokens: {name} (worktree slug), {tool}, {date} (MMDD).
    pub branch_template: String,
    /// Absolute dir new worktrees are created under; "" = the built-in app-data root.
    pub worktree_root: String,
    /// Relaunch agents that were running when the app last quit/crashed.
    pub auto_resume: bool,
    /// Per-tool permission policy: "off" | "allowlist" | "everything".
    pub auto_approve: BTreeMap<String, String>,
    /// Permission prompts raise a native toast even when the app is unfocused.
    pub remote_approval: bool,
    /// Master toggle: also fire native OS toasts (off = in-app only).
    pub os_notifications: bool,
    /// Per-event alert toggles (off = the alert is not raised at all).
    pub events: EventToggles,
    /// Play a sound cue when a notification fires.
    pub sound: bool,
    /// "Ping" | "Chime" | "Pop" | "Glass" | "Submarine".
    pub sound_name: String,
    /// 0–100.
    pub volume: u8,
}

/// Which notification kinds are raised at all (see plan: these gate the alert
/// entirely, not just the OS toast).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct EventToggles {
    pub finished: bool,
    pub question: bool,
    pub permission: bool,
    pub error: bool,
}

impl Default for EventToggles {
    fn default() -> Self {
        // every kind audible by default — users opt OUT of alerts, not in
        Self {
            finished: true,
            question: true,
            permission: true,
            error: true,
        }
    }
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            channel: "stable".into(),
            // Updates install themselves by default — the per-user MSI/NSIS need
            // no UAC, so "auto" is safe. (The design contract's original default
            // was "notify"; changed 2026-06-11.) Auto-resume on, remote approval on.
            update_policy: "auto".into(),
            default_tool: String::new(), // spawn modal keeps its own hardcoded default
            tool_model: BTreeMap::new(),
            tool_effort: BTreeMap::new(),
            tool_paths: BTreeMap::new(),
            branch_template: "agent/{name}".into(), // the historical branch shape
            worktree_root: String::new(),           // "" = ctor root (app-data/worktrees)
            auto_resume: true,
            auto_approve: BTreeMap::new(), // absent tool = "off" (tool's own flow)
            remote_approval: true,
            os_notifications: true,
            events: EventToggles::default(),
            sound: true,
            sound_name: "Ping".into(),
            volume: 70,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn serializes_camel_case() {
        let v = serde_json::to_value(Settings::default()).unwrap();
        for key in [
            "updatePolicy",
            "defaultTool",
            "toolModel",
            "toolEffort",
            "toolPaths",
            "branchTemplate",
            "worktreeRoot",
            "autoResume",
            "autoApprove",
            "remoteApproval",
            "osNotifications",
            "soundName",
        ] {
            assert!(v.get(key).is_some(), "camelCase key {key} present: {v}");
        }
        assert!(v["events"]["finished"].as_bool().unwrap());
    }

    #[test]
    fn default_matches_plan_schema() {
        let s = Settings::default();
        assert_eq!(s.channel, "stable");
        assert_eq!(s.update_policy, "auto");
        assert_eq!(s.branch_template, "agent/{name}");
        assert_eq!(s.worktree_root, "", "empty = fall back to the ctor root");
        assert!(s.auto_resume);
        assert!(s.auto_approve.is_empty(), "no tool defaults to bypassing");
        assert!(s.os_notifications);
        assert_eq!(s.sound_name, "Ping");
        assert_eq!(s.volume, 70);
    }

    // Forward-compat: a document written by a NEWER app version (unknown keys)
    // and an OLDER one (missing keys) must both load, defaults filling the gaps.
    #[test]
    fn deserialize_tolerates_unknown_and_missing_keys() {
        let s: Settings =
            serde_json::from_str(r#"{"channel":"beta","someFutureKey":{"x":1}}"#).unwrap();
        assert_eq!(s.channel, "beta");
        assert_eq!(s.update_policy, "auto", "missing key takes the default");
        assert_eq!(s.events, EventToggles::default());
    }

    #[test]
    fn round_trips_through_json() {
        let mut s = Settings::default();
        s.channel = "beta".into();
        s.auto_approve.insert("claude".into(), "everything".into());
        s.events.error = false;
        let back: Settings =
            serde_json::from_str(&serde_json::to_string(&s).unwrap()).unwrap();
        assert_eq!(back, s);
    }
}
