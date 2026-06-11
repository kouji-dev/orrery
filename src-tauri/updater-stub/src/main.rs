//! `orrery-updater.exe` — the branded update window.
//!
//! Spawned by the dying app (see `src-tauri/src/update.rs`), runs from %TEMP%
//! so the install can replace the app directory underneath it. Shows the
//! design-system banner (a transparent always-on-top 340×110 frame, epicycle
//! logo, gradient bar) while installing SILENTLY:
//!   * MSI  — in-process via the Windows Installer API with an external-UI
//!     record handler → real 0→100% + live action text. No msiexec, no native UI.
//!   * NSIS — `<setup.exe> /S`, which emits no progress → indeterminate bar.
//! Then relaunches the app. On failure: danger-red state, 5s countdown, and the
//! old (rolled-back) version is relaunched so the user never ends up app-less.
//!
//! UI states are driven by evaluating `window.__*` functions in the webview
//! (banner.html, adapted 1:1 from the Claude Design handoff).

#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod progress;

use std::path::{Path, PathBuf};
use std::process::Command;
use std::sync::OnceLock;
use std::time::Duration;

use tao::dpi::{LogicalSize, PhysicalPosition};
use tao::event::{Event, WindowEvent};
use tao::event_loop::{ControlFlow, EventLoopBuilder, EventLoopProxy};
use tao::window::WindowBuilder;
use wry::WebViewBuilder;

const BANNER_HTML: &str = include_str!("banner.html");
/// Banner frame size (logical px) — the design's 340×110, plus the taller
/// failed state with its action row.
const FRAME_W: f64 = 340.0;
const FRAME_H: f64 = 110.0;
const FRAME_H_FAIL: f64 = 152.0;
/// Restart state lingers this long before the window dissolves.
const RESTART_LINGER_MS: u64 = 1100;
/// Dissolve animation duration before the process exits.
const CLOSE_FADE_MS: u64 = 600;
/// How long we wait for the old app process to exit before installing anyway.
const WAIT_PID_MS: u32 = 15_000;

#[derive(Debug, Clone, PartialEq)]
pub struct Args {
    pub kind: String, // "msi" | "nsis"
    pub package: PathBuf,
    pub app: PathBuf,
    pub version: String,
    pub prev: String,
    pub wait_pid: Option<u32>,
    pub log: PathBuf,
}

/// Tiny flag parser (`--name value` pairs) — no CLI crate needed for 7 flags.
pub fn parse_args(argv: &[String]) -> Result<Args, String> {
    let get = |name: &str| -> Option<String> {
        argv.iter()
            .position(|a| a == name)
            .and_then(|i| argv.get(i + 1))
            .cloned()
    };
    let kind = get("--kind").ok_or("--kind missing")?;
    if kind != "msi" && kind != "nsis" {
        return Err(format!("--kind must be msi|nsis, got {kind}"));
    }
    Ok(Args {
        kind,
        package: PathBuf::from(get("--package").ok_or("--package missing")?),
        app: PathBuf::from(get("--app").ok_or("--app missing")?),
        version: get("--version").unwrap_or_else(|| "?".into()),
        prev: get("--prev").unwrap_or_else(|| "?".into()),
        wait_pid: get("--wait-pid").and_then(|p| p.parse().ok()),
        log: get("--log")
            .map(PathBuf::from)
            .unwrap_or_else(|| std::env::temp_dir().join("orrery-updater.log")),
    })
}

#[derive(Debug)]
enum Ui {
    Installing,
    Progress { pct: f64, action: Option<String> },
    Done,
    Failed(String),
    Quit,
}

static LOG_PATH: OnceLock<PathBuf> = OnceLock::new();

fn log_line(msg: &str) {
    use std::io::Write;
    if let Some(p) = LOG_PATH.get() {
        if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(p) {
            let _ = writeln!(f, "[{:?}] {msg}", std::time::SystemTime::now());
        }
    }
}

