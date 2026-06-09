//! Windows-aware self-update install step.
//!
//! Tauri's bundled updater installs an MSI by launching `msiexec` with the
//! ShellExecute verb `"open"` (which does NOT force elevation) in `/passive` mode,
//! then exits. For a PER-MACHINE MSI — Orrery installs to `C:\Program Files` — that
//! upgrade requires admin rights; when the implicit UAC step doesn't complete the
//! new version never lands, so the app re-offers the same update every boot. That's
//! the MSI update "loop" (see the updater-release invariants).
//!
//! We reuse the plugin for everything that already works — endpoint/version
//! resolution, download, and the Ed25519 signature check — and only override the
//! final launch, branching on how the running app was installed (detected from the
//! bytes the updater actually fetched):
//!   * NSIS (per-user): no elevation needed, so we defer to `Update::install` — the
//!     plugin installs silently and relaunches via NSIS `/R`.
//!   * MSI (per-machine, Windows): we hand the verified `.msi` to a detached,
//!     NON-elevated PowerShell that elevates ONLY `msiexec` (one UAC consent), runs
//!     the major upgrade `/quiet`, waits for it, then relaunches the app at normal
//!     integrity. `AUTOLAUNCHAPP` is intentionally NOT used — under the elevated
//!     msiexec it would relaunch the app elevated, which we don't want.
//!
//! The two commands mirror the frontend's existing check→install flow, so the
//! launch-screen loop-guard (localStorage) and progress UI stay unchanged.

use std::time::Duration;

use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::{Updater, UpdaterExt};

/// Build a configured updater — endpoints + pubkey come from `tauri.conf.json` —
/// honoring the caller's check timeout when given.
fn build_updater(app: &AppHandle, timeout_ms: Option<u64>) -> Result<Updater, String> {
    let mut builder = app.updater_builder();
    if let Some(ms) = timeout_ms {
        builder = builder.timeout(Duration::from_millis(ms));
    }
    builder.build().map_err(|e| e.to_string())
}

/// Resolve an available update; return its version (or `None` when up to date). The
/// frontend uses this version for its loop-guard before committing to an install.
#[tauri::command]
pub async fn update_check(
    app: AppHandle,
    timeout_ms: Option<u64>,
) -> Result<Option<String>, String> {
    let updater = build_updater(&app, timeout_ms)?;
    let update = updater.check().await.map_err(|e| e.to_string())?;
    let version = update.map(|u| u.version);
    log::info!("update_check: available={version:?}");
    Ok(version)
}

/// Download (signature-verified by the plugin) and install the available update,
/// emitting cumulative byte progress on `update://progress`.
///
/// On Windows this does NOT return on success — it hands off to the installer and
/// exits the process; it returns `Err` only on a pre-install failure (no update /
/// network / bad signature), which the caller treats as "no update".
#[tauri::command]
pub async fn update_perform(app: AppHandle, timeout_ms: Option<u64>) -> Result<(), String> {
    log::info!("update_perform: starting");
    let updater = build_updater(&app, timeout_ms)?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        log::info!("update_perform: nothing to install (up to date)");
        return Err("no update available".into());
    };
    log::info!("update_perform: installing {}", update.version);

    let mut downloaded: u64 = 0;
    let bytes = update
        .download(
            |chunk, total| {
                downloaded += chunk as u64;
                let _ = app.emit(
                    "update://progress",
                    serde_json::json!({ "downloaded": downloaded, "total": total }),
                );
            },
            || {},
        )
        .await
        .map_err(|e| e.to_string())?;

    log::info!("update_perform: downloaded {} bytes", bytes.len());

    // Per-machine MSI on Windows takes our elevated path (which exits the process);
    // everything else (per-user NSIS, non-Windows) uses the plugin's own installer.
    #[cfg(windows)]
    {
        if is_msi(&bytes) {
            log::info!("update_perform: per-machine MSI → elevated install");
            return install_msi_elevated(&app, &update.version, &bytes);
        }
        log::info!("update_perform: not an MSI → plugin installer (NSIS/per-user)");
    }

    update.install(&bytes).map_err(|e| e.to_string())?;
    Ok(())
}

/// MSI files are OLE/CFBF compound documents — magic bytes `D0 CF 11 E0 A1 B1 1A E1`.
/// An NSIS `-setup.exe` is a PE image (`MZ`), so this cleanly tells us which installer
/// the updater fetched for THIS install (the plugin picks the platform key by install
/// type) without trusting the file name.
#[cfg(windows)]
fn is_msi(bytes: &[u8]) -> bool {
    bytes.starts_with(&[0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1])
}

