//! Windows Defender exclusion for the worktree root — the one lever that makes
//! worktree creation fast on Windows, applied automatically at startup.
//!
//! Measured on a 7.8k-file project: with real-time scanning of every new file,
//! a gix checkout on 20 threads takes ~13 s; with the writing process or the
//! target folder excluded, 1.8 s. Defender's exclusion list is admin-only to
//! READ and to WRITE, so a per-user app cannot check it silently: the
//! "check, and add if missing" happens inside ONE elevated PowerShell run
//! (`Add-MpPreference` is idempotent, and the same elevated shell verifies the
//! path is listed afterwards). Elevation means a UAC prompt — the only part
//! of this the user ever sees.
//!
//! Because the list cannot be read back unprivileged, the app records what it
//! did (applied / declined / failed / not applicable) with the root and the
//! app version it applied to. Startup re-runs the elevated step when there is
//! no record, when the worktree root moved, or — for a refused prompt or a
//! failure — once per new app version, so a user who dismisses UAC is not
//! nagged on every launch. Existing installs get the same treatment on their
//! first launch of this version (the migration case). Nothing runs on other
//! platforms.

use serde::{Deserialize, Serialize};

use crate::agents::service::AgentService;
use crate::core::errors::{AppError, AppResult};
use crate::settings::SettingsService;

/// Settings-table key holding the [`Record`] (its own key: this is app
/// bookkeeping, not a user preference, so it stays out of `Settings`).
const KV_KEY: &str = "defender_exclusion";

/// What the app last did, persisted.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "camelCase", default)]
struct Record {
    /// "applied" | "declined" | "failed" | "unsupported"
    state: String,
    /// The worktree root the state refers to.
    root: String,
    /// App version that produced the record (refusals/failures retry per version).
    version: String,
    at_ms: u64,
    error: Option<String>,
}

/// Does startup need the elevated step? Pure, so the policy is testable:
/// - never asked → yes;
/// - applied to THIS root → no;
/// - applied to another root (the setting moved) → yes;
/// - declined / failed / not applicable → only once per new app version.
fn should_run(record: Option<&Record>, root: &str, version: &str) -> bool {
    match record {
        None => true,
        Some(r) if r.state == "applied" => r.root != root,
        Some(r) => r.root != root || r.version != version,
    }
}

fn now_ms() -> u64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_millis() as u64)
        .unwrap_or(0)
}

fn load(settings: &SettingsService) -> Option<Record> {
    settings
        .get_kv(KV_KEY)
        .ok()
        .flatten()
        .and_then(|s| serde_json::from_str(&s).ok())
}

fn store(settings: &SettingsService, rec: &Record) -> AppResult<()> {
    let json = serde_json::to_string(rec)
        .map_err(|e| AppError::Other(format!("defender record encode: {e}")))?;
    settings.set_kv(KV_KEY, &json)
}

/// The elevated script. Exit codes are the protocol with [`run_elevated`]:
/// 0 applied+verified · 2 added but not listed afterwards (tamper protection,
/// policy) · 3 Defender is not the active real-time scanner (another AV, or
/// passive mode) — nothing to exclude.
pub fn script(root: &str) -> String {
    let quoted = root.replace('\'', "''");
    format!(
        "$ErrorActionPreference = 'Stop'\n\
         try {{ $s = Get-MpComputerStatus }} catch {{ exit 3 }}\n\
         if (-not $s.RealTimeProtectionEnabled) {{ exit 3 }}\n\
         Add-MpPreference -ExclusionPath '{quoted}'\n\
         $list = @((Get-MpPreference).ExclusionPath)\n\
         if ($list -contains '{quoted}') {{ exit 0 }} else {{ exit 2 }}\n"
    )
}

/// PowerShell's `-EncodedCommand` wants base64 of UTF-16LE — the one encoding
/// that survives every quoting layer between us and the elevated shell.
pub fn encoded_command(script: &str) -> String {
    use base64::Engine as _;
    let utf16: Vec<u8> = script
        .encode_utf16()
        .flat_map(|u| u.to_le_bytes())
        .collect();
    base64::engine::general_purpose::STANDARD.encode(utf16)
}

