//! Where library sources live on this machine (M4). Pure functions over an
//! injected [`DiscoverEnv`] so every rule is testable with temp dirs; the
//! only process spawn (`java -XshowSettings`) is isolated in
//! [`java_home_from_cmd`] and goes through `core::proc::cmd`.
//!
//! - JDK (`jdk:<hash>`, one per installation): `JAVA_HOME`, the running
//!   `java`'s `java.home`, vendor dirs under `%ProgramFiles%`, sdkman, scoop,
//!   macOS JVMs → `lib/src.zip`.
//! - Cargo (`cargo:<projectId>`, one per project): the `[[package]]` entries
//!   with a `registry+…` source of the project's `Cargo.lock` (at the root,
//!   in a workspace member named by the root `Cargo.toml`, or one level
//!   down — `src-tauri/Cargo.lock`), each at its PINNED version under
//!   `~/.cargo/registry/src/<index>/<name>-<version>/`. Never "newest wins".
//! - Maven (`maven:<projectId>`, one per project): the `<dependency>` rows of
//!   the project's `pom.xml` (+ `<modules>`), `${property}` resolved from
//!   `<properties>`, `<parent>` (a `relativePath` pom on disk, else the
//!   parent's `.pom` in `~/.m2`) and `project.*`; a version left to a BOM /
//!   dependencyManagement falls back to the newest `*-sources.jar` of that
//!   artifact in `~/.m2`. Transitive dependencies are not resolved (v1);
//!   Gradle projects are not read (v1).
//!
//! An artifact named by a lockfile but absent on disk is reported as
//! `missing` (never an error).

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Serialize;

/// One project the app knows (id + display name + root on disk).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct LibProject {
    pub id: String,
    pub name: String,
    pub root: PathBuf,
}

/// One crate dir / sources jar / src.zip under a source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Artifact {
    /// `serde-1.0.219` / `gson-2.11.0-sources.jar` / `src.zip`.
    pub name: String,
    /// `crate | jar | zip`.
    pub kind: String,
    /// `None` = named by the lockfile but not on disk.
    pub path: Option<PathBuf>,
    /// Change ⇒ re-index this artifact only (a registry checkout is
    /// immutable at its pinned version, so its name is enough; a jar is
    /// `path|size|mtime`).
    pub fingerprint: String,
}

/// One discovered source.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct LibSource {
    /// `jdk:<blake3(path|size|mtime)[..16]>` / `cargo:<projectId>` / `maven:<projectId>`.
    pub id: String,
    /// `jdk | maven | cargo`.
    pub kind: String,
    /// The src.zip, or the (first) lockfile the source follows.
    pub path: PathBuf,
    /// `JDK 21 (openjdk21)` / `Cargo · orrery` / `Maven · shop`.
    pub label: String,
    /// JDK: `path|size|mtime`; cargo/maven: blake3 of the lockfile bytes.
    pub fingerprint: String,
    /// Zip size; lockfile sources report 0 here (summed while indexing).
    pub size_bytes: u64,
    pub project_id: Option<String>,
    pub project_name: Option<String>,
    pub artifacts: Vec<Artifact>,
}

impl LibSource {
    pub fn missing(&self) -> usize {
        self.artifacts.iter().filter(|a| a.path.is_none()).count()
    }
}

/// Everything discovery reads from the environment.
#[derive(Debug, Clone, Default)]
pub struct DiscoverEnv {
    pub java_home: Option<PathBuf>,
    /// `java.home` reported by the `java` on PATH (see [`java_home_from_cmd`]).
    pub java_home_from_cmd: Option<PathBuf>,
    pub program_files: Vec<PathBuf>,
    pub home: Option<PathBuf>,
    /// `~/.m2/repository` override (`M2_REPO`-like); defaults under `home`.
    pub m2_repository: Option<PathBuf>,
    /// `CARGO_HOME` override; defaults to `home/.cargo`.
    pub cargo_home: Option<PathBuf>,
}

impl DiscoverEnv {
    /// The real environment of this process (no spawn: `java_home_from_cmd`
    /// is filled by the caller when it wants the extra probe).
    pub fn from_process() -> Self {
        let home = std::env::var_os("USERPROFILE")
            .or_else(|| std::env::var_os("HOME"))
            .map(PathBuf::from);
        let mut program_files = Vec::new();
        for k in ["ProgramFiles", "ProgramW6432", "ProgramFiles(x86)"] {
            if let Some(p) = std::env::var_os(k) {
                let p = PathBuf::from(p);
                if !program_files.contains(&p) {
                    program_files.push(p);
                }
            }
        }
        Self {
            java_home: std::env::var_os("JAVA_HOME").map(PathBuf::from).filter(|p| !p.as_os_str().is_empty()),
            java_home_from_cmd: None,
            program_files,
            home,
            m2_repository: None,
            cargo_home: std::env::var_os("CARGO_HOME").map(PathBuf::from),
        }
    }
}

/// `java.home` of the `java` on PATH: `java -XshowSettings:properties -version`
/// prints the property table on stderr. `None` when there is no java or the
/// output has no such line.
pub fn java_home_from_cmd() -> Option<PathBuf> {
    let out = crate::core::proc::cmd("java")
        .args(["-XshowSettings:properties", "-version"])
        .output()
        .ok()?;
    let text = String::from_utf8_lossy(&out.stderr);
    parse_java_home(&text)
}

/// The `java.home = <path>` line of a `-XshowSettings:properties` dump.
pub fn parse_java_home(stderr: &str) -> Option<PathBuf> {
    stderr
        .lines()
        .map(str::trim)
        .find_map(|l| l.strip_prefix("java.home"))
        .map(|rest| rest.trim_start().trim_start_matches('=').trim())
        .filter(|p| !p.is_empty())
        .map(PathBuf::from)
}

/// `(path|size|mtime, size)` of a file.
pub fn stat_fingerprint(path: &Path) -> (String, u64) {
    let (size, mtime) = std::fs::metadata(path)
        .map(|m| {
            let mtime = m
                .modified()
                .ok()
                .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
                .map(|d| d.as_secs())
                .unwrap_or(0);
            (m.len(), mtime)
        })
        .unwrap_or((0, 0));
    let norm = path.to_string_lossy().replace('\\', "/");
    (format!("{norm}|{size}|{mtime}"), size)
}