/// Write the verified MSI to a temp file and hand off to a detached PowerShell that
/// elevates msiexec (one UAC consent), upgrades `/quiet`, waits, then relaunches the
/// app non-elevated — then quit so msiexec can replace our (locked) files.
#[cfg(windows)]
fn install_msi_elevated(app: &AppHandle, version: &str, bytes: &[u8]) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    // CREATE_NO_WINDOW: the relauncher runs with a HIDDEN console — no popup during
    // the install. (History: the first attempt used DETACHED_PROCESS|CREATE_NO_WINDOW
    // and "silently failed"; the fix blamed the missing console and went visible via
    // CREATE_NEW_CONSOLE. Re-tested empirically 2026-06-10: the actual killers were
    // DETACHED_PROCESS — PowerShell dies at startup with NO console at all, hidden
    // is fine — and the kill-on-close Job Object reaping the relauncher on app.exit.
    // A CREATE_NO_WINDOW PowerShell raises UAC and completes the elevated install.)
    // CREATE_BREAKAWAY_FROM_JOB lets it leave our kill-on-close Job Object so it
    // outlives our exit (the job permits this via JOB_OBJECT_LIMIT_BREAKAWAY_OK).
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

    let msi = std::env::temp_dir().join(format!("Orrery_{version}_update.msi"));
    std::fs::write(&msi, bytes).map_err(|e| format!("write msi: {e}"))?;
    let exe = std::env::current_exe().map_err(|e| format!("current exe: {e}"))?;
    log::info!(
        "install_msi_elevated: msi={} exe={} ({} bytes) — spawning elevated relauncher",
        msi.display(),
        exe.display(),
        bytes.len()
    );

    // creation_flags REPLACES the helper's default, so NO_WINDOW must be restated.
    crate::core::proc::cmd("powershell.exe")
        .args([
            "-NoProfile",
            "-ExecutionPolicy",
            "Bypass",
            "-Command",
            &relaunch_script(&msi, &exe),
        ])
        .creation_flags(crate::core::proc::CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB)
        .spawn()
        .map_err(|e| format!("spawn updater shell: {e}"))?;

    log::info!("install_msi_elevated: relauncher spawned, exiting app for msiexec");
    // Quit gracefully (agent PTYs torn down via the RunEvent handler) so the running
    // exe unlocks and msiexec can replace it.
    app.exit(0);
    Ok(())
}

/// The detached-PowerShell program. Single-quoted PS literals keep backslash paths
/// intact; embedded single quotes are doubled. `-Verb RunAs` raises ONE UAC consent
/// for msiexec only; `-Wait` blocks until the upgrade finishes; the final
/// `Start-Process` relaunches the app at this (non-elevated) integrity. A cancelled
/// UAC is swallowed so the app still relaunches (old version) instead of vanishing.
#[cfg(windows)]
fn relaunch_script(msi: &std::path::Path, exe: &std::path::Path) -> String {
    let log = std::env::temp_dir().join("orrery-relaunch.log");
    let log = log.to_string_lossy().replace('\'', "''");
    let msi = msi.to_string_lossy().replace('\'', "''");
    let exe = exe.to_string_lossy().replace('\'', "''");
    // Each step appends to orrery-relaunch.log so a failed update is diagnosable:
    // we see the runas exit code (0 = installed) or the cancellation/error, then the
    // relaunch. -PassThru -Wait gives us msiexec's exit code.
    format!(
        "$ErrorActionPreference='SilentlyContinue'; $l='{log}'; \
         \"$(Get-Date -Format o) start\" | Out-File -Append -Encoding utf8 $l; \
         try {{ $p = Start-Process 'msiexec.exe' -Verb RunAs -Wait -PassThru -ArgumentList '/i \"{msi}\" /quiet'; \
                \"$(Get-Date -Format o) msiexec exit=$($p.ExitCode)\" | Out-File -Append -Encoding utf8 $l }} \
         catch {{ \"$(Get-Date -Format o) runas error: $($_.Exception.Message)\" | Out-File -Append -Encoding utf8 $l }}; \
         \"$(Get-Date -Format o) relaunch\" | Out-File -Append -Encoding utf8 $l; \
         Start-Process -FilePath '{exe}'"
    )
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;
    use std::path::Path;

    #[test]
    fn is_msi_distinguishes_compound_file_from_pe() {
        // CFBF (MSI) magic vs a PE (`MZ`, an NSIS -setup.exe), plus edge inputs.
        assert!(is_msi(&[
            0xD0, 0xCF, 0x11, 0xE0, 0xA1, 0xB1, 0x1A, 0xE1, 0x00
        ]));
        assert!(!is_msi(b"MZ\x90\x00\x03"));
        assert!(!is_msi(&[]));
        assert!(!is_msi(&[0xD0, 0xCF, 0x11])); // truncated, fewer than the 8 magic bytes
    }

    #[test]
    fn relaunch_script_elevates_quiet_waits_then_relaunches() {
        let s = relaunch_script(
            Path::new(r"C:\Temp\Orrery_0.1.7_update.msi"),
            Path::new(r"C:\Program Files\Orrery\Orrery.exe"),
        );
        // one elevated, quiet, awaited msiexec major-upgrade
        assert!(s.contains("-Verb RunAs"), "elevates msiexec: {s}");
        assert!(s.contains("/quiet"), "silent install: {s}");
        assert!(s.contains("-Wait"), "waits for install to finish: {s}");
        assert!(s.contains("-PassThru"), "captures msiexec exit code: {s}");
        assert!(s.contains("Out-File"), "logs each step for diagnosis: {s}");
        assert!(
            s.contains("Orrery_0.1.7_update.msi"),
            "msi path present: {s}"
        );
        // relaunch happens AFTER the elevate block and must NOT ride AUTOLAUNCHAPP
        assert!(
            !s.contains("AUTOLAUNCHAPP"),
            "must not elevate-relaunch: {s}"
        );
        let relaunch_at = s
            .rfind("Start-Process -FilePath")
            .expect("relaunch present");
        let elevate_at = s.find("-Verb RunAs").unwrap();
        assert!(
            relaunch_at > elevate_at,
            "relaunch comes after elevate: {s}"
        );
        assert!(
            s.contains(r"C:\Program Files\Orrery\Orrery.exe"),
            "relaunch exe: {s}"
        );
    }

    #[test]
    fn relaunch_script_escapes_single_quotes_for_ps_literal() {
        let s = relaunch_script(Path::new(r"C:\a'b\x.msi"), Path::new(r"C:\a'b\Orrery.exe"));
        assert!(
            s.contains("a''b"),
            "single quote doubled for PS literal: {s}"
        );
    }
}