/// What the elevated run came back with.
#[derive(Debug, PartialEq, Eq)]
enum Outcome {
    Applied,
    Declined,
    NotApplicable,
    Failed(String),
}

/// The non-elevated launcher: `Start-Process -Verb RunAs` is what raises the
/// UAC prompt; `-Wait -PassThru` gives us the elevated shell's exit code.
/// A refused prompt throws ("The operation was canceled by the user") → 10;
/// any other launch failure → 11.
fn launcher(encoded: &str) -> String {
    format!(
        "try {{ \
           $p = Start-Process -FilePath 'powershell.exe' -Verb RunAs -Wait -PassThru -WindowStyle Hidden \
             -ArgumentList @('-NoProfile','-NonInteractive','-ExecutionPolicy','Bypass','-EncodedCommand','{encoded}'); \
           exit $p.ExitCode \
         }} catch {{ \
           if ($_.Exception.Message -match 'cancel') {{ exit 10 }} else {{ [Console]::Error.WriteLine($_.Exception.Message); exit 11 }} \
         }}"
    )
}

fn outcome_from(code: Option<i32>, stderr: &str) -> Outcome {
    match code {
        Some(0) => Outcome::Applied,
        Some(10) => Outcome::Declined,
        Some(3) => Outcome::NotApplicable,
        Some(2) => Outcome::Failed(
            "the exclusion was not listed after adding it — tamper protection or a policy may block changes".into(),
        ),
        Some(c) => Outcome::Failed(format!(
            "elevated PowerShell exited with {c}{}",
            if stderr.trim().is_empty() {
                String::new()
            } else {
                format!(": {}", stderr.trim())
            }
        )),
        None => Outcome::Failed("elevated PowerShell was terminated".into()),
    }
}

#[cfg(windows)]
fn run_elevated(root: &str) -> Outcome {
    let out = crate::core::proc::cmd("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &launcher(&encoded_command(&script(root))),
        ])
        .output();
    match out {
        Ok(o) => outcome_from(o.status.code(), &String::from_utf8_lossy(&o.stderr)),
        Err(e) => Outcome::Failed(format!("powershell: {e}")),
    }
}

#[cfg(not(windows))]
fn run_elevated(_root: &str) -> Outcome {
    Outcome::NotApplicable
}

fn effective_root(agents: &AgentService) -> String {
    let root = agents.worktree_root_effective();
    let _ = std::fs::create_dir_all(&root); // the exclusion should name a real folder
    root.to_string_lossy().to_string()
}

fn record_for(outcome: &Outcome, root: &str, version: &str) -> Record {
    let (state, error) = match outcome {
        Outcome::Applied => ("applied", None),
        Outcome::Declined => ("declined", None),
        Outcome::NotApplicable => ("unsupported", None),
        Outcome::Failed(e) => ("failed", Some(e.clone())),
    };
    Record {
        state: state.into(),
        root: root.into(),
        version: version.into(),
        at_ms: now_ms(),
        error,
    }
}