/// `blake3(text)` hex, first 16 chars.
pub fn id_of(fingerprint: &str) -> String {
    blake3::hash(fingerprint.as_bytes()).to_hex()[..16].to_string()
}

fn version_key(v: &str) -> Vec<u64> {
    v.split(['.', '-', '+'])
        .map(|p| p.parse::<u64>().unwrap_or(0))
        .collect()
}

fn subdirs(dir: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = std::fs::read_dir(dir)
        .map(|rd| rd.flatten().map(|e| e.path()).filter(|p| p.is_dir()).collect())
        .unwrap_or_default();
    out.sort();
    out
}

// ---- JDK ------------------------------------------------------------------------

/// `JAVA_VERSION="21.0.2"` → `21` from the JDK's `release` file; falls back
/// to a digit run in the home dir name (`openjdk21`, `jdk-17.0.1`).
pub fn jdk_major(home: &Path) -> Option<String> {
    if let Ok(rel) = std::fs::read_to_string(home.join("release")) {
        for line in rel.lines() {
            if let Some(v) = line.trim().strip_prefix("JAVA_VERSION=") {
                let v = v.trim().trim_matches('"');
                let major = match v.strip_prefix("1.") {
                    Some(rest) => rest.split(['.', '_']).next().unwrap_or(rest),
                    None => v.split(['.', '-', '+']).next().unwrap_or(v),
                };
                if !major.is_empty() && major.chars().all(|c| c.is_ascii_digit()) {
                    return Some(major.to_string());
                }
            }
        }
    }
    let name = jdk_dir_name(home);
    let digits: String = name.chars().skip_while(|c| !c.is_ascii_digit()).take_while(|c| c.is_ascii_digit()).collect();
    if digits.is_empty() {
        None
    } else {
        Some(digits)
    }
}

/// The meaningful directory name of a JDK home: skips scoop's `current` and
/// macOS' `Contents/Home`.
pub fn jdk_dir_name(home: &Path) -> String {
    let mut p = Some(home);
    while let Some(cur) = p {
        let name = cur.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if !name.is_empty() && !matches!(name.as_str(), "current" | "Home" | "Contents") {
            return name;
        }
        p = cur.parent();
    }
    home.to_string_lossy().to_string()
}

pub fn jdk_label(home: &Path) -> String {
    let name = jdk_dir_name(home);
    match jdk_major(home) {
        Some(major) => format!("JDK {major} ({name})"),
        None => format!("JDK ({name})"),
    }
}

/// `home/lib/src.zip` when it exists (also accepts `home/src.zip`, the JDK 8
/// layout).
fn src_zip_of(home: &Path) -> Option<PathBuf> {
    let a = home.join("lib").join("src.zip");
    if a.is_file() {
        return Some(a);
    }
    let b = home.join("src.zip");
    if b.is_file() {
        return Some(b);
    }
    None
}

/// Candidate JDK homes in precedence order (duplicates removed).
pub fn jdk_homes(env: &DiscoverEnv) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut push = |p: PathBuf| {
        if p.is_dir() && !out.contains(&p) {
            out.push(p);
        }
    };
    if let Some(h) = &env.java_home {
        push(h.clone());
    }
    if let Some(h) = &env.java_home_from_cmd {
        push(h.clone());
    }
    for pf in &env.program_files {
        for vendor in ["Java", "Eclipse Adoptium", "Microsoft", "Zulu", "Eclipse Foundation", "Amazon Corretto"] {
            for d in subdirs(&pf.join(vendor)) {
                push(d);
            }
        }
    }
    if let Some(home) = &env.home {
        for d in subdirs(&home.join(".sdkman").join("candidates").join("java")) {
            if d.file_name().is_some_and(|n| n != "current") {
                push(d);
            }
        }
        for d in subdirs(&home.join("scoop").join("apps")) {
            let name = d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
            if name.starts_with("openjdk") || name.starts_with("temurin") || name.contains("jdk") {
                push(d.join("current"));
            }
        }
    }
    for d in subdirs(Path::new("/Library/Java/JavaVirtualMachines")) {
        push(d.join("Contents").join("Home"));
    }
    out
}

pub fn discover_jdk(env: &DiscoverEnv) -> Vec<LibSource> {
    let mut out = Vec::new();
    let mut seen: Vec<PathBuf> = Vec::new();
    for home in jdk_homes(env) {
        let Some(zip) = src_zip_of(&home) else {
            continue;
        };
        let canon = std::fs::canonicalize(&zip).unwrap_or_else(|_| zip.clone());
        if seen.contains(&canon) {
            continue;
        }
        seen.push(canon);
        let (fingerprint, size) = stat_fingerprint(&zip);
        out.push(LibSource {
            id: format!("jdk:{}", id_of(&fingerprint)),
            kind: "jdk".into(),
            label: jdk_label(&home),
            artifacts: vec![Artifact {
                name: "src.zip".into(),
                kind: "zip".into(),
                path: Some(zip.clone()),
                fingerprint: fingerprint.clone(),
            }],
            path: zip,
            fingerprint,
            size_bytes: size,
            project_id: None,
            project_name: None,
        });
    }
    out
}

// ---- Cargo ----------------------------------------------------------------------

pub fn cargo_registry_src(env: &DiscoverEnv) -> Option<PathBuf> {
    env.cargo_home
        .clone()
        .or_else(|| env.home.as_ref().map(|h| h.join(".cargo")))
        .map(|c| c.join("registry").join("src"))
}

/// `serde-1.0.219` → `("serde", "1.0.219")`: the version starts at the first
/// `-<digit>` boundary.
pub fn split_crate_dir(name: &str) -> Option<(&str, &str)> {
    let mut idx = None;
    for (i, _) in name.match_indices('-') {
        if name[i + 1..].chars().next().is_some_and(|c| c.is_ascii_digit()) {
            idx = Some(i);
            break;
        }
    }
    let i = idx?;
    let (n, v) = (&name[..i], &name[i + 1..]);
    if n.is_empty() || v.is_empty() {
        return None;
    }
    Some((n, v))
}

