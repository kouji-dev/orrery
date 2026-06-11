//! Windows-aware self-update install step.
//!
//! The MSI is a PER-USER package since the custom WiX template (`wix/main.wxs`):
//! it installs to `%LOCALAPPDATA%\Programs\Orrery` with NO elevation and NO UAC.
//!
//! We reuse the plugin for everything that already works — endpoint/version
//! resolution, download, and the Ed25519 signature check — and only override the
//! final launch. On Windows BOTH installer kinds are handed to the branded
//! updater stub (`orrery-updater.exe`, see `updater-stub/`): the app copies the
//! stub + the verified package to %TEMP%, spawns the stub detached, and exits.
//! The stub shows the design-system banner while installing SILENTLY —
//!   * MSI:  in-process Windows Installer API → real 0→100% progress;
//!   * NSIS: `<setup.exe> /S` → indeterminate bar (NSIS emits no progress) —
//! then relaunches the app (also after a failure, so the user is never left
//! app-less). Installer kind is detected from the downloaded bytes.
//!
//! If the stub cannot be staged (missing resource — e.g. dev builds) we fall
//! back to the plugin's own installer so updates still complete.
//!
//! The two commands mirror the frontend's existing check→install flow, so the
//! launch-screen loop-guard (localStorage) and progress UI stay unchanged.

use std::time::Duration;

use serde::Serialize;
use tauri::{AppHandle, Emitter};
use tauri_plugin_updater::{Updater, UpdaterExt};

/// What a check found, for the settings dialog's update card.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct UpdateInfo {
    pub version: String,
    /// Publish date as reported by the manifest (RFC3339-ish), when present.
    pub date: Option<String>,
    /// Release notes (the manifest's `notes` body), when present.
    pub notes: Option<String>,
}

/// The beta manifest lives next to the stable one in the SAME release repo:
/// `latest.json` → `latest-beta.json` (see the settings plan; the release
/// pipeline that publishes it is a follow-up). Derived from the configured
/// stable URL rather than hard-coded so a repo move stays a one-line
/// `tauri.conf.json` change.
fn beta_url(stable: &str) -> String {
    stable.replace("latest.json", "latest-beta.json")
}

/// First configured updater endpoint from `tauri.conf.json` (the stable URL).
fn configured_endpoint(app: &AppHandle) -> Option<String> {
    app.config()
        .plugins
        .0
        .get("updater")?
        .get("endpoints")?
        .get(0)?
        .as_str()
        .map(str::to_string)
}

/// Build a configured updater. Endpoints + pubkey come from `tauri.conf.json`;
/// a `channel` of `"beta"` overrides the endpoint to the repo's
/// `latest-beta.json` (same pubkey — beta artifacts are signed with the same
/// key). `None`/`"stable"`/anything else keeps the configured stable endpoint,
/// so an unrecognized channel value degrades safely.
fn build_updater(
    app: &AppHandle,
    channel: Option<&str>,
    timeout_ms: Option<u64>,
) -> Result<Updater, String> {
    let mut builder = app.updater_builder();
    if let Some(ms) = timeout_ms {
        builder = builder.timeout(Duration::from_millis(ms));
    }
    if channel.is_some_and(|c| c.eq_ignore_ascii_case("beta")) {
        let stable = configured_endpoint(app).ok_or("no updater endpoint configured")?;
        let beta = tauri::Url::parse(&beta_url(&stable)).map_err(|e| e.to_string())?;
        builder = builder.endpoints(vec![beta]).map_err(|e| e.to_string())?;
    }
    builder.build().map_err(|e| e.to_string())
}

/// Resolve an available update on `channel` (default stable); `None` when up to
/// date. The frontend uses `version` for its loop-guard before committing to an
/// install, and `date`/`notes` for the settings dialog's update card.
#[tauri::command]
pub async fn update_check(
    app: AppHandle,
    channel: Option<String>,
    timeout_ms: Option<u64>,
) -> Result<Option<UpdateInfo>, String> {
    let updater = build_updater(&app, channel.as_deref(), timeout_ms)?;
    let update = updater.check().await.map_err(|e| e.to_string())?;
    let info = update.map(|u| UpdateInfo {
        version: u.version,
        date: u.date.map(|d| d.to_string()),
        notes: u.body,
    });
    log::info!(
        "update_check: channel={} available={:?}",
        channel.as_deref().unwrap_or("stable"),
        info.as_ref().map(|i| &i.version)
    );
    Ok(info)
}

