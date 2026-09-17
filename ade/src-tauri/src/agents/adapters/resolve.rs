//! Where detection LOOKS for a coding-agent CLI, and in what order.
//!
//! The old lookup was "split `$PATH`, try `` / `.exe` / `.cmd` / `.bat`, first
//! hit wins". Three real-world gaps made an installed tool report as missing (or,
//! worse, as found-but-unrunnable — the amber "needs path" tile):
//!
//! 1. **A stale process PATH.** A GUI app inherits Explorer's environment block,
//!    frozen when Explorer started. Install claude / pi from a terminal *after*
//!    Orrery is running and the new `PATH` entry simply does not exist for us,
//!    so the tool stays "not found" until the whole SHELL is restarted — not just
//!    the app. [`live_path_dirs`] re-reads the user + machine `Path` values
//!    straight from the registry on every detection pass, so an install lands on
//!    the next re-scan.
//! 2. **The extensionless shim won.** npm writes three files side by side:
//!    `claude` (a `#!/bin/sh` script, for Git Bash), `claude.cmd` and
//!    `claude.ps1`. `""` came first in the extension list, so detection resolved
//!    the POSIX shell script — which `CreateProcessW` cannot launch at all
//!    (os error 193). The launcher papered over it by hopping to a sibling at
//!    spawn time, but the path we REPORTED (and the one the model probe ran) was
//!    the unrunnable one. [`path_exts`] now orders directly-runnable extensions
//!    first and keeps `""` as the last resort, and honours `PATHEXT` so a tool
//!    installed as anything Windows considers executable is found.
//! 3. **PATH was the only place we looked.** Several of these CLIs install to a
//!    well-known directory that their installer adds to the *user's* PATH but
//!    which may be missing from ours for reason 1, or which the user never had on
//!    PATH to begin with (a `--ignore-scripts` npm install, claude's native
//!    `~/.local/bin` installer). [`well_known_dirs`] sweeps those, and each
//!    adapter can name its own via
//!    [`AgentAdapter::extra_dirs`](super::AgentAdapter::extra_dirs).
//!
//! Order matters and is deliberate: PATH (process, then live registry) always
//! wins, because that is what the user's own shell would run. Well-known and
//! adapter-specific directories are a FALLBACK, consulted only when PATH has
//! nothing — detection never prefers a copy the user's terminal wouldn't pick,
//! and [`candidates`] therefore stops the moment the PATH sweep produced a hit.
//! That early exit is not only correctness: the full sweep is ~38 deduped
//! directories x 13 extensions ~= 500 `is_file` calls (~34ms measured), against
//! ~48 for a PATH that answers (~6ms), and detection runs on every settings
//! open.

use std::path::{Path, PathBuf};

/// Executable extensions to try in each directory, most-directly-runnable first.
///
/// On Windows: our own preferred order (`.exe`, `.com`, `.cmd`, `.bat`, `.ps1`)
/// followed by anything else `PATHEXT` lists, and finally `""` — the
/// extensionless npm/POSIX shim, which is a real hit but the worst one, since it
/// needs a sibling hop before it can actually be spawned.
///
/// Everywhere else there is exactly one candidate: the bare name.
pub fn path_exts() -> Vec<String> {
    if !cfg!(windows) {
        return vec![String::new()];
    }
    let preferred = [".exe", ".com", ".cmd", ".bat", ".ps1"];
    let mut exts: Vec<String> = preferred.iter().map(|e| e.to_string()).collect();
    if let Some(pathext) = std::env::var_os("PATHEXT") {
        for raw in pathext.to_string_lossy().split(';') {
            let e = raw.trim().to_ascii_lowercase();
            if e.is_empty() || !e.starts_with('.') {
                continue;
            }
            // `.js` / `.vbs` / `.wsf` are in a default PATHEXT and would resolve
            // a script we cannot spawn directly; they are still valid hits, just
            // ranked after everything we know how to launch.
            if !exts.iter().any(|k| k == &e) {
                exts.push(e);
            }
        }
    }
    exts.push(String::new()); // last resort: the extensionless shim
    exts
}