/// `(name, version)` of every `[[package]]` with a `registry+…` source —
/// path / git / workspace packages are never in the registry.
pub fn cargo_lock_packages(text: &str) -> Vec<(String, String)> {
    let Ok(doc) = text.parse::<toml_edit::DocumentMut>() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    if let Some(pkgs) = doc.get("package").and_then(|p| p.as_array_of_tables()) {
        for t in pkgs {
            let source = t.get("source").and_then(|v| v.as_str()).unwrap_or("");
            if !source.starts_with("registry+") {
                continue;
            }
            let (Some(name), Some(version)) = (
                t.get("name").and_then(|v| v.as_str()),
                t.get("version").and_then(|v| v.as_str()),
            ) else {
                continue;
            };
            out.push((name.to_string(), version.to_string()));
        }
    }
    out.sort();
    out.dedup();
    out
}

/// `[workspace] members` of a `Cargo.toml` (a trailing `/*` glob expanded
/// against the dir), as dirs under `root`.
pub fn workspace_members(root: &Path, cargo_toml: &str) -> Vec<PathBuf> {
    let Ok(doc) = cargo_toml.parse::<toml_edit::DocumentMut>() else {
        return Vec::new();
    };
    let mut out = Vec::new();
    let members = doc
        .get("workspace")
        .and_then(|w| w.get("members"))
        .and_then(|m| m.as_array());
    if let Some(members) = members {
        for m in members.iter().filter_map(|v| v.as_str()) {
            let m = m.trim().trim_end_matches('/');
            if let Some(base) = m.strip_suffix("/*") {
                out.extend(subdirs(&root.join(base)));
            } else if !m.contains('*') {
                out.push(root.join(m));
            }
        }
    }
    out
}

const SKIP_DIRS: &[&str] = &["node_modules", "target", "dist", "build", "out", ".git"];

/// The `Cargo.lock` files a project root owns: at the root, in workspace
/// members, or one level down.
pub fn find_cargo_locks(root: &Path) -> Vec<PathBuf> {
    let mut out: Vec<PathBuf> = Vec::new();
    let mut push = |p: PathBuf| {
        if p.is_file() && !out.contains(&p) {
            out.push(p);
        }
    };
    push(root.join("Cargo.lock"));
    if let Ok(toml) = std::fs::read_to_string(root.join("Cargo.toml")) {
        for m in workspace_members(root, &toml) {
            push(m.join("Cargo.lock"));
        }
    }
    for d in subdirs(root) {
        let name = d.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default();
        if name.starts_with('.') || SKIP_DIRS.contains(&name.as_str()) {
            continue;
        }
        push(d.join("Cargo.lock"));
    }
    out
}

/// The registry checkout of `<name>-<version>` under any index dir.
pub fn cargo_crate_dir(registry_src: &Path, name: &str, version: &str) -> Option<PathBuf> {
    let dir_name = format!("{name}-{version}");
    for index in subdirs(registry_src) {
        let d = index.join(&dir_name);
        if d.join("Cargo.toml").is_file() && d.join("src").is_dir() {
            return Some(d);
        }
    }
    None
}

/// One artifact per pinned package (missing when not extracted yet).
pub fn cargo_artifacts(registry_src: Option<&Path>, packages: &[(String, String)]) -> Vec<Artifact> {
    packages
        .iter()
        .map(|(name, version)| {
            let dir_name = format!("{name}-{version}");
            Artifact {
                path: registry_src.and_then(|r| cargo_crate_dir(r, name, version)),
                fingerprint: dir_name.clone(),
                name: dir_name,
                kind: "crate".into(),
            }
        })
        .collect()
}

/// The project's cargo source (`None` without a lockfile).
pub fn discover_cargo(env: &DiscoverEnv, project: &LibProject) -> Option<LibSource> {
    let locks = find_cargo_locks(&project.root);
    if locks.is_empty() {
        return None;
    }
    let mut hasher = blake3::Hasher::new();
    let mut packages = Vec::new();
    for lock in &locks {
        let Ok(bytes) = std::fs::read(lock) else { continue };
        hasher.update(&bytes);
        packages.extend(cargo_lock_packages(&String::from_utf8_lossy(&bytes)));
    }
    packages.sort();
    packages.dedup();
    let registry = cargo_registry_src(env);
    Some(LibSource {
        id: format!("cargo:{}", project.id),
        kind: "cargo".into(),
        path: locks[0].clone(),
        label: format!("Cargo · {}", project.name),
        fingerprint: hasher.finalize().to_hex()[..16].to_string(),
        size_bytes: 0,
        project_id: Some(project.id.clone()),
        project_name: Some(project.name.clone()),
        artifacts: cargo_artifacts(registry.as_deref(), &packages),
    })
}

// ---- Maven ----------------------------------------------------------------------

pub fn m2_repository(env: &DiscoverEnv) -> Option<PathBuf> {
    env.m2_repository
        .clone()
        .or_else(|| env.home.as_ref().map(|h| h.join(".m2").join("repository")))
}

/// A minimal XML element tree: names, text, children (attributes, comments,
/// processing instructions and CDATA markers dropped) — all a pom needs.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct Xml {
    pub name: String,
    pub text: String,
    pub children: Vec<Xml>,
}

impl Xml {
    pub fn child(&self, name: &str) -> Option<&Xml> {
        self.children.iter().find(|c| c.name == name)
    }
    pub fn children_named<'a>(&'a self, name: &'a str) -> impl Iterator<Item = &'a Xml> + 'a {
        self.children.iter().filter(move |c| c.name == name)
    }
    /// Trimmed text of a direct child.
    pub fn text_of(&self, name: &str) -> Option<&str> {
        self.child(name).map(|c| c.text.trim()).filter(|t| !t.is_empty())
    }
}

