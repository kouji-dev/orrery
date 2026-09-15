//! Language-server detection: where would we launch this server from? Pure
//! over a [`DetectEnv`] snapshot so tests can hand in a temp PATH. No process
//! is spawned here — prerequisite version probing (java 17+) is a later
//! milestone; today the `requires` list only feeds the `hint`.
//!
//! Precedence ([`resolve_server`]): the installed self-contained pack
//! (`bundled`) → the user's manual path (`configured`) → a system server on
//! PATH / known dirs, ONLY when `lsp_use_system_servers` is on (`found`) →
//! `missing` with a hint.

use std::collections::HashMap;
use std::ffi::OsString;
use std::path::{Path, PathBuf};

use serde::Serialize;

use super::manifest::{Launch, ServerManifest};

#[derive(Debug, Clone, PartialEq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Detection {
    /// `"bundled" | "configured" | "found" | "missing"`.
    pub status: String,
    pub path: Option<String>,
    pub hint: Option<String>,
}

impl Detection {
    fn found(path: PathBuf) -> Self {
        Self {
            status: "found".into(),
            path: Some(path.to_string_lossy().into_owned()),
            hint: None,
        }
    }
}

/// A self-contained pack resolved on disk: what to spawn and the entry it
/// runs. `program` = the runtime binary (node / java) or `entry` itself.
#[derive(Debug, Clone, PartialEq)]
pub struct Bundled {
    pub program: PathBuf,
    pub entry: PathBuf,
    pub launch: Launch,
}

/// Where a server launches from, in precedence order.
#[derive(Debug, Clone, PartialEq)]
pub enum Resolved {
    Bundled(Bundled),
    /// `settings.lspPaths[id]` — the user's explicit choice.
    Configured(PathBuf),
    /// A system server (opt-in fallback).
    System(PathBuf),
    Missing { hint: Option<String> },
}

impl Resolved {
    pub fn detection(&self) -> Detection {
        let s = |p: &Path| Some(p.to_string_lossy().into_owned());
        match self {
            Resolved::Bundled(b) => Detection { status: "bundled".into(), path: s(&b.program), hint: None },
            Resolved::Configured(p) => Detection { status: "configured".into(), path: s(p), hint: None },
            Resolved::System(p) => Detection { status: "found".into(), path: s(p), hint: None },
            Resolved::Missing { hint } => Detection { status: "missing".into(), path: None, hint: hint.clone() },
        }
    }
}

/// Inputs of [`resolve_server`] beyond the manifest.
pub struct ResolveOpts<'a> {
    /// The installed pack dir (`None` = not installed).
    pub pack_dir: Option<&'a Path>,
    /// Binary of the installed runtime pack `launch.runtime` names.
    pub runtime_bin: Option<&'a Path>,
    /// `settings.lspPaths[id]`.
    pub configured: Option<&'a str>,
    /// `settings.lspUseSystemServers`.
    pub use_system: bool,
    pub target: &'a str,
}

/// `*` wildcard match on one path segment (`org.eclipse.equinox.launcher_*.jar`).
fn segment_matches(pattern: &str, name: &str) -> bool {
    let parts: Vec<&str> = pattern.split('*').collect();
    if parts.len() == 1 {
        return pattern == name;
    }
    let mut rest = name;
    for (i, part) in parts.iter().enumerate() {
        if i == 0 {
            let Some(r) = rest.strip_prefix(part) else { return false };
            rest = r;
        } else if i == parts.len() - 1 {
            return rest.ends_with(part);
        } else if let Some(at) = rest.find(part) {
            rest = &rest[at + part.len()..];
        } else {
            return false;
        }
    }
    true
}

/// `entry` under `pack_dir`; a `*` segment must match exactly one child (0 →
/// not installed correctly, 2+ → ambiguous — both are errors, never a guess).
pub fn resolve_entry(pack_dir: &Path, entry: &str) -> Result<PathBuf, String> {
    let mut cur = pack_dir.to_path_buf();
    for seg in entry.split('/').filter(|s| !s.is_empty()) {
        if !seg.contains('*') {
            cur = cur.join(seg);
            continue;
        }
        let mut hits: Vec<PathBuf> = std::fs::read_dir(&cur)
            .map_err(|e| format!("{}: {e}", cur.display()))?
            .filter_map(Result::ok)
            .filter(|e| e.file_name().to_str().is_some_and(|n| segment_matches(seg, n)))
            .map(|e| e.path())
            .collect();
        match hits.len() {
            1 => cur = hits.remove(0),
            0 => return Err(format!("{}: nothing matches {seg:?}", cur.display())),
            n => {
                hits.sort();
                return Err(format!("{}: {n} entries match {seg:?} (ambiguous)", cur.display()));
            }
        }
    }
    if !cur.is_file() {
        return Err(format!("{}: launch entry not found", cur.display()));
    }
    Ok(cur)
}