/// The `Path` entries currently configured for the machine + this user, read
/// live from the registry rather than from our own (possibly hours-old) process
/// environment block. Windows only; `Vec::new()` elsewhere and on any read
/// failure — this is strictly additive to the process PATH, never a replacement.
///
/// `REG_EXPAND_SZ` values (the normal shape for `Path`) come back already
/// expanded: `RegGetValueW` performs the `%SystemRoot%`-style substitution for
/// us unless `RRF_NOEXPAND` is passed, and we deliberately do not pass it.
pub fn live_path_dirs() -> Vec<PathBuf> {
    #[cfg(windows)]
    {
        const MACHINE_ENV: &str =
            r"SYSTEM\CurrentControlSet\Control\Session Manager\Environment";
        let mut out = Vec::new();
        for (root, sub) in [
            (windows_registry::HKEY_CURRENT_USER, "Environment"),
            (windows_registry::HKEY_LOCAL_MACHINE, MACHINE_ENV),
        ] {
            let Some(value) = windows_registry::read_string(root, sub, "Path") else {
                continue;
            };
            out.extend(std::env::split_paths(&value));
        }
        out
    }
    #[cfg(not(windows))]
    {
        Vec::new()
    }
}

/// Minimal `RegGetValueW` wrapper — the one registry read detection needs.
#[cfg(windows)]
mod windows_registry {
    use windows_sys::Win32::Foundation::ERROR_SUCCESS;
    use windows_sys::Win32::System::Registry::{RegGetValueW, HKEY, RRF_RT_REG_EXPAND_SZ, RRF_RT_REG_SZ};

    pub use windows_sys::Win32::System::Registry::{HKEY_CURRENT_USER, HKEY_LOCAL_MACHINE};

    /// UTF-16, NUL-terminated — the shape every `*W` registry entry point wants.
    fn wide(s: &str) -> Vec<u16> {
        s.encode_utf16().chain(std::iter::once(0)).collect()
    }

    /// Read one `REG_SZ` / `REG_EXPAND_SZ` value, with `%VAR%` expansion applied.
    /// `None` for any failure — a missing key is the normal case for a user who
    /// has never set a per-user PATH.
    pub fn read_string(root: HKEY, subkey: &str, name: &str) -> Option<String> {
        let sub = wide(subkey);
        let val = wide(name);
        let flags = RRF_RT_REG_SZ | RRF_RT_REG_EXPAND_SZ;
        // Size probe first: `Path` routinely runs past any fixed buffer we'd pick.
        let mut bytes: u32 = 0;
        // SAFETY: both name pointers are NUL-terminated UTF-16 buffers that
        // outlive the call; passing null data with a valid size-out pointer is
        // the documented way to ask for the required byte count.
        let rc = unsafe {
            RegGetValueW(
                root,
                sub.as_ptr(),
                val.as_ptr(),
                flags,
                std::ptr::null_mut(),
                std::ptr::null_mut(),
                &mut bytes,
            )
        };
        if rc != ERROR_SUCCESS || bytes == 0 {
            return None;
        }
        // Round up to whole u16s; the count includes the terminating NUL.
        let mut buf: Vec<u16> = vec![0; bytes as usize / 2 + 1];
        let mut cap: u32 = (buf.len() * 2) as u32;
        // SAFETY: `buf` has `cap` writable bytes and the key/value pointers are
        // still live; `cap` is updated in place with the bytes actually written.
        let rc = unsafe {
            RegGetValueW(
                root,
                sub.as_ptr(),
                val.as_ptr(),
                flags,
                std::ptr::null_mut(),
                buf.as_mut_ptr().cast(),
                &mut cap,
            )
        };
        if rc != ERROR_SUCCESS {
            return None;
        }
        let len = (cap as usize / 2).min(buf.len());
        let s = &buf[..len];
        let s = match s.iter().position(|&c| c == 0) {
            Some(n) => &s[..n],
            None => s,
        };
        Some(String::from_utf16_lossy(s))
    }
}