fn relaunch(app: &Path) {
    let mut cmd = Command::new(app);
    if let Some(dir) = app.parent() {
        cmd.current_dir(dir);
    }
    match cmd.spawn() {
        Ok(_) => log_line("relaunched app"),
        Err(e) => log_line(&format!("relaunch failed: {e}")),
    }
}

fn open_log() {
    if let Some(p) = LOG_PATH.get() {
        let _ = Command::new("notepad.exe").arg(p).spawn();
    }
}

fn main() {
    let argv: Vec<String> = std::env::args().collect();
    let args = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            // No window yet — a bad invocation can only be diagnosed from the log.
            let _ = std::fs::write(
                std::env::temp_dir().join("orrery-updater.log"),
                format!("bad arguments: {e}\nargv: {argv:?}\n"),
            );
            std::process::exit(2);
        }
    };
    let _ = LOG_PATH.set(args.log.clone());
    log_line(&format!(
        "start kind={} package={} app={} v{} (prev v{})",
        args.kind,
        args.package.display(),
        args.app.display(),
        args.version,
        args.prev
    ));

    let event_loop = EventLoopBuilder::<Ui>::with_user_event().build();
    let proxy = event_loop.create_proxy();

    // Opaque window sized EXACTLY to the banner frame, with native DWM corner
    // rounding (Win11; square on Win10). True per-pixel transparency is a trap
    // here: WebView2's windowed hosting can't alpha-composite against other
    // apps, so a "transparent" window renders white.
    let window = WindowBuilder::new()
        .with_title("Orrery Update")
        .with_decorations(false)
        .with_always_on_top(true)
        .with_resizable(false)
        .with_inner_size(LogicalSize::new(FRAME_W, FRAME_H))
        .build(&event_loop)
        .expect("create window");
    center(&window);
    round_corners(&window);

    let ipc_app = args.app.clone();
    let webview = WebViewBuilder::new()
        .with_html(BANNER_HTML)
        .with_ipc_handler(move |req: wry::http::Request<String>| match req.body().as_str() {
            // Failed state: countdown elapsed or "Relaunch now" — bring the
            // (rolled-back) app up and leave.
            "relaunch" => {
                relaunch(&ipc_app);
                std::process::exit(0);
            }
            "viewlog" => open_log(),
            other => log_line(&format!("unknown ipc: {other}")),
        })
        .build(&window)
        .expect("create webview");

    spawn_worker(args.clone(), proxy.clone());

    let app_path = args.app.clone();
    let package = args.package.clone();
    let version = args.version.clone();
    event_loop.run(move |event, _, control_flow| {
        *control_flow = ControlFlow::Wait;
        match event {
            // It's a viewer, not a controller: the window cannot be closed
            // mid-install. (Failure offers explicit buttons instead.)
            Event::WindowEvent { event: WindowEvent::CloseRequested, .. } => {}
            Event::UserEvent(ui) => {
                let js = match ui {
                    Ui::Installing => match args.kind.as_str() {
                        "msi" => format!("__msi({:?})", version),
                        _ => format!("__nsis({:?})", version),
                    },
                    Ui::Progress { pct, action } => match action {
                        Some(a) => format!("__progress({pct:.1}, {a:?})"),
                        None => format!("__progress({pct:.1}, null)"),
                    },
                    Ui::Done => {
                        let _ = std::fs::remove_file(&package);
                        relaunch(&app_path);
                        let p = proxy.clone();
                        std::thread::spawn(move || {
                            std::thread::sleep(Duration::from_millis(RESTART_LINGER_MS));
                            let _ = p.send_event(Ui::Quit);
                        });
                        "__restart()".into()
                    }
                    Ui::Failed(msg) => {
                        log_line(&format!("install failed: {msg}"));
                        // the failed state adds an action row — give it room
                        window.set_inner_size(LogicalSize::new(FRAME_W, FRAME_H_FAIL));
                        center(&window);
                        format!("__fail({:?})", format!("{msg} — keeping v{}", args.prev))
                    }
                    Ui::Quit => {
                        let _ = webview.evaluate_script("__close()");
                        std::thread::sleep(Duration::from_millis(CLOSE_FADE_MS));
                        // Hide BEFORE exiting — destroying a live WebView2
                        // window can flash its white default background.
                        window.set_visible(false);
                        std::process::exit(0);
                    }
                };
                let _ = webview.evaluate_script(&js);
            }
            _ => {}
        }
    });
}