/// The installed pack's launch on disk. `Err` = installed but unusable (the
/// entry vanished, the runtime pack is missing) — surfaced as the hint.
pub fn resolve_bundled(
    m: &ServerManifest,
    pack_dir: &Path,
    runtime_bin: Option<&Path>,
) -> Result<Bundled, String> {
    let launch = m
        .launch
        .clone()
        .ok_or_else(|| "pack has no launch block (legacy pack)".to_string())?;
    let entry = resolve_entry(pack_dir, &launch.entry)?;
    let program = match launch.runtime.as_deref() {
        None => entry.clone(),
        Some(rt) => match runtime_bin {
            Some(p) if p.is_file() => p.to_path_buf(),
            Some(p) => return Err(format!("runtime.{rt}: {} is missing", p.display())),
            None => {
                return Err(format!(
                    "runtime.{rt} is not installed — reinstall {} from Extensions",
                    m.id
                ))
            }
        },
    };
    Ok(Bundled { program, entry, launch })
}

/// The launch source for a server pack. See the module doc for the order.
pub fn resolve_server(m: &ServerManifest, o: &ResolveOpts<'_>, env: &DetectEnv) -> Resolved {
    let mut bundled_err = None;
    if let (Some(dir), Some(_)) = (o.pack_dir, &m.launch) {
        match resolve_bundled(m, dir, o.runtime_bin) {
            Ok(b) => return Resolved::Bundled(b),
            Err(e) => bundled_err = Some(e),
        }
    }
    if let Some(p) = o.configured.map(str::trim).filter(|p| !p.is_empty()) {
        return Resolved::Configured(PathBuf::from(p));
    }
    if o.use_system {
        let d = detect_server(m, None, o.pack_dir, o.target, env);
        if let Some(p) = d.path.filter(|_| d.status != "missing") {
            return Resolved::System(PathBuf::from(p));
        }
    }
    let hint = if let Some(e) = bundled_err {
        Some(e)
    } else if o.pack_dir.is_none() {
        Some(format!("install the {} server pack from Extensions", m.label_or_id()))
    } else if o.use_system {
        Some(hint_for(m, o.target)).filter(|h| !h.is_empty())
    } else {
        Some(format!(
            "this {} pack is not self-contained — enable system servers or set a path",
            m.label_or_id()
        ))
    };
    Resolved::Missing { hint }
}

impl ServerManifest {
    fn label_or_id(&self) -> &str {
        if self.label.trim().is_empty() {
            &self.id
        } else {
            &self.label
        }
    }
}

/// The bits of the environment detection reads, captured once per call.
pub struct DetectEnv {
    pub path_env: Option<OsString>,
    pub home: Option<PathBuf>,
    pub vars: HashMap<String, String>,
    pub windows: bool,
}

impl DetectEnv {
    pub fn current() -> Self {
        Self {
            path_env: std::env::var_os("PATH"),
            home: std::env::var_os("HOME")
                .or_else(|| std::env::var_os("USERPROFILE"))
                .map(PathBuf::from),
            vars: std::env::vars().collect(),
            windows: cfg!(windows),
        }
    }
}

/// `name` inside `dir`, honoring Windows launcher extensions when `name` has
/// none. On Windows the launcher wins over a bare extensionless file: npm
/// writes a POSIX `sh` shim (`typescript-language-server`) NEXT TO the
/// `.cmd`, and CreateProcess cannot run the shim (error 193). Elsewhere the
/// bare file is the launcher (a shell script).
fn find_in_dir(dir: &Path, name: &str, windows: bool) -> Option<PathBuf> {
    if windows && Path::new(name).extension().is_none() {
        for ext in ["exe", "cmd", "bat"] {
            let p = dir.join(format!("{name}.{ext}"));
            if p.is_file() {
                return Some(p);
            }
        }
    }
    let direct = dir.join(name);
    if direct.is_file() {
        return Some(direct);
    }
    None
}