/// Package-manager and installer bin directories these CLIs can land in without
/// their parent being on OUR PATH, consulted only after the PATH sweep came up
/// empty (see [`candidates`]).
///
/// Every entry is either DERIVED from the machine or a constant that was checked
/// against a real install. An earlier version of this list was neither: a dozen
/// plausible-looking paths (`~/.volta/bin`, `~/scoop/shims`, `~/.cargo/bin`,
/// `~/go/bin`, `~/.yarn/bin`, `~/bin`, …) copied from convention, none of which
/// any of these five CLIs ships through, and each of which would silently rot as
/// the tools change installers. Guessing widely is not free — every entry is 13
/// more `is_file` calls per tool per detection pass — and a guess that happens to
/// hold a same-named stranger's binary is worse than a miss.
///
/// DERIVED (recomputed per call, so a version-manager switch is picked up):
/// * the directory holding `node` — where npm's global shims land on Windows,
///   and the one entry that covers an nvm-for-windows layout (`C:\nvm4w\nodejs`)
///   with no nvm-specific knowledge at all;
/// * `npm_config_prefix` / `PNPM_HOME` / `BUN_INSTALL`, each read from the
///   environment the package manager itself sets.
///
/// VERIFIED CONSTANTS (a real install on a real machine, not convention):
/// * `%APPDATA%\npm` — npm's documented default Windows prefix, which is where
///   the shims go when node is NOT under a version manager;
/// * `~/.local/bin` — where claude's native installer script writes;
/// * `/usr/local/bin`, `/opt/homebrew/bin`, `/usr/bin` off Windows — a GUI app
///   launched from Finder/a launcher inherits a minimal PATH that routinely
///   lacks all three, which is the same stale-environment failure the module
///   docs open with.
///
/// Deliberately NOT covered: a pnpm or bun install whose own env var is unset.
/// Both installers put their bin dir on the user's PATH, and [`live_path_dirs`]
/// reads that PATH from the registry rather than from our process block — so the
/// case a hardcoded `~/AppData/Local/pnpm` would catch is already caught, one
/// layer up and without the guess.
pub fn well_known_dirs() -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();

    // Derived. `node`'s own directory IS npm's prefix on Windows; elsewhere the
    // shims sit beside it too (`<prefix>/bin`), so the sibling holds in both.
    if let Some(dir) = node_bin_dir() {
        out.push(dir);
    }
    // Derived. npm writes binaries into `<prefix>` on Windows, `<prefix>/bin`
    // elsewhere; pushing both costs one miss and needs no platform branch.
    if let Some(dir) = std::env::var_os("npm_config_prefix").map(PathBuf::from) {
        out.push(dir.join("bin"));
        out.push(dir);
    }
    // Derived. `pnpm setup` exports PNPM_HOME; bun's installer exports
    // BUN_INSTALL, whose binaries live one level down in `bin`.
    if let Some(dir) = std::env::var_os("PNPM_HOME").map(PathBuf::from) {
        out.push(dir);
    }
    if let Some(dir) = std::env::var_os("BUN_INSTALL").map(PathBuf::from) {
        out.push(dir.join("bin"));
    }
    // Verified constants.
    if cfg!(windows) {
        if let Some(dir) = std::env::var_os("APPDATA").map(PathBuf::from) {
            out.push(dir.join("npm"));
        }
    } else {
        for fixed in ["/usr/local/bin", "/opt/homebrew/bin", "/usr/bin"] {
            out.push(PathBuf::from(fixed));
        }
    }
    out.extend(in_home(&[".local", "bin"]));
    out
}

/// The directory `node` resolves to on PATH, if any — the derivation behind the
/// npm-prefix entry of [`well_known_dirs`].
///
/// Scans [`path_dirs`] only, NEVER [`candidates`]: `candidates` consults
/// `well_known_dirs`, so going through it here would recurse forever. The extra
/// sweep is paid only on the fallback branch, i.e. only for a tool the PATH
/// sweep already failed to find.
fn node_bin_dir() -> Option<PathBuf> {
    let exts = path_exts();
    path_dirs().into_iter().find(|dir| {
        exts.iter()
            .any(|ext| dir.join(format!("node{ext}")).is_file())
    })
}