/// Native rounded corners (Win11's DWM; no-op on Win10 → square corners).
#[cfg(windows)]
fn round_corners(window: &tao::window::Window) {
    use tao::platform::windows::WindowExtWindows;
    use windows_sys::Win32::Graphics::Dwm::DwmSetWindowAttribute;
    const DWMWA_WINDOW_CORNER_PREFERENCE: u32 = 33;
    const DWMWCP_ROUND: i32 = 2;
    unsafe {
        DwmSetWindowAttribute(
            window.hwnd() as _,
            DWMWA_WINDOW_CORNER_PREFERENCE,
            &DWMWCP_ROUND as *const _ as _,
            std::mem::size_of::<i32>() as u32,
        );
    }
}

#[cfg(not(windows))]
fn round_corners(_window: &tao::window::Window) {}

fn center(window: &tao::window::Window) {
    if let Some(mon) = window.current_monitor() {
        let m = mon.size();
        let w = window.outer_size();
        let pos = mon.position();
        window.set_outer_position(PhysicalPosition::new(
            pos.x + ((m.width.saturating_sub(w.width)) / 2) as i32,
            pos.y + ((m.height.saturating_sub(w.height)) / 2) as i32,
        ));
    }
}

/// Prepare → install → done/fail, off the UI thread.
fn spawn_worker(args: Args, proxy: EventLoopProxy<Ui>) {
    std::thread::spawn(move || {
        if let Some(pid) = args.wait_pid {
            wait_for_pid(pid, WAIT_PID_MS);
        }
        // Belt and braces: even after the process is gone, give the OS a beat
        // to release file locks before the installer touches the directory.
        std::thread::sleep(Duration::from_millis(250));
        let _ = proxy.send_event(Ui::Installing);

        let result = match args.kind.as_str() {
            "msi" => install_msi(&args, &proxy),
            _ => install_nsis(&args),
        };
        log_line(&format!("install result: {result:?}"));
        let _ = proxy.send_event(match result {
            Ok(()) => Ui::Done,
            Err(e) => Ui::Failed(e),
        });
    });
}

fn install_nsis(args: &Args) -> Result<(), String> {
    let mut cmd = Command::new(&args.package);
    cmd.arg("/S");
    // Belt and braces: a console-subsystem package must never flash a terminal
    // over the banner. (The real NSIS setup is a GUI exe, but cheap insurance.)
    #[cfg(windows)]
    {
        use std::os::windows::process::CommandExt;
        cmd.creation_flags(0x0800_0000); // CREATE_NO_WINDOW
    }
    let status = cmd
        .status()
        .map_err(|e| format!("installer would not start ({e})"))?;
    if status.success() {
        Ok(())
    } else {
        Err(format!(
            "installer exited with code {}",
            status.code().unwrap_or(-1)
        ))
    }
}

// ---------------- Windows-only plumbing ----------------

#[cfg(windows)]
fn wait_for_pid(pid: u32, timeout_ms: u32) {
    use windows_sys::Win32::Foundation::CloseHandle;
    use windows_sys::Win32::System::Threading::{OpenProcess, WaitForSingleObject};
    const SYNCHRONIZE: u32 = 0x0010_0000;
    unsafe {
        let h = OpenProcess(SYNCHRONIZE, 0, pid);
        if h.is_null() {
            return; // already gone
        }
        WaitForSingleObject(h, timeout_ms);
        CloseHandle(h);
    }
}

#[cfg(not(windows))]
fn wait_for_pid(_pid: u32, _timeout_ms: u32) {}

#[cfg(windows)]
mod msi_engine {
    //! In-process silent MSI install with an external-UI record handler.
    //! `MsiInstallProductW` blocks the worker thread; Windows Installer calls
    //! `handler` with progress/action records which we fold through
    //! [`progress::ProgressModel`] and forward (throttled) to the UI thread.