/// Download + install from `channel` (default stable) — both the settings
/// dialog's "Install & relaunch" and the launch-screen auto-update path.
/// On Windows a successful install never returns (hands off + exits).
#[tauri::command]
pub async fn update_install(
    app: AppHandle,
    channel: Option<String>,
    timeout_ms: Option<u64>,
) -> Result<(), String> {
    perform(app, channel, timeout_ms).await
}

/// Download (signature-verified by the plugin) and install the available update,
/// emitting cumulative byte progress on `update://progress`.
///
/// On Windows this does NOT return on success — it hands off to the installer and
/// exits the process; it returns `Err` only on a pre-install failure (no update /
/// network / bad signature), which the caller treats as "no update".
async fn perform(
    app: AppHandle,
    channel: Option<String>,
    timeout_ms: Option<u64>,
) -> Result<(), String> {
    log::info!(
        "update_install: starting (channel={})",
        channel.as_deref().unwrap_or("stable")
    );
    let updater = build_updater(&app, channel.as_deref(), timeout_ms)?;
    let Some(update) = updater.check().await.map_err(|e| e.to_string())? else {
        log::info!("update_install: nothing to install (up to date)");
        return Err("no update available".into());
    };
    log::info!("update_install: installing {}", update.version);

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

    log::info!("update_install: downloaded {} bytes", bytes.len());

    // Download is over — the installer takes over from here. The UI flips from
    // the byte-progress bar to its "installing" state on this event.
    let _ = app.emit("update://phase", "installing");

    // Windows: both MSI and NSIS hand off to the branded updater stub (which
    // exits this process). If the stub can't be staged (dev build, missing
    // resource), fall back to the plugin's installer so the update still lands.
    #[cfg(windows)]
    {
        let kind = if is_msi(&bytes) { "msi" } else { "nsis" };
        match install_via_stub(&app, kind, &update.version, &bytes) {
            Ok(()) => return Ok(()), // unreached on success: the app exits
            Err(e) => log::warn!("update_install: stub unavailable ({e}) → plugin installer"),
        }
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

/// Stage the verified package + the branded updater stub in %TEMP%, spawn the
/// stub detached, and exit the app so the installer can replace our files.
/// The stub (see `updater-stub/`) shows the design-system banner, installs
/// silently (MSI: real progress via the Windows Installer API; NSIS: `/S`),
/// and relaunches the app — including after a failure.
#[cfg(windows)]
fn install_via_stub(app: &AppHandle, kind: &str, version: &str, bytes: &[u8]) -> Result<(), String> {
    use std::os::windows::process::CommandExt;
    use tauri::path::BaseDirectory;
    use tauri::Manager;
    // CREATE_BREAKAWAY_FROM_JOB lets the stub leave our kill-on-close Job Object
    // so it outlives our exit (the job permits this via BREAKAWAY_OK) — the same
    // battle-tested arrangement the old PowerShell relauncher used.
    const CREATE_BREAKAWAY_FROM_JOB: u32 = 0x0100_0000;

    // The stub ships as a bundled resource but must RUN from %TEMP% — the
    // install replaces the whole app directory underneath it.
    let stub_src = app
        .path()
        .resolve("binaries/orrery-updater.exe", BaseDirectory::Resource)
        .map_err(|e| format!("resolve stub resource: {e}"))?;
    if !stub_src.exists() {
        return Err(format!("stub not bundled at {}", stub_src.display()));
    }
    let tmp = std::env::temp_dir();
    let stub = tmp.join("orrery-updater.exe");
    std::fs::copy(&stub_src, &stub).map_err(|e| format!("stage stub: {e}"))?;

    let ext = if kind == "msi" { "msi" } else { "exe" };
    let pkg = tmp.join(format!("Orrery_{version}_update.{ext}"));
    std::fs::write(&pkg, bytes).map_err(|e| format!("write package: {e}"))?;

    let exe = std::env::current_exe().map_err(|e| format!("current exe: {e}"))?;
    let prev = app.package_info().version.to_string();
    let args = stub_args(kind, &pkg, &exe, version, &prev, std::process::id());
    log::info!(
        "install_via_stub: kind={kind} pkg={} stub={} ({} bytes)",
        pkg.display(),
        stub.display(),
        bytes.len()
    );

    // creation_flags REPLACES the helper's default, so NO_WINDOW is restated
    // (harmless for the GUI stub; kept so the flag set stays the proven one).
    crate::core::proc::cmd(&stub)
        .args(&args)
        .creation_flags(crate::core::proc::CREATE_NO_WINDOW | CREATE_BREAKAWAY_FROM_JOB)
        .spawn()
        .map_err(|e| format!("spawn updater stub: {e}"))?;

    log::info!("install_via_stub: stub spawned, exiting app for the installer");
    // Quit gracefully (agent PTYs torn down via the RunEvent handler) so the
    // running exe unlocks and the installer can replace it.
    app.exit(0);
    Ok(())
}

/// The stub's command line — pure so the contract is testable. `--wait-pid` is
/// OUR pid: the stub holds its "preparing…" state until this process is gone.
#[cfg(windows)]
fn stub_args(
    kind: &str,
    package: &std::path::Path,
    app_exe: &std::path::Path,
    version: &str,
    prev: &str,
    pid: u32,
) -> Vec<String> {
    vec![
        "--kind".into(),
        kind.into(),
        "--package".into(),
        package.to_string_lossy().into_owned(),
        "--app".into(),
        app_exe.to_string_lossy().into_owned(),
        "--version".into(),
        version.into(),
        "--prev".into(),
        prev.into(),
        "--wait-pid".into(),
        pid.to_string(),
        "--log".into(),
        std::env::temp_dir()
            .join("orrery-updater.log")
            .to_string_lossy()
            .into_owned(),
    ]
}

#[cfg(test)]
mod channel_tests {
    use super::*;

    #[test]
    fn beta_url_swaps_manifest_name_in_same_repo() {
        assert_eq!(
            beta_url("https://github.com/kouji-dev/orrery-releases/releases/latest/download/latest.json"),
            "https://github.com/kouji-dev/orrery-releases/releases/latest/download/latest-beta.json"
        );
    }

    #[test]
    fn beta_url_without_manifest_name_is_passthrough() {
        // a custom endpoint not named latest.json stays untouched (the updater
        // will just check the same manifest — safe, never a broken URL)
        let url = "https://example.com/updates.json";
        assert_eq!(beta_url(url), url);
    }
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
    fn stub_args_carry_the_full_contract() {
        let args = stub_args(
            "msi",
            Path::new(r"C:\Temp\Orrery_0.3.0_update.msi"),
            Path::new(r"C:\Users\u\AppData\Local\Programs\Orrery\Orrery.exe"),
            "0.3.0",
            "0.2.2",
            4242,
        );
        let s = args.join(" ");
        // never any elevation anywhere in the handoff
        assert!(!s.contains("RunAs"), "no elevation: {s}");
        assert!(!s.contains("msiexec"), "stub uses the MSI API, not msiexec: {s}");
        // flag/value pairs the stub's parser (updater-stub) requires
        for (flag, val) in [
            ("--kind", "msi"),
            ("--package", r"C:\Temp\Orrery_0.3.0_update.msi"),
            ("--app", r"C:\Users\u\AppData\Local\Programs\Orrery\Orrery.exe"),
            ("--version", "0.3.0"),
            ("--prev", "0.2.2"),
            ("--wait-pid", "4242"),
        ] {
            let i = args.iter().position(|a| a == flag).unwrap_or_else(|| panic!("{flag} present"));
            assert_eq!(args[i + 1], val, "{flag} value");
        }
        let i = args.iter().position(|a| a == "--log").expect("--log present");
        assert!(args[i + 1].ends_with("orrery-updater.log"), "log path: {s}");
    }

    #[test]
    fn stub_args_nsis_kind_passes_through() {
        let args = stub_args("nsis", Path::new("p.exe"), Path::new("o.exe"), "1.0.0", "0.9.9", 7);
        let i = args.iter().position(|a| a == "--kind").unwrap();
        assert_eq!(args[i + 1], "nsis");
    }
}