/// Parse a document; `None` without a root element. Tolerant: a stray
/// close tag is ignored, an unclosed one closed at the end.
pub fn parse_xml(src: &str) -> Option<Xml> {
    let mut stack: Vec<Xml> = vec![Xml::default()];
    let mut rest = src;
    while let Some(lt) = rest.find('<') {
        let text = &rest[..lt];
        if let Some(top) = stack.last_mut() {
            top.text.push_str(text);
        }
        rest = &rest[lt..];
        if let Some(r) = rest.strip_prefix("<!--") {
            rest = r.find("-->").map(|i| &r[i + 3..]).unwrap_or("");
        } else if let Some(r) = rest.strip_prefix("<![CDATA[") {
            let end = r.find("]]>").unwrap_or(r.len());
            if let Some(top) = stack.last_mut() {
                top.text.push_str(&r[..end]);
            }
            rest = r.get(end + 3..).unwrap_or("");
        } else if let Some(r) = rest.strip_prefix("<?") {
            rest = r.find("?>").map(|i| &r[i + 2..]).unwrap_or("");
        } else if let Some(r) = rest.strip_prefix("<!") {
            rest = r.find('>').map(|i| &r[i + 1..]).unwrap_or("");
        } else if let Some(r) = rest.strip_prefix("</") {
            let end = r.find('>').unwrap_or(r.len());
            let name = r[..end].trim();
            rest = r.get(end + 1..).unwrap_or("");
            if stack.len() > 1 && stack.last().is_some_and(|t| t.name == name) {
                let done = stack.pop().expect("len > 1");
                stack.last_mut().expect("root frame").children.push(done);
            }
        } else {
            let r = &rest[1..];
            let end = r.find('>').unwrap_or(r.len());
            let inner = &r[..end];
            rest = r.get(end + 1..).unwrap_or("");
            let self_closing = inner.ends_with('/');
            let inner = inner.trim_end_matches('/');
            let name = inner
                .split(|c: char| c.is_whitespace())
                .next()
                .unwrap_or("")
                .to_string();
            if name.is_empty() {
                continue;
            }
            let el = Xml {
                name,
                ..Default::default()
            };
            if self_closing {
                stack.last_mut().expect("root frame").children.push(el);
            } else {
                stack.push(el);
            }
        }
    }
    while stack.len() > 1 {
        let done = stack.pop().expect("len > 1");
        stack.last_mut().expect("root frame").children.push(done);
    }
    stack.pop().and_then(|root| root.children.into_iter().next())
}

/// One `<dependency>` of a pom (versions resolved as far as the pom allows).
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MavenDep {
    pub group: String,
    pub artifact: String,
    /// `None` = left to a BOM / dependencyManagement / an unknown property.
    pub version: Option<String>,
}

/// `${x}` → its value from `props` (`project.version` & co. included),
/// recursively, unknown ones left in place.
pub fn resolve_props(v: &str, props: &HashMap<String, String>) -> String {
    let mut out = v.to_string();
    for _ in 0..8 {
        let Some(start) = out.find("${") else { break };
        let Some(len) = out[start..].find('}') else { break };
        let key = &out[start + 2..start + len];
        match props.get(key) {
            Some(val) => {
                let val = val.clone();
                out.replace_range(start..start + len + 1, &val);
            }
            None => break,
        }
    }
    out
}

/// `<properties>` + `project.groupId/version/artifactId` (+ `parent.*`) of a
/// pom; the parent's properties (when given) sit underneath.
pub fn pom_properties(pom: &Xml, parent: Option<&HashMap<String, String>>) -> HashMap<String, String> {
    let mut props: HashMap<String, String> = parent.cloned().unwrap_or_default();
    if let Some(p) = pom.child("properties") {
        for c in &p.children {
            props.insert(c.name.clone(), c.text.trim().to_string());
        }
    }
    let parent_el = pom.child("parent");
    let pv = parent_el.and_then(|p| p.text_of("version")).map(str::to_string);
    let pg = parent_el.and_then(|p| p.text_of("groupId")).map(str::to_string);
    let version = pom.text_of("version").map(str::to_string).or_else(|| pv.clone());
    let group = pom.text_of("groupId").map(str::to_string).or_else(|| pg.clone());
    if let Some(v) = version {
        props.insert("project.version".into(), v.clone());
        props.insert("version".into(), v.clone());
        props.insert("pom.version".into(), v);
    }
    if let Some(g) = group {
        props.insert("project.groupId".into(), g.clone());
        props.insert("groupId".into(), g.clone());
        props.insert("pom.groupId".into(), g);
    }
    if let Some(a) = pom.text_of("artifactId") {
        props.insert("project.artifactId".into(), a.to_string());
    }
    if let Some(v) = pv {
        props.insert("project.parent.version".into(), v.clone());
        props.insert("parent.version".into(), v);
    }
    if let Some(g) = pg {
        props.insert("project.parent.groupId".into(), g.clone());
        props.insert("parent.groupId".into(), g);
    }
    props
}

/// `<dependencyManagement>` versions of a pom (own and parent) by
/// `group:artifact`, so a managed dependency without a version resolves.
fn managed_versions(pom: &Xml, props: &HashMap<String, String>, into: &mut HashMap<String, String>) {
    let Some(deps) = pom.child("dependencyManagement").and_then(|m| m.child("dependencies")) else {
        return;
    };
    for d in deps.children_named("dependency") {
        let (Some(g), Some(a), Some(v)) = (d.text_of("groupId"), d.text_of("artifactId"), d.text_of("version")) else {
            continue;
        };
        let key = format!("{}:{}", resolve_props(g, props), a);
        into.entry(key).or_insert_with(|| resolve_props(v, props));
    }
}

/// The parent pom of `pom` (`<relativePath>`, default `../pom.xml`, else the
/// parent's `.pom` in the repository).
fn parent_pom(pom: &Xml, pom_dir: &Path, m2: Option<&Path>) -> Option<Xml> {
    let parent = pom.child("parent")?;
    let rel = parent.text_of("relativePath").unwrap_or("../pom.xml");
    if !rel.is_empty() {
        let mut p = pom_dir.join(rel);
        if p.is_dir() {
            p = p.join("pom.xml");
        }
        if let Ok(text) = std::fs::read_to_string(&p) {
            if let Some(x) = parse_xml(&text) {
                if x.text_of("artifactId") == parent.text_of("artifactId") {
                    return Some(x);
                }
            }
        }
    }
    let (g, a, v) = (parent.text_of("groupId")?, parent.text_of("artifactId")?, parent.text_of("version")?);
    let p = m2?.join(g.replace('.', "/")).join(a).join(v).join(format!("{a}-{v}.pom"));
    std::fs::read_to_string(p).ok().and_then(|t| parse_xml(&t))
}