/// The user's home directory, from whichever variable this platform sets.
pub fn home_dir() -> Option<PathBuf> {
    std::env::var_os("USERPROFILE")
        .or_else(|| std::env::var_os("HOME"))
        .map(PathBuf::from)
        .filter(|p| !p.as_os_str().is_empty())
}

/// Build `~/<parts…>` when a home directory is known. The convenience every
/// adapter's [`extra_dirs`](super::AgentAdapter::extra_dirs) is written in terms
/// of.
pub fn in_home(parts: &[&str]) -> Option<PathBuf> {
    let mut p = home_dir()?;
    for part in parts {
        p.push(part);
    }
    Some(p)
}

/// Comparison key for a search directory: trailing separators trimmed, and
/// case-folded on Windows, where two spellings name the same directory. Without
/// it a PATH that lists `C:\bin` and `c:\bin\` would be swept — and every miss
/// in it probed — twice.
fn dir_key(dir: &Path) -> String {
    let s = dir.to_string_lossy();
    let s = s.trim_end_matches(['\\', '/']);
    if cfg!(windows) {
        s.to_ascii_lowercase()
    } else {
        s.to_string()
    }
}

/// Directories gathered in priority order with duplicates collapsed by
/// [`dir_key`]. Carrying the `seen` set across two `take`s is what lets
/// [`candidates`] build the fallback phase only if it needs it, while still
/// dropping fallback directories that the PATH phase already swept.
#[derive(Default)]
struct DirSet {
    seen: Vec<String>,
    out: Vec<PathBuf>,
}

impl DirSet {
    fn push(&mut self, dir: PathBuf) {
        if dir.as_os_str().is_empty() {
            return;
        }
        let key = dir_key(&dir);
        if self.seen.iter().any(|k| k == &key) {
            return;
        }
        self.seen.push(key);
        self.out.push(dir);
    }

    fn extend(&mut self, dirs: impl IntoIterator<Item = PathBuf>) {
        for dir in dirs {
            self.push(dir);
        }
    }

    /// The directories accumulated since the last take; the `seen` set survives.
    fn take(&mut self) -> Vec<PathBuf> {
        std::mem::take(&mut self.out)
    }
}

/// The PATH a lookup sweeps FIRST: this process's `PATH`, then the live registry
/// one, de-duplicated. What the user's own shell would search — so a hit here
/// ends the search (see [`candidates`]).
pub fn path_dirs() -> Vec<PathBuf> {
    let mut set = DirSet::default();
    set.extend(std::env::var_os("PATH").iter().flat_map(std::env::split_paths));
    set.extend(live_path_dirs());
    set.take()
}

/// The RESCUE directories, swept only when [`path_dirs`] knew nothing about the
/// command: the adapter's own `extra` hints first — a tool's own installer is
/// better evidence than a shared guess — then [`well_known_dirs`].
///
/// Built lazily by [`candidates`], and only there, because assembling it costs a
/// second PATH sweep (see [`node_bin_dir`]) that the common case must not pay.
pub fn fallback_dirs(extra: &[PathBuf]) -> Vec<PathBuf> {
    let mut set = DirSet::default();
    set.extend(extra.iter().cloned());
    set.extend(well_known_dirs());
    set.take()
}

/// How many on-disk hits [`candidates`] reports. Detection probes these in order
/// until one runs, so the cap bounds the worst case: a machine littered with dead
/// shims costs at most this many spawns, while every real install answers on the
/// first or second.
pub const MAX_CANDIDATES: usize = 6;