fn find_on_path(names: &[String], env: &DetectEnv) -> Option<PathBuf> {
    let dirs: Vec<PathBuf> = env
        .path_env
        .as_ref()
        .map(|p| std::env::split_paths(p).collect())
        .unwrap_or_default();
    for name in names {
        for dir in &dirs {
            if let Some(p) = find_in_dir(dir, name, env.windows) {
                return Some(p);
            }
        }
    }
    None
}

fn hint_for(m: &ServerManifest, target: &str) -> String {
    let mut parts: Vec<String> = m
        .detect
        .requires
        .iter()
        .map(|r| match &r.min_version {
            Some(v) => format!("needs {} {v}+", r.program),
            None => format!("needs {}", r.program),
        })
        .collect();
    if m.download.contains_key(target) || m.download.contains_key("any") {
        parts.push("download available".into());
    } else if !m.program.is_empty() {
        parts.push(format!("install {} and add it to PATH", m.program));
    }
    parts.join("; ")
}

/// The JVM a Java-based server (jdtls) runs on: `$JAVA_HOME/bin/java`, else
/// `java` on PATH.
pub fn find_java(env: &DetectEnv) -> Option<PathBuf> {
    if let Some(home) = env.vars.get("JAVA_HOME").filter(|v| !v.is_empty()) {
        if let Some(p) = find_in_dir(&Path::new(home).join("bin"), "java", env.windows) {
            return Some(p);
        }
    }
    find_on_path(&["java".to_string()], env)
}