/// The `<dependencies>` of a pom (project level only — never a plugin's),
/// `pom`-typed rows (BOM imports) dropped. `depth` bounds the parent chain.
pub fn pom_dependencies(pom: &Xml, pom_dir: &Path, m2: Option<&Path>) -> Vec<MavenDep> {
    // parent chain: properties + managed versions, root-most first
    let mut chain: Vec<Xml> = Vec::new();
    let mut cur = parent_pom(pom, pom_dir, m2);
    let mut dir = pom_dir.to_path_buf();
    let mut depth = 0;
    while let Some(p) = cur {
        depth += 1;
        if depth > 8 {
            break;
        }
        dir = dir.join("..");
        cur = parent_pom(&p, &dir, m2);
        chain.push(p);
    }
    let mut props: HashMap<String, String> = HashMap::new();
    let mut managed: HashMap<String, String> = HashMap::new();
    for p in chain.iter().rev() {
        props = pom_properties(p, Some(&props));
    }
    props = pom_properties(pom, Some(&props));
    for p in chain.iter() {
        managed_versions(p, &props, &mut managed);
    }
    managed_versions(pom, &props, &mut managed);
    let mut out = Vec::new();
    let Some(deps) = pom.child("dependencies") else {
        return out;
    };
    for d in deps.children_named("dependency") {
        if d.text_of("type").is_some_and(|t| t == "pom") {
            continue;
        }
        let (Some(g), Some(a)) = (d.text_of("groupId"), d.text_of("artifactId")) else {
            continue;
        };
        let group = resolve_props(g, &props);
        let artifact = resolve_props(a, &props);
        let version = d
            .text_of("version")
            .map(|v| resolve_props(v, &props))
            .or_else(|| managed.get(&format!("{group}:{artifact}")).cloned())
            .filter(|v| !v.contains("${") && !v.is_empty());
        out.push(MavenDep {
            group,
            artifact,
            version,
        });
    }
    out
}

/// The pom files a project root owns: the root pom + its `<modules>` (one
/// level).
pub fn find_poms(root: &Path) -> Vec<(PathBuf, Xml)> {
    let mut out = Vec::new();
    let path = root.join("pom.xml");
    let Some(pom) = std::fs::read_to_string(&path).ok().and_then(|t| parse_xml(&t)) else {
        return out;
    };
    let modules: Vec<String> = pom
        .child("modules")
        .map(|m| m.children_named("module").map(|c| c.text.trim().to_string()).collect())
        .unwrap_or_default();
    out.push((path, pom));
    for m in modules {
        let p = root.join(&m).join("pom.xml");
        if let Some(x) = std::fs::read_to_string(&p).ok().and_then(|t| parse_xml(&t)) {
            out.push((p, x));
        }
    }
    out
}

/// Newest `<artifact>-<ver>-sources.jar` under `<m2>/<group>/<artifact>/`.
fn newest_sources_jar(artifact_dir: &Path, artifact: &str) -> Option<PathBuf> {
    subdirs(artifact_dir)
        .into_iter()
        .filter_map(|d| {
            let v = d.file_name()?.to_string_lossy().to_string();
            let jar = d.join(format!("{artifact}-{v}-sources.jar"));
            jar.is_file().then(|| (version_key(&v), jar))
        })
        .max_by(|a, b| a.0.cmp(&b.0))
        .map(|(_, p)| p)
}

/// One artifact per dependency: the pinned sources jar, else (no version)
/// the newest one on disk, else missing.
pub fn maven_artifacts(m2: Option<&Path>, deps: &[MavenDep]) -> Vec<Artifact> {
    let mut out: Vec<Artifact> = Vec::new();
    for d in deps {
        let artifact_dir = m2.map(|m| m.join(d.group.replace('.', "/")).join(&d.artifact));
        let path = match (&artifact_dir, &d.version) {
            (Some(dir), Some(v)) => {
                let p = dir.join(v).join(format!("{}-{v}-sources.jar", d.artifact));
                p.is_file().then_some(p)
            }
            (Some(dir), None) => newest_sources_jar(dir, &d.artifact),
            (None, _) => None,
        };
        let name = match (&path, &d.version) {
            (Some(p), _) => p.file_name().map(|n| n.to_string_lossy().to_string()).unwrap_or_default(),
            (None, Some(v)) => format!("{}-{v}-sources.jar", d.artifact),
            (None, None) => format!("{}-sources.jar", d.artifact),
        };
        if out.iter().any(|a| a.name == name) {
            continue;
        }
        let fingerprint = path.as_deref().map(|p| stat_fingerprint(p).0).unwrap_or_default();
        out.push(Artifact {
            name,
            kind: "jar".into(),
            path,
            fingerprint,
        });
    }
    out
}

/// The project's maven source (`None` without a `pom.xml`). A Gradle-only
/// project is not read (v1).
pub fn discover_maven(env: &DiscoverEnv, project: &LibProject) -> Option<LibSource> {
    let poms = find_poms(&project.root);
    if poms.is_empty() {
        return None;
    }
    let m2 = m2_repository(env);
    let mut hasher = blake3::Hasher::new();
    let mut deps = Vec::new();
    for (path, pom) in &poms {
        if let Ok(bytes) = std::fs::read(path) {
            hasher.update(&bytes);
        }
        let dir = path.parent().unwrap_or(&project.root);
        deps.extend(pom_dependencies(pom, dir, m2.as_deref()));
    }
    Some(LibSource {
        id: format!("maven:{}", project.id),
        kind: "maven".into(),
        path: poms[0].0.clone(),
        label: format!("Maven · {}", project.name),
        fingerprint: hasher.finalize().to_hex()[..16].to_string(),
        size_bytes: 0,
        project_id: Some(project.id.clone()),
        project_name: Some(project.name.clone()),
        artifacts: maven_artifacts(m2.as_deref(), &deps),
    })
}

// ---- all ------------------------------------------------------------------------

/// A project's lockfile sources (cargo, then maven).
pub fn discover_project(env: &DiscoverEnv, project: &LibProject) -> Vec<LibSource> {
    let mut out = Vec::new();
    out.extend(discover_cargo(env, project));
    out.extend(discover_maven(env, project));
    out
}