    use super::{log_line, progress::ProgressModel, Ui};
    use std::ffi::c_void;
    use std::sync::Mutex;
    use tao::event_loop::EventLoopProxy;
    use windows_sys::Win32::System::ApplicationInstallationAndServicing::{
        MsiEnableLogW, MsiInstallProductW, MsiRecordGetInteger, MsiRecordGetStringW,
        MsiSetExternalUIRecord, MsiSetInternalUI, INSTALLLOGMODE_ACTIONDATA,
        INSTALLLOGMODE_ACTIONSTART, INSTALLLOGMODE_ERROR, INSTALLLOGMODE_FATALEXIT,
        INSTALLLOGMODE_INFO, INSTALLLOGMODE_PROGRESS, INSTALLUILEVEL_NONE, MSIHANDLE,
    };

    const IDOK: i32 = 1;
    // INSTALLMESSAGE type lives in the high byte of the callback's message type.
    const MSG_KIND_MASK: u32 = 0xFF00_0000;
    const MSG_ACTIONSTART: u32 = 0x0800_0000;
    const MSG_ACTIONDATA: u32 = 0x0900_0000;
    const MSG_PROGRESS: u32 = 0x0A00_0000;

    struct Shared {
        model: ProgressModel,
        proxy: EventLoopProxy<Ui>,
        last_sent_pct: f64,
        action: Option<String>,
    }
    // The handler is a C callback with a void* context we'd rather not juggle:
    // one install runs per process, so a static slot is simpler and safe.
    static SHARED: Mutex<Option<Shared>> = Mutex::new(None);

    fn wide(s: &std::ffi::OsStr) -> Vec<u16> {
        use std::os::windows::ffi::OsStrExt;
        s.encode_wide().chain(std::iter::once(0)).collect()
    }

    fn record_int(h: MSIHANDLE, field: u32) -> i32 {
        unsafe { MsiRecordGetInteger(h, field) }
    }

    fn record_string(h: MSIHANDLE, field: u32) -> String {
        let mut buf = [0u16; 512];
        let mut len = buf.len() as u32 - 1;
        let rc = unsafe { MsiRecordGetStringW(h, field, buf.as_mut_ptr(), &mut len) };
        if rc != 0 {
            return String::new();
        }
        String::from_utf16_lossy(&buf[..len as usize]).trim().to_string()
    }

    unsafe extern "system" fn handler(_ctx: *mut c_void, msg_type: u32, record: MSIHANDLE) -> i32 {
        let kind = msg_type & MSG_KIND_MASK;
        let mut guard = match SHARED.lock() {
            Ok(g) => g,
            Err(_) => return IDOK,
        };
        let Some(sh) = guard.as_mut() else { return IDOK };
        match kind {
            MSG_PROGRESS => {
                sh.model.on_progress(
                    record_int(record, 1),
                    record_int(record, 2),
                    record_int(record, 3),
                    record_int(record, 4),
                );
            }
            MSG_ACTIONDATA => sh.model.on_action_data(),
            MSG_ACTIONSTART => {
                // field 2 is the human description ("Copying new files"); field 1
                // the raw action name — use whichever is present.
                let desc = record_string(record, 2);
                let name = record_string(record, 1);
                let label = if !desc.is_empty() { desc } else { name };
                if !label.is_empty() {
                    sh.action = Some(label.trim_end_matches('.').to_string());
                    sh.last_sent_pct = -1.0; // force a UI push with the new action
                }
            }
            _ => return IDOK,
        }
        // Throttle: the tick stream is dense; only cross the thread on movement.
        if let Some(pct) = sh.model.percent() {
            if pct - sh.last_sent_pct >= 0.5 || sh.last_sent_pct < 0.0 {
                sh.last_sent_pct = pct;
                let _ = sh.proxy.send_event(Ui::Progress { pct, action: sh.action.take() });
            }
        }
        IDOK
    }