/// Startup hook: apply the exclusion for the current worktree root when the
/// policy says so (see [`should_run`]). Runs on its own thread after a short
/// delay so the window is up before the UAC prompt, and never blocks setup.
pub fn ensure_on_startup(agents: AgentService, settings: SettingsService, version: String) {
    if !cfg!(windows) {
        return;
    }
    let spawned = std::thread::Builder::new()
        .name("defender-exclusion".into())
        .spawn(move || {
            std::thread::sleep(std::time::Duration::from_secs(3));
            let root = effective_root(&agents);
            let record = load(&settings);
            if !should_run(record.as_ref(), &root, &version) {
                return;
            }
            let outcome = crate::perf::timed("defender_apply", || run_elevated(&root));
            log::info!("defender exclusion for {root}: {outcome:?}");
            if let Err(e) = store(&settings, &record_for(&outcome, &root, &version)) {
                log::warn!("defender exclusion record not saved: {e}");
            }
        });
    if let Err(e) = spawned {
        log::warn!("defender-exclusion thread: {e}");
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn rec(state: &str, root: &str, version: &str) -> Record {
        Record {
            state: state.into(),
            root: root.into(),
            version: version.into(),
            at_ms: 1,
            error: None,
        }
    }

    #[test]
    fn runs_when_never_asked_and_stops_once_applied_to_this_root() {
        assert!(should_run(None, r"C:\wt", "1.0"));
        assert!(!should_run(Some(&rec("applied", r"C:\wt", "1.0")), r"C:\wt", "1.0"));
        // an applied record survives app upgrades — the exclusion is still there
        assert!(!should_run(Some(&rec("applied", r"C:\wt", "1.0")), r"C:\wt", "2.0"));
    }

    #[test]
    fn runs_again_when_the_root_moved() {
        assert!(should_run(Some(&rec("applied", r"C:\old", "1.0")), r"D:\new", "1.0"));
        assert!(should_run(Some(&rec("declined", r"C:\old", "1.0")), r"D:\new", "1.0"));
    }

    #[test]
    fn refusals_and_failures_retry_once_per_app_version() {
        for state in ["declined", "failed", "unsupported"] {
            assert!(
                !should_run(Some(&rec(state, r"C:\wt", "1.0")), r"C:\wt", "1.0"),
                "{state}: same version stays quiet"
            );
            assert!(
                should_run(Some(&rec(state, r"C:\wt", "1.0")), r"C:\wt", "1.1"),
                "{state}: new version retries"
            );
        }
    }

    #[test]
    fn script_quotes_the_root_and_verifies_after_adding() {
        let s = script(r"C:\Users\o'neil\worktrees");
        assert!(s.contains(r"Add-MpPreference -ExclusionPath 'C:\Users\o''neil\worktrees'"));
        assert!(s.contains("-contains 'C:\\Users\\o''neil\\worktrees'"));
        assert!(s.contains("exit 3"), "not-applicable path present");
    }

    #[test]
    fn encoded_command_is_base64_utf16le() {
        use base64::Engine as _;
        let enc = encoded_command("exit 0");
        let bytes = base64::engine::general_purpose::STANDARD.decode(enc).unwrap();
        let units: Vec<u16> = bytes
            .chunks(2)
            .map(|c| u16::from_le_bytes([c[0], c[1]]))
            .collect();
        assert_eq!(String::from_utf16(&units).unwrap(), "exit 0");
    }

    #[test]
    fn launcher_carries_the_encoded_script_and_the_uac_codes() {
        let l = launcher("QUJD");
        assert!(l.contains("-Verb RunAs"));
        assert!(l.contains("'-EncodedCommand','QUJD'"));
        assert!(l.contains("exit 10"), "declined UAC maps to 10");
    }

    #[test]
    fn outcomes_follow_the_exit_code_protocol() {
        assert_eq!(outcome_from(Some(0), ""), Outcome::Applied);
        assert_eq!(outcome_from(Some(10), ""), Outcome::Declined);
        assert_eq!(outcome_from(Some(3), ""), Outcome::NotApplicable);
        assert!(matches!(outcome_from(Some(2), ""), Outcome::Failed(_)));
        assert!(matches!(outcome_from(Some(11), "boom"), Outcome::Failed(m) if m.contains("boom")));
        assert!(matches!(outcome_from(None, ""), Outcome::Failed(_)));
    }

    #[test]
    fn record_round_trips_through_json() {
        let r = record_for(&Outcome::Failed("x".into()), r"C:\wt", "0.22.0");
        let json = serde_json::to_string(&r).unwrap();
        let back: Record = serde_json::from_str(&json).unwrap();
        assert_eq!(back.state, "failed");
        assert_eq!(back.error.as_deref(), Some("x"));
        assert_eq!(back.root, r"C:\wt");
        assert_eq!(back.version, "0.22.0");
    }

}