/// Every on-disk hit for `cmd` in the phase that answered, ranked: directory
/// order outer, [`path_exts`] order inner (runnable before shim), capped at
/// [`MAX_CANDIDATES`]. [`path_dirs`] is swept first and, if it produced anything
/// at all, is the whole answer; only a PATH that knows nothing about `cmd` falls
/// through to `extra` + [`well_known_dirs`].
///
/// A ranked LIST rather than a single winner, because **a file existing is not
/// evidence that a tool is installed**. Uninstalls leave launchers behind: npm's
/// `.cmd` shim survives a `node_modules` deleted by hand, and each of these CLIs
/// keeps a config tree (`~/.claude`, `~/.codex`, `~/.pi`) that outlives the
/// binary by design — so a directory we sweep can easily hold a shim pointing at
/// nothing. Stopping at the first such file would report a machine that has a
/// leftover shim AND a working install elsewhere as broken, and one with only the
/// leftover as "needs path" when the honest answer is "not installed". Handing
/// every hit back lets the version probe decide, and running the binary is the
/// only thing that actually proves a tool is there.
///
/// Pure filesystem work; never spawns anything.
pub fn candidates(cmd: &str, extra: &[PathBuf]) -> Vec<PathBuf> {
    // An explicit path (absolute, or one carrying a separator) is not a PATH
    // lookup — it is the user's manual override, and the only candidate there is.
    if cmd.contains('/') || cmd.contains('\\') {
        let p = Path::new(cmd);
        return if p.is_file() { vec![p.to_path_buf()] } else { Vec::new() };
    }
    let exts = path_exts();
    let mut out: Vec<PathBuf> = Vec::new();
    let mut set = DirSet::default();
    set.extend(path_dirs());
    // PATH ANSWERED — stop. The fallback dirs exist to rescue a tool PATH does
    // not know about; sweeping them anyway to top the list up to MAX_CANDIDATES
    // would hand detection lower-ranked copies the user's own shell would never
    // run, and costs the ~28ms difference between an early exit (~48 stats) and
    // the full sweep (~500) on every pass, for every tool.
    if !sweep(&set.take(), cmd, &exts, &mut out) && out.is_empty() {
        // Pushed through the same `set`, so its `seen` keys still hold: a
        // fallback directory that is ALSO on PATH was swept a moment ago and
        // must not be stat-ed a second time.
        set.extend(fallback_dirs(extra));
        sweep(&set.take(), cmd, &exts, &mut out);
    }
    out
}

/// Append every `<dir>/<cmd><ext>` that exists, `dirs` outer and `exts` inner.
/// `true` when [`MAX_CANDIDATES`] was reached and the caller must stop.
fn sweep(dirs: &[PathBuf], cmd: &str, exts: &[String], out: &mut Vec<PathBuf>) -> bool {
    for dir in dirs {
        for ext in exts {
            let cand = dir.join(format!("{cmd}{ext}"));
            if cand.is_file() {
                out.push(cand);
                if out.len() >= MAX_CANDIDATES {
                    return true;
                }
            }
        }
    }
    false
}