    pub fn run(package: &std::path::Path, msi_log: &std::path::Path, proxy: EventLoopProxy<Ui>) -> Result<(), String> {
        *SHARED.lock().unwrap() = Some(Shared {
            model: ProgressModel::new(),
            proxy,
            last_sent_pct: -1.0,
            action: None,
        });
        let filter = (INSTALLLOGMODE_PROGRESS
            | INSTALLLOGMODE_ACTIONSTART
            | INSTALLLOGMODE_ACTIONDATA
            | INSTALLLOGMODE_FATALEXIT
            | INSTALLLOGMODE_ERROR) as u32;
        let pkg = wide(package.as_os_str());
        let log = wide(msi_log.as_os_str());
        // Per-user package upgrading in place: suppress any reboot ambitions.
        let cmdline = wide(std::ffi::OsStr::new("REBOOT=ReallySuppress"));
        let rc = unsafe {
            MsiSetInternalUI(INSTALLUILEVEL_NONE, std::ptr::null_mut());
            MsiSetExternalUIRecord(Some(handler), filter, std::ptr::null(), None);
            MsiEnableLogW(
                (INSTALLLOGMODE_FATALEXIT | INSTALLLOGMODE_ERROR | INSTALLLOGMODE_INFO) as u32,
                log.as_ptr(),
                0,
            );
            MsiInstallProductW(pkg.as_ptr(), cmdline.as_ptr())
        };
        *SHARED.lock().unwrap() = None;
        log_line(&format!("MsiInstallProduct rc={rc}"));
        match rc {
            0 | 3010 => Ok(()), // success (3010 = success, reboot flagged — suppressed)
            1602 => Err("the installation was cancelled".into()),
            1618 => Err("another installation is already running".into()),
            1619 | 1620 => Err("the update package could not be opened".into()),
            code => Err(format!("installer exited with an error (code {code})")),
        }
    }
}

#[cfg(windows)]
fn install_msi(args: &Args, proxy: &EventLoopProxy<Ui>) -> Result<(), String> {
    let msi_log = std::env::temp_dir().join("orrery-msi-install.log");
    msi_engine::run(&args.package, &msi_log, proxy.clone())
}

#[cfg(not(windows))]
fn install_msi(_args: &Args, _proxy: &EventLoopProxy<Ui>) -> Result<(), String> {
    Err("MSI installs are Windows-only".into())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn argv(s: &[&str]) -> Vec<String> {
        std::iter::once("orrery-updater.exe".to_string())
            .chain(s.iter().map(|x| x.to_string()))
            .collect()
    }

    #[test]
    fn parses_a_full_invocation() {
        let a = parse_args(&argv(&[
            "--kind", "msi",
            "--package", r"C:\t\u.msi",
            "--app", r"C:\p\Orrery.exe",
            "--version", "0.3.0",
            "--prev", "0.2.2",
            "--wait-pid", "4242",
            "--log", r"C:\t\u.log",
        ]))
        .unwrap();
        assert_eq!(a.kind, "msi");
        assert_eq!(a.package, PathBuf::from(r"C:\t\u.msi"));
        assert_eq!(a.app, PathBuf::from(r"C:\p\Orrery.exe"));
        assert_eq!(a.version, "0.3.0");
        assert_eq!(a.prev, "0.2.2");
        assert_eq!(a.wait_pid, Some(4242));
        assert_eq!(a.log, PathBuf::from(r"C:\t\u.log"));
    }

    #[test]
    fn rejects_missing_or_bad_kind() {
        assert!(parse_args(&argv(&["--package", "x"])).is_err());
        assert!(parse_args(&argv(&["--kind", "zip", "--package", "x", "--app", "y"])).is_err());
    }

    #[test]
    fn optional_flags_default() {
        let a = parse_args(&argv(&["--kind", "nsis", "--package", "p.exe", "--app", "o.exe"])).unwrap();
        assert_eq!(a.version, "?");
        assert_eq!(a.wait_pid, None);
        assert!(a.log.ends_with("orrery-updater.log"));
    }
}