/// Everything: JDKs, then every project's lockfile sources.
pub fn discover_all(env: &DiscoverEnv, projects: &[LibProject]) -> Vec<LibSource> {
    let mut out = discover_jdk(env);
    for p in projects {
        out.extend(discover_project(env, p));
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn project(root: &Path) -> LibProject {
        LibProject {
            id: "p-1".into(),
            name: "shop".into(),
            root: root.to_path_buf(),
        }
    }

    #[test]
    fn parses_java_home_from_showsettings() {
        let dump = "Property settings:\n    java.class.path = \n    java.home = C:\\jdk\\21\n    java.vendor = x\n";
        assert_eq!(parse_java_home(dump), Some(PathBuf::from("C:\\jdk\\21")));
        assert_eq!(parse_java_home("nothing here"), None);
    }

    #[test]
    fn jdk_labels_from_release_or_dir_name() {
        let t = tempfile::tempdir().unwrap();
        let home = t.path().join("scoop").join("apps").join("openjdk21").join("current");
        fs::create_dir_all(home.join("lib")).unwrap();
        assert_eq!(jdk_dir_name(&home), "openjdk21");
        assert_eq!(jdk_major(&home).as_deref(), Some("21"));
        fs::write(home.join("release"), "IMPLEMENTOR=\"x\"\nJAVA_VERSION=\"17.0.9\"\n").unwrap();
        assert_eq!(jdk_major(&home).as_deref(), Some("17"));
        assert_eq!(jdk_label(&home), "JDK 17 (openjdk21)");
        fs::write(home.join("release"), "JAVA_VERSION=\"1.8.0_392\"\n").unwrap();
        assert_eq!(jdk_major(&home).as_deref(), Some("8"));
        let mac = t.path().join("JavaVirtualMachines").join("temurin-21.jdk").join("Contents").join("Home");
        fs::create_dir_all(&mac).unwrap();
        assert_eq!(jdk_dir_name(&mac), "temurin-21.jdk");
        assert_eq!(jdk_major(&mac).as_deref(), Some("21"));
    }

    #[test]
    fn discovers_jdk_in_precedence_order_without_duplicates() {
        let t = tempfile::tempdir().unwrap();
        let mk = |rel: &str| {
            let home = t.path().join(rel);
            fs::create_dir_all(home.join("lib")).unwrap();
            fs::write(home.join("lib").join("src.zip"), b"PK").unwrap();
            home
        };
        let jh = mk("java_home");
        let pf = t.path().join("pf");
        let vendor = mk("pf/Eclipse Adoptium/jdk-17");
        fs::create_dir_all(pf.join("Java").join("jre-8")).unwrap(); // a home without src.zip
        let home = t.path().join("home");
        let scoop = mk("home/scoop/apps/openjdk21/current");
        let env = DiscoverEnv {
            java_home: Some(jh.clone()),
            java_home_from_cmd: Some(jh.clone()), // duplicate of JAVA_HOME
            program_files: vec![pf.clone()],
            home: Some(home.clone()),
            ..Default::default()
        };
        let found = discover_jdk(&env);
        let paths: Vec<&Path> = found.iter().map(|s| s.path.as_path()).collect();
        assert_eq!(
            paths,
            vec![
                jh.join("lib").join("src.zip").as_path(),
                vendor.join("lib").join("src.zip").as_path(),
                scoop.join("lib").join("src.zip").as_path(),
            ]
        );
        assert!(found.iter().all(|s| s.kind == "jdk" && s.project_id.is_none()));
        assert_eq!(found[2].label, "JDK 21 (openjdk21)");
        assert_eq!(found[1].label, "JDK 17 (jdk-17)");
        assert!(found[0].id.starts_with("jdk:"));
        assert_eq!(found[0].id.len(), 20);
        assert!(found[0].fingerprint.contains("|2|"));
        assert_eq!(found[0].artifacts.len(), 1);
        assert_eq!(found[0].artifacts[0].name, "src.zip");
        assert_eq!(found[0].artifacts[0].path.as_deref(), Some(found[0].path.as_path()));
        assert_eq!(found[0].missing(), 0);
        // a missing home / zip is simply skipped
        let empty = DiscoverEnv {
            java_home: Some(t.path().join("nope")),
            ..Default::default()
        };
        assert!(discover_jdk(&empty).is_empty());
    }

    const LOCK: &str = r#"# generated
version = 4

[[package]]
name = "serde"
version = "1.0.219"
source = "registry+https://github.com/rust-lang/crates.io-index"
checksum = "abc"
dependencies = [
 "serde_derive",
]

[[package]]
name = "orrery"
version = "0.23.1"
dependencies = [
 "serde",
]

[[package]]
name = "gix"
version = "0.70.0"
source = "git+https://github.com/GitoxideLabs/gitoxide?rev=abc#abc"

[[package]]
name = "windows-sys"
version = "0.52.0"
source = "registry+https://github.com/rust-lang/crates.io-index"

[[package]]
name = "windows-sys"
version = "0.59.0"
source = "registry+https://github.com/rust-lang/crates.io-index"
"#;

    #[test]
    fn cargo_lock_pins_registry_packages_only() {
        assert_eq!(split_crate_dir("serde-1.0.219"), Some(("serde", "1.0.219")));
        assert_eq!(split_crate_dir("windows-sys-0.61.2"), Some(("windows-sys", "0.61.2")));
        assert_eq!(split_crate_dir("noversion"), None);
        let pkgs = cargo_lock_packages(LOCK);
        assert_eq!(
            pkgs,
            vec![
                ("serde".to_string(), "1.0.219".to_string()),
                ("windows-sys".to_string(), "0.52.0".to_string()),
                ("windows-sys".to_string(), "0.59.0".to_string()),
            ],
            "path + git packages dropped, both pinned versions kept"
        );
        assert!(cargo_lock_packages("not = [toml").is_empty());
    }

    #[test]
    fn cargo_source_follows_the_pinned_versions_on_disk() {
        let t = tempfile::tempdir().unwrap();
        let reg = t.path().join("cargo").join("registry").join("src");
        let idx = reg.join("index.crates.io-1949cf8c6b5b557f");
        // serde 1.0.219 (pinned) + 1.0.300 (newer, NOT wanted) + windows-sys 0.52 only
        for name in ["serde-1.0.219", "serde-1.0.300", "windows-sys-0.52.0", "broken-0.1.0"] {
            let d = idx.join(name);
            fs::create_dir_all(d.join("src")).unwrap();
            if name != "broken-0.1.0" {
                fs::write(d.join("Cargo.toml"), "[package]\n").unwrap();
            }
        }
        let root = t.path().join("proj");
        fs::create_dir_all(root.join("src-tauri")).unwrap();
        fs::write(root.join("src-tauri").join("Cargo.lock"), LOCK).unwrap();
        let env = DiscoverEnv {
            cargo_home: Some(t.path().join("cargo")),
            ..Default::default()
        };
        let src = discover_cargo(&env, &project(&root)).expect("a lock one level down counts");
        assert_eq!(src.id, "cargo:p-1");
        assert_eq!(src.kind, "cargo");
        assert_eq!(src.label, "Cargo · shop");
        assert_eq!(src.path, root.join("src-tauri").join("Cargo.lock"));
        assert_eq!(src.project_id.as_deref(), Some("p-1"));
        assert_eq!(src.fingerprint.len(), 16);
        let names: Vec<(&str, bool)> = src.artifacts.iter().map(|a| (a.name.as_str(), a.path.is_some())).collect();
        assert_eq!(
            names,
            vec![("serde-1.0.219", true), ("windows-sys-0.52.0", true), ("windows-sys-0.59.0", false)]
        );
        assert_eq!(src.artifacts[0].path.as_deref(), Some(idx.join("serde-1.0.219").as_path()));
        assert_eq!(src.artifacts[0].kind, "crate");
        assert_eq!(src.artifacts[0].fingerprint, "serde-1.0.219");
        assert_eq!(src.missing(), 1);
        // the fingerprint follows the lock bytes
        fs::write(root.join("src-tauri").join("Cargo.lock"), format!("{LOCK}\n# touched\n")).unwrap();
        assert_ne!(discover_cargo(&env, &project(&root)).unwrap().fingerprint, src.fingerprint);
        // no lock anywhere → no source; a workspace member's lock is found
        let empty = t.path().join("empty");
        fs::create_dir_all(&empty).unwrap();
        assert!(discover_cargo(&env, &project(&empty)).is_none());
        let ws = t.path().join("ws");
        fs::create_dir_all(ws.join("crates").join("a")).unwrap();
        fs::write(ws.join("Cargo.toml"), "[workspace]\nmembers = [\"crates/*\", \"tool\"]\n").unwrap();
        fs::write(ws.join("crates").join("a").join("Cargo.lock"), LOCK).unwrap();
        assert_eq!(find_cargo_locks(&ws), vec![ws.join("crates").join("a").join("Cargo.lock")]);
        assert_eq!(workspace_members(&ws, "[workspace]\nmembers = [\"tool\"]").len(), 1);
        assert!(discover_cargo(&env, &project(&ws)).is_some());
        // a registry that is not there → every crate missing, still a source
        let none = DiscoverEnv::default();
        assert_eq!(discover_cargo(&none, &project(&root)).unwrap().missing(), 3);
    }

    const POM: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<!-- a comment with <dependency> inside -->
<project xmlns="http://maven.apache.org/POM/4.0.0">
  <modelVersion>4.0.0</modelVersion>
  <parent>
    <groupId>com.acme</groupId>
    <artifactId>acme-parent</artifactId>
    <version>2.0</version>
    <relativePath>../parent/pom.xml</relativePath>
  </parent>
  <artifactId>shop</artifactId>
  <properties>
    <gson.version>2.11.0</gson.version>
    <lang3.version>${commons.version}</lang3.version>
  </properties>
  <dependencyManagement>
    <dependencies>
      <dependency><groupId>org.slf4j</groupId><artifactId>slf4j-api</artifactId><version>2.0.9</version></dependency>
    </dependencies>
  </dependencyManagement>
  <dependencies>
    <dependency>
      <groupId>com.google.code.gson</groupId>
      <artifactId>gson</artifactId>
      <version>${gson.version}</version>
    </dependency>
    <dependency>
      <groupId>org.apache.commons</groupId>
      <artifactId>commons-lang3</artifactId>
      <version>${lang3.version}</version>
    </dependency>
    <dependency>
      <groupId>org.slf4j</groupId>
      <artifactId>slf4j-api</artifactId>
    </dependency>
    <dependency>
      <groupId>${project.groupId}</groupId>
      <artifactId>acme-core</artifactId>
      <version>${project.version}</version>
    </dependency>
    <dependency>
      <groupId>org.springframework.boot</groupId>
      <artifactId>spring-boot-starter</artifactId>
    </dependency>
    <dependency>
      <groupId>org.springframework.boot</groupId>
      <artifactId>spring-boot-dependencies</artifactId>
      <version>3.0.0</version>
      <type>pom</type>
      <scope>import</scope>
    </dependency>
    <dependency>
      <groupId>x</groupId>
      <artifactId>unknown-prop</artifactId>
      <version>${nope.version}</version>
    </dependency>
  </dependencies>
  <build><plugins><plugin><groupId>p</groupId><artifactId>plugin</artifactId><version>1</version></plugin></plugins></build>
</project>
"#;

    const PARENT_POM: &str = r#"<project>
  <groupId>com.acme</groupId>
  <artifactId>acme-parent</artifactId>
  <version>2.0</version>
  <properties>
    <commons.version>3.14.0</commons.version>
  </properties>
  <dependencyManagement><dependencies>
    <dependency><groupId>org.springframework.boot</groupId><artifactId>spring-boot-starter</artifactId><version>${boot.version}</version></dependency>
  </dependencies></dependencyManagement>
</project>"#;

    #[test]
    fn xml_parser_handles_comments_cdata_and_self_closing() {
        let x = parse_xml("<?xml version=\"1.0\"?><!-- <a/> --><r a=\"1\"><b>t<![CDATA[<c>]]></b><d/><e>x</e></r>").unwrap();
        assert_eq!(x.name, "r");
        assert_eq!(x.text_of("b"), Some("t<c>"));
        assert_eq!(x.child("d").map(|d| d.children.len()), Some(0));
        assert_eq!(x.children.len(), 3);
        assert!(parse_xml("").is_none());
        // unclosed + stray close tags are tolerated
        let x = parse_xml("<r><a>1</b></r>").unwrap();
        assert_eq!(x.child("a").map(|a| a.text.as_str()), Some("1"));
        assert_eq!(resolve_props("${a}-${b}", &HashMap::from([("a".to_string(), "1".to_string())])), "1-${b}");
    }

    #[test]
    fn pom_dependencies_resolve_properties_parent_and_management() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("shop");
        let parent = t.path().join("parent");
        fs::create_dir_all(&root).unwrap();
        fs::create_dir_all(&parent).unwrap();
        fs::write(root.join("pom.xml"), POM).unwrap();
        fs::write(parent.join("pom.xml"), PARENT_POM).unwrap();
        let pom = parse_xml(POM).unwrap();
        let deps = pom_dependencies(&pom, &root, None);
        let rows: Vec<(&str, &str, Option<&str>)> = deps.iter().map(|d| (d.group.as_str(), d.artifact.as_str(), d.version.as_deref())).collect();
        assert_eq!(
            rows,
            vec![
                ("com.google.code.gson", "gson", Some("2.11.0")),
                ("org.apache.commons", "commons-lang3", Some("3.14.0")),
                ("org.slf4j", "slf4j-api", Some("2.0.9")),
                ("com.acme", "acme-core", Some("2.0")),
                ("org.springframework.boot", "spring-boot-starter", None),
                ("x", "unknown-prop", None),
            ],
            "own props, the parent's props, own + parent management, project.* — the BOM import dropped, an unresolved property → None"
        );
        // a parent found through ~/.m2 instead of relativePath
        let m2 = t.path().join("m2");
        let ppath = m2.join("com").join("acme").join("acme-parent").join("2.0");
        fs::create_dir_all(&ppath).unwrap();
        fs::write(ppath.join("acme-parent-2.0.pom"), PARENT_POM).unwrap();
        fs::remove_file(parent.join("pom.xml")).unwrap();
        let deps = pom_dependencies(&pom, &root, Some(&m2));
        assert_eq!(deps[1].version.as_deref(), Some("3.14.0"));
        assert_eq!(pom_dependencies(&pom, &root, None)[1].version, None, "no parent anywhere → unresolved");
    }

    #[test]
    fn maven_source_maps_dependencies_to_sources_jars() {
        let t = tempfile::tempdir().unwrap();
        let root = t.path().join("shop");
        fs::create_dir_all(root.join("api")).unwrap();
        fs::write(
            root.join("pom.xml"),
            "<project><groupId>com.acme</groupId><artifactId>shop</artifactId><version>1</version>
             <modules><module>api</module></modules>
             <dependencies>
               <dependency><groupId>com.google.code.gson</groupId><artifactId>gson</artifactId><version>2.11.0</version></dependency>
               <dependency><groupId>org.slf4j</groupId><artifactId>slf4j-api</artifactId></dependency>
               <dependency><groupId>x</groupId><artifactId>absent</artifactId><version>9</version></dependency>
               <dependency><groupId>x</groupId><artifactId>never</artifactId></dependency>
             </dependencies></project>",
        )
        .unwrap();
        fs::write(
            root.join("api").join("pom.xml"),
            "<project><parent><groupId>com.acme</groupId><artifactId>shop</artifactId><version>1</version></parent><artifactId>api</artifactId>
             <dependencies>
               <dependency><groupId>com.google.code.gson</groupId><artifactId>gson</artifactId><version>2.11.0</version></dependency>
               <dependency><groupId>com.acme</groupId><artifactId>gradle-only</artifactId><version>1</version></dependency>
             </dependencies></project>",
        )
        .unwrap();
        let m2 = t.path().join("m2");
        let gson = m2.join("com").join("google").join("code").join("gson").join("gson").join("2.11.0");
        fs::create_dir_all(&gson).unwrap();
        fs::write(gson.join("gson-2.11.0.jar"), b"PK").unwrap();
        fs::write(gson.join("gson-2.11.0-sources.jar"), b"PK").unwrap();
        for v in ["1.7.36", "2.0.9", "2.0.13"] {
            let d = m2.join("org").join("slf4j").join("slf4j-api").join(v);
            fs::create_dir_all(&d).unwrap();
            if v != "2.0.13" {
                fs::write(d.join(format!("slf4j-api-{v}-sources.jar")), b"PK").unwrap();
            }
        }
        let env = DiscoverEnv {
            m2_repository: Some(m2.clone()),
            ..Default::default()
        };
        let src = discover_maven(&env, &project(&root)).unwrap();
        assert_eq!(src.id, "maven:p-1");
        assert_eq!(src.label, "Maven · shop");
        assert_eq!(src.path, root.join("pom.xml"));
        let names: Vec<(&str, bool)> = src.artifacts.iter().map(|a| (a.name.as_str(), a.path.is_some())).collect();
        assert_eq!(
            names,
            vec![
                ("gson-2.11.0-sources.jar", true),
                ("slf4j-api-2.0.9-sources.jar", true),
                ("absent-9-sources.jar", false),
                ("never-sources.jar", false),
                ("gradle-only-1-sources.jar", false),
            ],
            "pinned jar, newest jar on disk for an unversioned dep, missing rows, the module's deps deduplicated"
        );
        assert_eq!(src.artifacts[0].kind, "jar");
        assert!(src.artifacts[0].fingerprint.contains("|2|"));
        assert_eq!(src.missing(), 3);
        // gradle only → nothing (v1)
        let g = t.path().join("gradle");
        fs::create_dir_all(&g).unwrap();
        fs::write(g.join("build.gradle.kts"), "").unwrap();
        assert!(discover_maven(&env, &project(&g)).is_none());
        // discover_all: jdk first, then the project's sources
        let all = discover_all(&env, &[project(&root)]);
        assert_eq!(all.iter().map(|s| s.kind.as_str()).collect::<Vec<_>>(), vec!["maven"]);
        assert_eq!(id_of("x").len(), 16);
    }
}