/// Resolve a server's launch path. Order: the user's manual path → the
/// manifest's `detect.env` dirs → `detect.path` names on PATH → `~/.cargo/bin`
/// and `~/go/bin` → the downloaded `entry` under the pack dir → `missing`.
pub fn detect_server(
    m: &ServerManifest,
    configured: Option<&str>,
    pack_dir: Option<&Path>,
    target: &str,
    env: &DetectEnv,
) -> Detection {
    if let Some(p) = configured.map(str::trim).filter(|p| !p.is_empty()) {
        return Detection {
            status: "configured".into(),
            path: Some(p.to_string()),
            hint: None,
        };
    }
    let names: Vec<String> = if m.detect.path.is_empty() {
        vec![m.program.clone()]
    } else {
        m.detect.path.clone()
    };
    let names: Vec<String> = names.into_iter().filter(|n| !n.is_empty()).collect();
    for var in &m.detect.env {
        if let Some(dir) = env.vars.get(var).filter(|v| !v.is_empty()) {
            let dir = Path::new(dir);
            for base in [dir.to_path_buf(), dir.join("bin")] {
                for name in &names {
                    if let Some(p) = find_in_dir(&base, name, env.windows) {
                        return Detection::found(p);
                    }
                }
            }
        }
    }
    if let Some(p) = find_on_path(&names, env) {
        return Detection::found(p);
    }
    if let Some(home) = &env.home {
        for base in [home.join(".cargo").join("bin"), home.join("go").join("bin")] {
            for name in &names {
                if let Some(p) = find_in_dir(&base, name, env.windows) {
                    return Detection::found(p);
                }
            }
        }
    }
    if let Some(dir) = pack_dir {
        let entry = m
            .download
            .get(target)
            .or_else(|| m.download.get("any"))
            .and_then(|d| d.entry.as_deref());
        if let Some(entry) = entry {
            let p = dir.join(entry);
            if p.is_file() {
                return Detection::found(p);
            }
        }
    }
    Detection {
        status: "missing".into(),
        path: None,
        hint: Some(hint_for(m, target)).filter(|h| !h.is_empty()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::manifest::{fixtures, Manifest};

    fn server() -> ServerManifest {
        let Manifest::Server(s) = Manifest::parse(fixtures::SERVER).unwrap() else {
            unreachable!()
        };
        s
    }

    fn env(path_dirs: &[&Path], windows: bool) -> DetectEnv {
        DetectEnv {
            path_env: Some(std::env::join_paths(path_dirs).unwrap()),
            home: None,
            vars: HashMap::new(),
            windows,
        }
    }

    #[test]
    fn configured_path_wins_over_everything() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("jdtls"), b"").unwrap();
        let d = detect_server(
            &server(),
            Some("C:/manual/jdtls.bat"),
            None,
            "windows-x86_64",
            &env(&[tmp.path()], false),
        );
        assert_eq!(d.status, "configured");
        assert_eq!(d.path.as_deref(), Some("C:/manual/jdtls.bat"));
        // whitespace-only is "not configured"
        let d = detect_server(&server(), Some("  "), None, "windows-x86_64", &env(&[], false));
        assert_eq!(d.status, "missing");
    }

    #[test]
    fn finds_on_temp_path_with_windows_launcher_extensions() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("jdtls.cmd"), b"").unwrap();
        let d = detect_server(&server(), None, None, "windows-x86_64", &env(&[tmp.path()], true));
        assert_eq!(d.status, "found");
        assert!(d.path.unwrap().ends_with("jdtls.cmd"));
        // non-windows: `.cmd` is not a launcher, so nothing is found
        let d = detect_server(&server(), None, None, "linux-x86_64", &env(&[tmp.path()], false));
        assert_eq!(d.status, "missing");
        // second name in detect.path (`jdtls.bat`) is tried verbatim on any OS
        std::fs::write(tmp.path().join("jdtls.bat"), b"").unwrap();
        let d = detect_server(&server(), None, None, "linux-x86_64", &env(&[tmp.path()], false));
        assert_eq!(d.status, "found");
    }

    #[test]
    fn env_dir_home_bins_and_pack_entry_are_fallbacks() {
        let tmp = tempfile::tempdir().unwrap();
        let m = server();
        // detect.env → JDTLS_HOME/bin/jdtls
        let home_dir = tmp.path().join("jdtls-home");
        std::fs::create_dir_all(home_dir.join("bin")).unwrap();
        std::fs::write(home_dir.join("bin").join("jdtls"), b"").unwrap();
        let mut e = env(&[], false);
        e.vars.insert("JDTLS_HOME".into(), home_dir.to_string_lossy().into_owned());
        let d = detect_server(&m, None, None, "windows-x86_64", &e);
        assert_eq!(d.status, "found");
        assert!(d.path.unwrap().contains("jdtls-home"));
        // ~/.cargo/bin
        let home = tmp.path().join("home");
        std::fs::create_dir_all(home.join(".cargo").join("bin")).unwrap();
        std::fs::write(home.join(".cargo").join("bin").join("jdtls"), b"").unwrap();
        let mut e = env(&[], false);
        e.home = Some(home.clone());
        let d = detect_server(&m, None, None, "windows-x86_64", &e);
        assert_eq!(d.status, "found");
        assert!(d.path.unwrap().contains(".cargo"));
        // downloaded entry under the pack dir (target-specific)
        let pack = tmp.path().join("pack");
        std::fs::create_dir_all(pack.join("bin")).unwrap();
        std::fs::write(pack.join("bin").join("jdtls.bat"), b"").unwrap();
        let d = detect_server(&m, None, Some(&pack), "windows-x86_64", &env(&[], false));
        assert_eq!(d.status, "found");
        let d = detect_server(&m, None, Some(&pack), "macos-aarch64", &env(&[], false));
        assert_eq!(d.status, "missing", "no download entry for this target");
    }

    #[test]
    fn windows_prefers_the_cmd_launcher_over_npm_s_sh_shim() {
        let tmp = tempfile::tempdir().unwrap();
        std::fs::write(tmp.path().join("jdtls"), b"#!/bin/sh
").unwrap();
        std::fs::write(tmp.path().join("jdtls.cmd"), b"").unwrap();
        let d = detect_server(&server(), None, None, "windows-x86_64", &env(&[tmp.path()], true));
        assert!(d.path.unwrap().ends_with("jdtls.cmd"), "the sh shim is not spawnable on Windows");
        let d = detect_server(&server(), None, None, "linux-x86_64", &env(&[tmp.path()], false));
        assert!(d.path.unwrap().ends_with("jdtls"), "posix: the script is the launcher");
        // `.exe` outranks `.cmd`; an explicit extension in the name is taken verbatim
        std::fs::write(tmp.path().join("jdtls.exe"), b"").unwrap();
        let d = detect_server(&server(), None, None, "windows-x86_64", &env(&[tmp.path()], true));
        assert!(d.path.unwrap().ends_with("jdtls.exe"));
        let mut m = server();
        m.detect.path = vec!["jdtls.bat".into()];
        std::fs::write(tmp.path().join("jdtls.bat"), b"").unwrap();
        let d = detect_server(&m, None, None, "windows-x86_64", &env(&[tmp.path()], true));
        assert!(d.path.unwrap().ends_with("jdtls.bat"));
    }

    #[test]
    fn java_home_wins_over_path() {
        let tmp = tempfile::tempdir().unwrap();
        let home = tmp.path().join("jdk");
        std::fs::create_dir_all(home.join("bin")).unwrap();
        std::fs::write(home.join("bin").join("java.exe"), b"").unwrap();
        let on_path = tmp.path().join("bin");
        std::fs::create_dir_all(&on_path).unwrap();
        std::fs::write(on_path.join("java.exe"), b"").unwrap();
        let mut e = env(&[&on_path], true);
        assert!(find_java(&e).unwrap().starts_with(&on_path));
        e.vars.insert("JAVA_HOME".into(), home.to_string_lossy().into_owned());
        assert!(find_java(&e).unwrap().starts_with(&home));
        assert_eq!(find_java(&env(&[], true)), None);
    }

    fn bundled() -> ServerManifest {
        let Manifest::Server(s) = Manifest::parse(fixtures::SERVER_BUNDLED).unwrap() else {
            unreachable!()
        };
        s
    }

    fn jdtls_bundled() -> ServerManifest {
        let Manifest::Server(s) = Manifest::parse(fixtures::SERVER_JDTLS_BUNDLED).unwrap() else {
            unreachable!()
        };
        s
    }

    fn touch(p: &Path) {
        std::fs::create_dir_all(p.parent().unwrap()).unwrap();
        std::fs::write(p, b"").unwrap();
    }

    #[test]
    fn entry_glob_resolves_exactly_one_match() {
        let tmp = tempfile::tempdir().unwrap();
        let pack = tmp.path();
        touch(&pack.join("plugins").join("org.eclipse.equinox.launcher_1.7.0.jar"));
        touch(&pack.join("plugins").join("org.eclipse.equinox.launcher.win32_1.2.jar"));
        let p = resolve_entry(pack, "plugins/org.eclipse.equinox.launcher_*.jar").unwrap();
        assert!(p.ends_with("org.eclipse.equinox.launcher_1.7.0.jar"));
        // plain path
        touch(&pack.join("server").join("cli.mjs"));
        assert_eq!(resolve_entry(pack, "server/cli.mjs").unwrap(), pack.join("server").join("cli.mjs"));
        // 0 matches → error; 2 matches → error
        let e = resolve_entry(pack, "plugins/nothing_*.jar").unwrap_err();
        assert!(e.contains("nothing matches"), "{e}");
        touch(&pack.join("plugins").join("org.eclipse.equinox.launcher_1.8.0.jar"));
        let e = resolve_entry(pack, "plugins/org.eclipse.equinox.launcher_*.jar").unwrap_err();
        assert!(e.contains("ambiguous"), "{e}");
        assert!(resolve_entry(pack, "server/missing.mjs").is_err());
        assert!(segment_matches("a*b*c", "aXXbYYc"));
        assert!(!segment_matches("a*b*c", "aXXcYYb"));
        assert!(segment_matches("*.jar", "x.jar"));
        assert!(!segment_matches("x.jar", "y.jar"));
    }

    #[test]
    fn bundled_resolution_per_runtime_kind() {
        let tmp = tempfile::tempdir().unwrap();
        let pack = tmp.path().join("server.typescript-language-server").join("6.0.0-1");
        let entry = pack.join("server/node_modules/typescript-language-server/lib/cli.mjs");
        touch(&entry);
        let node = tmp.path().join("runtime.node").join("22").join("node.exe");
        touch(&node);
        let b = resolve_bundled(&bundled(), &pack, Some(&node)).unwrap();
        assert_eq!(b.program, node);
        assert_eq!(b.entry, entry);
        assert_eq!(b.launch.runtime.as_deref(), Some("node"));
        // runtime pack missing → error naming it
        let e = resolve_bundled(&bundled(), &pack, None).unwrap_err();
        assert!(e.contains("runtime.node is not installed"), "{e}");
        let e = resolve_bundled(&bundled(), &pack, Some(&tmp.path().join("gone.exe"))).unwrap_err();
        assert!(e.contains("is missing"), "{e}");
        // runtime: null → the entry is the program
        let mut m = bundled();
        m.launch.as_mut().unwrap().runtime = None;
        m.requires.clear();
        let b = resolve_bundled(&m, &pack, None).unwrap();
        assert_eq!(b.program, entry);
        // legacy manifest → error
        assert!(resolve_bundled(&server(), &pack, None).is_err());
    }

    #[test]
    fn precedence_bundled_over_configured_over_system_over_missing() {
        let tmp = tempfile::tempdir().unwrap();
        let pack = tmp.path().join("pack");
        touch(&pack.join("server/node_modules/typescript-language-server/lib/cli.mjs"));
        let node = tmp.path().join("node.exe");
        touch(&node);
        let on_path = tmp.path().join("bin");
        touch(&on_path.join("typescript-language-server.cmd"));
        let e = env(&[&on_path], true);
        fn opts<'a>(
            pack_dir: Option<&'a Path>,
            runtime: Option<&'a Path>,
            configured: Option<&'a str>,
            use_system: bool,
        ) -> ResolveOpts<'a> {
            ResolveOpts { pack_dir, runtime_bin: runtime, configured, use_system, target: "windows-x86_64" }
        }
        // 1. bundled beats everything
        let r = resolve_server(&bundled(), &opts(Some(&pack), Some(&node), Some("C:/manual/tls.cmd"), true), &e);
        assert!(matches!(&r, Resolved::Bundled(b) if b.program == node), "{r:?}");
        let d = r.detection();
        assert_eq!(d.status, "bundled");
        assert_eq!(d.path.as_deref(), Some(node.to_string_lossy().as_ref()));
        // 2. no pack → configured
        let r = resolve_server(&bundled(), &opts(None, None, Some("C:/manual/tls.cmd"), true), &e);
        assert_eq!(r, Resolved::Configured(PathBuf::from("C:/manual/tls.cmd")));
        assert_eq!(r.detection().status, "configured");
        // 3. no pack, no manual path, system enabled → PATH hit
        let r = resolve_server(&bundled(), &opts(None, None, None, true), &e);
        assert!(matches!(&r, Resolved::System(p) if p.ends_with("typescript-language-server.cmd")), "{r:?}");
        assert_eq!(r.detection().status, "found");
        // 4. system disabled (the default) → missing, even though PATH has it
        let r = resolve_server(&bundled(), &opts(None, None, None, false), &e);
        let d = r.detection();
        assert_eq!(d.status, "missing");
        assert!(d.hint.unwrap().contains("install the TypeScript Language Server server pack"));
        // installed pack whose runtime is gone → missing with the runtime hint, not the PATH server
        let r = resolve_server(&bundled(), &opts(Some(&pack), None, None, false), &e);
        assert!(r.detection().hint.unwrap().contains("runtime.node"));
        // …but a manual path still rescues it (explicit user choice)
        let r = resolve_server(&bundled(), &opts(Some(&pack), None, Some("C:/m.cmd"), false), &e);
        assert_eq!(r, Resolved::Configured(PathBuf::from("C:/m.cmd")));
        // legacy pack (no launch) installed + system off → missing with the not-self-contained hint
        let r = resolve_server(&server(), &opts(Some(&pack), None, None, false), &e);
        assert!(r.detection().hint.unwrap().contains("not self-contained"));
        // legacy pack + system on → the requires-derived hint
        let r = resolve_server(&server(), &opts(Some(&pack), None, None, true), &env(&[], true));
        assert!(r.detection().hint.unwrap().contains("needs java 17+"));
        // jdtls glob entry + java runtime
        let jp = tmp.path().join("jdtls");
        touch(&jp.join("plugins").join("org.eclipse.equinox.launcher_1.7.0.v2025.jar"));
        let java = tmp.path().join("jre").join("bin").join("java.exe");
        touch(&java);
        let r = resolve_server(&jdtls_bundled(), &opts(Some(&jp), Some(&java), None, false), &e);
        let Resolved::Bundled(b) = r else { panic!("{r:?}") };
        assert_eq!(b.program, java);
        assert!(b.entry.ends_with("org.eclipse.equinox.launcher_1.7.0.v2025.jar"));
    }

    #[test]
    fn missing_carries_a_hint_from_requires() {
        let d = detect_server(&server(), None, None, "windows-x86_64", &env(&[], true));
        assert_eq!(d.status, "missing");
        assert_eq!(d.path, None);
        let hint = d.hint.unwrap();
        assert!(hint.contains("needs java 17+"), "{hint}");
        assert!(hint.contains("download available"), "{hint}");
        let mut m = server();
        m.detect.requires.clear();
        m.download.clear();
        let d = detect_server(&m, None, None, "windows-x86_64", &env(&[], true));
        assert_eq!(d.hint.as_deref(), Some("install jdtls and add it to PATH"));
    }
}