/// The best-ranked on-disk hit for `cmd`, with no proof that it runs — see
/// [`candidates`] for why that distinction matters. Callers that can afford a
/// spawn should probe the whole candidate list instead.
pub fn lookup(cmd: &str, extra: &[PathBuf]) -> Option<PathBuf> {
    candidates(cmd, extra).into_iter().next()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn path_exts_ranks_runnable_before_the_bare_shim() {
        let exts = path_exts();
        assert_eq!(exts.last().map(String::as_str), Some(""), "shim is last");
        if cfg!(windows) {
            assert_eq!(exts[0], ".exe", "a real binary outranks every script");
            let cmd = exts.iter().position(|e| e == ".cmd").unwrap();
            let ps1 = exts.iter().position(|e| e == ".ps1").unwrap();
            let bare = exts.iter().position(|e| e.is_empty()).unwrap();
            assert!(cmd < ps1, "a cmd wrapper is cheaper than a PowerShell one");
            assert!(ps1 < bare, "even a ps1 beats the unspawnable POSIX shim");
            // no duplicates, however PATHEXT is spelled on this machine
            let mut sorted = exts.clone();
            sorted.sort();
            sorted.dedup();
            assert_eq!(sorted.len(), exts.len());
        } else {
            assert_eq!(exts, vec![String::new()]);
        }
    }

    #[test]
    fn path_dirs_starts_with_the_process_path_and_has_no_duplicates() {
        let dirs = path_dirs();
        let mut keys: Vec<String> = dirs.iter().map(|d| dir_key(d)).collect();
        let before = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(before, keys.len(), "every search directory appears once");

        if let Some(paths) = std::env::var_os("PATH") {
            if let Some(first) = std::env::split_paths(&paths).find(|p| !p.as_os_str().is_empty()) {
                assert_eq!(dirs.first(), Some(&first), "the process PATH leads");
            }
        }
    }

    /// Within the rescue phase an adapter's own hint outranks the shared bins:
    /// the tool's installer is direct evidence about THIS tool, while a
    /// package-manager prefix is a place any same-named binary could sit.
    #[test]
    fn an_adapter_hint_outranks_the_shared_well_known_bins() {
        let marker = PathBuf::from(if cfg!(windows) {
            r"C:\orrery-test-marker-dir"
        } else {
            "/orrery-test-marker-dir"
        });
        let dirs = fallback_dirs(std::slice::from_ref(&marker));
        assert_eq!(dirs.first(), Some(&marker), "the adapter's own hint leads: {dirs:?}");
        // …and the shared list follows it, de-duplicated: the same directory
        // swept twice is stats paid twice for an answer already known.
        let mut keys: Vec<String> = dirs.iter().map(|d| dir_key(d)).collect();
        let before = keys.len();
        keys.sort();
        keys.dedup();
        assert_eq!(before, keys.len(), "no repeats in {dirs:?}");
    }

    #[test]
    fn lookup_misses_a_bogus_command_and_accepts_an_explicit_path() {
        assert!(lookup("definitely-not-a-real-binary-xyzzy", &[]).is_none());

        let f = tempfile::NamedTempFile::new().unwrap();
        let p = f.path().to_string_lossy().into_owned();
        assert_eq!(lookup(&p, &[]).as_deref(), Some(f.path()), "explicit path wins");
        assert!(lookup(&format!("{p}-nope"), &[]).is_none());
    }

    #[test]
    fn lookup_prefers_the_runnable_extension_in_a_directory() {
        // npm's trio, side by side: the bare POSIX shim must NOT win.
        let dir = tempfile::tempdir().unwrap();
        let bare = dir.path().join("orrerytesttool");
        std::fs::write(&bare, "#!/bin/sh\n").unwrap();
        let extra = vec![dir.path().to_path_buf()];
        if cfg!(windows) {
            let cmd = dir.path().join("orrerytesttool.cmd");
            std::fs::write(&cmd, "@echo off\n").unwrap();
            assert_eq!(lookup("orrerytesttool", &extra).as_deref(), Some(cmd.as_path()));
        } else {
            assert_eq!(lookup("orrerytesttool", &extra).as_deref(), Some(bare.as_path()));
        }
    }

    #[test]
    fn candidates_lists_every_hit_in_rank_order_and_caps() {
        let dir = tempfile::tempdir().unwrap();
        let extra = vec![dir.path().to_path_buf()];
        for ext in ["", ".cmd", ".bat", ".exe", ".ps1", ".com"] {
            std::fs::write(dir.path().join(format!("orrerymulti{ext}")), "x").unwrap();
        }
        let got = candidates("orrerymulti", &extra);
        assert!(got.len() <= MAX_CANDIDATES, "capped at {MAX_CANDIDATES}");
        if cfg!(windows) {
            let names: Vec<String> = got
                .iter()
                .map(|p| p.file_name().unwrap().to_string_lossy().into_owned())
                .collect();
            assert_eq!(names.first().map(String::as_str), Some("orrerymulti.exe"));
            if let Some(bare) = names.iter().position(|n| n == "orrerymulti") {
                assert_eq!(bare, names.len() - 1, "the POSIX shim ranks last");
            }
        }
    }

    /// The module's contract: the fallback dirs are a RESCUE, not a top-up. A
    /// tool PATH already knows about must never be reported alongside copies from
    /// a package-manager bin the user's own shell would not search — and the
    /// sweep those dirs cost (~500 stats vs ~48) must not be paid either.
    #[test]
    fn a_path_hit_stops_the_sweep_before_the_fallback_dirs() {
        // A real interpreter, so the PATH phase genuinely answers without this
        // test having to mutate the process PATH (which every other test in this
        // binary reads, concurrently).
        let (name, ext) = if cfg!(windows) { ("cmd", ".exe") } else { ("sh", "") };
        let exts = path_exts();
        let Some(real) = path_dirs()
            .into_iter()
            .find_map(|d| exts.iter().map(|e| d.join(format!("{name}{e}"))).find(|p| p.is_file()))
        else {
            return; // no interpreter on PATH — nothing to assert
        };
        // A same-named copy in a fallback directory must be invisible: PATH
        // already answered, so the fallback phase never runs at all.
        let fallback = tempfile::tempdir().unwrap();
        std::fs::copy(&real, fallback.path().join(format!("{name}{ext}"))).unwrap();

        let got = candidates(name, &[fallback.path().to_path_buf()]);
        assert!(!got.is_empty(), "the PATH copy is found");
        assert!(
            !got.iter().any(|p| p.starts_with(fallback.path())),
            "a fallback dir must not top the list up once PATH answered: {got:?}",
        );
    }

    #[test]
    fn candidates_is_empty_for_a_directory_that_holds_no_such_file() {
        // The leftover-uninstall shape: the directory survives, the binary does
        // not. Nothing may be reported, or detection would call it installed.
        let dir = tempfile::tempdir().unwrap();
        assert!(candidates("orrerygone", &[dir.path().to_path_buf()]).is_empty());
    }

    /// The fallback list is derivation + verified constants. Two properties keep
    /// it from drifting back into convention: whatever is in it must be reachable
    /// from the machine's own state, and the entries that were only ever folklore
    /// must stay gone — each one costs 13 `is_file` calls per tool per pass and
    /// invites a same-named stranger's binary to be reported as the agent.
    #[test]
    fn well_known_dirs_are_derived_not_guessed() {
        let dirs = well_known_dirs();
        let keys: Vec<String> = dirs.iter().map(|d| dir_key(d)).collect();

        // Derived: node's own directory, which is npm's global prefix.
        if let Some(node) = node_bin_dir() {
            assert!(
                keys.contains(&dir_key(&node)),
                "node's directory is the derived npm prefix: {dirs:?}",
            );
        }
        // Derived: each package manager's own variable, when it set one.
        for (var, suffix) in [("PNPM_HOME", None), ("BUN_INSTALL", Some("bin"))] {
            if let Some(v) = std::env::var_os(var).map(PathBuf::from) {
                let want = suffix.map_or(v.clone(), |s| v.join(s));
                assert!(keys.contains(&dir_key(&want)), "{var} is honoured: {dirs:?}");
            }
        }
        // Guesses that were removed, and must not creep back.
        for gone in [".volta", "scoop", ".cargo", ".yarn", "winget", "npm-global"] {
            assert!(
                !keys.iter().any(|k| k.contains(gone)),
                "`{gone}` is convention, not evidence: {dirs:?}",
            );
        }
    }

    #[test]
    fn in_home_builds_under_the_home_directory() {
        if let Some(home) = home_dir() {
            assert_eq!(in_home(&["a", "b"]), Some(home.join("a").join("b")));
        }
    }

    #[cfg(windows)]
    #[test]
    fn live_path_dirs_reads_the_registry() {
        // Every Windows box has a machine Path; a user Path is optional. The read
        // must at minimum produce the system directories.
        let dirs = live_path_dirs();
        assert!(!dirs.is_empty(), "machine Path should always resolve");
        assert!(
            dirs.iter().any(|d| {
                let s = d.to_string_lossy().to_ascii_lowercase();
                s.contains("system32")
            }),
            "expected the system directory among {dirs:?}",
        );
        // Expansion happened: no literal %VAR% survives a successful read.
        assert!(
            !dirs.iter().any(|d| d.to_string_lossy().contains('%')),
            "REG_EXPAND_SZ values must come back expanded",
        );
    }
}
