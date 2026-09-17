//! Extension pack manifests + the registry `index.json`: serde types and
//! validation. Unknown fields are ignored on purpose — the build pipeline may
//! add keys before the app learns about them. Validation is strict about the
//! few fields that become filesystem paths (`id`, `version`, `library`, query
//! paths): an installed pack lives at `extensions/<id>/<version>/`, so a
//! manifest must never be able to steer that outside the extensions root.

use std::collections::BTreeMap;

use serde::{Deserialize, Serialize};
use serde_json::Value;

/// The one manifest/index schema this build understands.
pub const SCHEMA: u32 = 1;

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct GrammarQueries {
    /// Path (inside the pack) of the tags query; `None` = no symbol extraction.
    pub tags: Option<String>,
    /// Path of the locals query; `None` = upstream ships none (java).
    pub locals: Option<String>,
}

/// `kind: "grammar"` — a prebuilt tree-sitter parser dylib + its queries.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct GrammarManifest {
    #[serde(default)]
    pub schema: u32,
    pub id: String,
    pub kind: String,
    pub version: String,
    #[serde(default)]
    pub label: String,
    pub language: String,
    #[serde(default)]
    pub monaco_language: Option<String>,
    #[serde(default)]
    pub file_extensions: Vec<String>,
    /// Exported C symbol returning the `TSLanguage*` (e.g. `tree_sitter_java`).
    pub symbol: String,
    /// tree-sitter language ABI the parser was generated for.
    pub abi: u32,
    /// Dylib file name relative to the pack root (no directories).
    pub library: String,
    #[serde(default)]
    pub queries: GrammarQueries,
    #[serde(default)]
    pub upstream: Option<Value>,
    #[serde(default)]
    pub target: String,
    #[serde(default)]
    pub min_orrery_version: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Requirement {
    pub program: String,
    pub min_version: Option<String>,
    pub env: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerDetect {
    /// Executable names to look for on PATH (first hit wins).
    pub path: Vec<String>,
    /// Env vars whose value is a dir holding the program (e.g. `JDTLS_HOME`).
    pub env: Vec<String>,
    /// Prerequisites (runtime, minimum version) — surfaced as the `hint`.
    pub requires: Vec<Requirement>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct ServerDownload {
    pub url: String,
    pub sha256: String,
    pub unpack: Option<String>,
    /// Program path relative to the unpacked dir.
    pub entry: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct VirtualRead {
    pub method: String,
}

/// How a SELF-CONTAINED server pack launches: `entry` (relative to the pack
/// dir, one `*` glob per segment allowed — `plugins/org.eclipse.equinox.launcher_*.jar`)
/// run by the downloaded `runtime` (`node` | `java`) or directly (`null`).
/// Placeholders (`${extDir}`, `${workspaceStorage}`, `${projectRoot}`,
/// `${jdtlsConfig}`) are substituted in `args` / `jvmArgs`.
#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct Launch {
    /// `"node" | "java"` — the runtime pack that runs `entry`; `None` = the
    /// entry is itself the executable.
    pub runtime: Option<String>,
    pub entry: String,
    pub args: Vec<String>,
    /// JVM flags placed before `-jar` (java runtimes only).
    pub jvm_args: Vec<String>,
}

/// `kind: "runtime"` — a downloaded Node / Java runtime other packs run on.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RuntimeManifest {
    #[serde(default)]
    pub schema: u32,
    pub id: String,
    pub kind: String,
    pub version: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub target: String,
    /// `"node" | "java"` — what `launch.runtime` names.
    pub runtime: String,
    /// Executable relative to the pack root (`node.exe`, `bin/java.exe`,
    /// `Contents/Home/bin/java`).
    pub binary: String,
    #[serde(default)]
    pub min_orrery_version: Option<String>,
}

/// `kind: "server"` — how to find/launch a language server.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ServerManifest {
    #[serde(default)]
    pub schema: u32,
    pub id: String,
    pub kind: String,
    pub version: String,
    #[serde(default)]
    pub label: String,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub transport: Option<String>,
    #[serde(default)]
    pub program: String,
    #[serde(default)]
    pub args: Vec<String>,
    #[serde(default)]
    pub detect: ServerDetect,
    #[serde(default)]
    pub download: BTreeMap<String, ServerDownload>,
    #[serde(default)]
    pub initialization_options: Option<Value>,
    #[serde(default)]
    pub settings: Option<Value>,
    #[serde(default)]
    pub virtual_schemes: Vec<String>,
    #[serde(default)]
    pub virtual_read: Option<VirtualRead>,
    #[serde(default)]
    pub min_orrery_version: Option<String>,
    /// Pack ids installed before/with this one (`runtime.node`, …).
    #[serde(default)]
    pub requires: Vec<String>,
    /// Self-contained launch; `None` = legacy pack (system `detect` only).
    #[serde(default)]
    pub launch: Option<Launch>,
}

#[derive(Debug, Clone, PartialEq)]
pub enum Manifest {
    Grammar(GrammarManifest),
    Server(ServerManifest),
    Runtime(RuntimeManifest),
}

impl Manifest {
    /// Parse + validate `manifest.json`. The `kind` field selects the shape.
    pub fn parse(json: &str) -> Result<Self, String> {
        let v: Value =
            serde_json::from_str(json).map_err(|e| format!("manifest: invalid json: {e}"))?;
        let kind = v.get("kind").and_then(Value::as_str).unwrap_or("");
        let m = match kind {
            "grammar" => Manifest::Grammar(
                serde_json::from_value(v).map_err(|e| format!("manifest: {e}"))?,
            ),
            "server" => Manifest::Server(
                serde_json::from_value(v).map_err(|e| format!("manifest: {e}"))?,
            ),
            "runtime" => Manifest::Runtime(
                serde_json::from_value(v).map_err(|e| format!("manifest: {e}"))?,
            ),
            other => return Err(format!("manifest: unknown kind {other:?}")),
        };
        m.validate()?;
        Ok(m)
    }

    pub fn validate(&self) -> Result<(), String> {
        let (schema, id, kind, version) = match self {
            Manifest::Grammar(g) => (g.schema, &g.id, &g.kind, &g.version),
            Manifest::Server(s) => (s.schema, &s.id, &s.kind, &s.version),
            Manifest::Runtime(r) => (r.schema, &r.id, &r.kind, &r.version),
        };
        if schema != SCHEMA {
            return Err(format!("manifest: unsupported schema {schema} (want {SCHEMA})"));
        }
        validate_id(id)?;
        if !is_safe_segment(version) {
            return Err(format!("manifest: unsafe version {version:?}"));
        }
        match self {
            Manifest::Grammar(g) => {
                if kind != "grammar" {
                    return Err("manifest: kind mismatch".into());
                }
                if !id.starts_with("grammar.") {
                    return Err(format!("manifest: grammar id must start with 'grammar.': {id}"));
                }
                if g.language.trim().is_empty() {
                    return Err("manifest: language missing".into());
                }
                if !is_c_identifier(&g.symbol) {
                    return Err(format!("manifest: symbol {:?} is not an identifier", g.symbol));
                }
                if g.abi == 0 {
                    return Err("manifest: abi missing".into());
                }
                if !is_safe_segment(&g.library) {
                    return Err(format!(
                        "manifest: library {:?} must be a bare file name",
                        g.library
                    ));
                }
                for q in [&g.queries.tags, &g.queries.locals].into_iter().flatten() {
                    if !is_safe_relative(q) {
                        return Err(format!("manifest: unsafe query path {q:?}"));
                    }
                }
                for ext in &g.file_extensions {
                    if ext.is_empty() || ext.contains(['/', '\\', '.']) {
                        return Err(format!("manifest: bad file extension {ext:?}"));
                    }
                }
            }
            Manifest::Server(s) => {
                if kind != "server" {
                    return Err("manifest: kind mismatch".into());
                }
                if !id.starts_with("server.") {
                    return Err(format!("manifest: server id must start with 'server.': {id}"));
                }
                if s.program.trim().is_empty() && s.download.is_empty() && s.launch.is_none() {
                    return Err("manifest: server needs a launch, a program or a download".into());
                }
                for (target, d) in &s.download {
                    if d.url.is_empty() || d.sha256.len() != 64 {
                        return Err(format!("manifest: download[{target}] needs url + sha256"));
                    }
                    if let Some(entry) = &d.entry {
                        if !is_safe_relative(entry) {
                            return Err(format!("manifest: unsafe entry {entry:?}"));
                        }
                    }
                }
                for dep in &s.requires {
                    validate_id(dep)?;
                    if dep == id {
                        return Err(format!("manifest: {id} requires itself"));
                    }
                }
                if let Some(l) = &s.launch {
                    if !is_safe_relative_glob(&l.entry) {
                        return Err(format!("manifest: unsafe launch entry {:?}", l.entry));
                    }
                    match l.runtime.as_deref() {
                        None | Some("node") | Some("java") => {}
                        Some(other) => {
                            return Err(format!("manifest: unknown launch runtime {other:?}"))
                        }
                    }
                    if let Some(rt) = &l.runtime {
                        let dep = format!("runtime.{rt}");
                        if !s.requires.iter().any(|r| r == &dep) {
                            return Err(format!(
                                "manifest: launch.runtime {rt:?} but requires lacks {dep}"
                            ));
                        }
                    }
                }
            }
            Manifest::Runtime(r) => {
                if kind != "runtime" {
                    return Err("manifest: kind mismatch".into());
                }
                if !id.starts_with("runtime.") {
                    return Err(format!("manifest: runtime id must start with 'runtime.': {id}"));
                }
                if !matches!(r.runtime.as_str(), "node" | "java") {
                    return Err(format!("manifest: unknown runtime {:?}", r.runtime));
                }
                if !is_safe_relative(&r.binary) {
                    return Err(format!("manifest: unsafe runtime binary {:?}", r.binary));
                }
            }
        }
        Ok(())
    }

    pub fn id(&self) -> &str {
        match self {
            Manifest::Grammar(g) => &g.id,
            Manifest::Server(s) => &s.id,
            Manifest::Runtime(r) => &r.id,
        }
    }

    pub fn kind(&self) -> &str {
        match self {
            Manifest::Grammar(_) => "grammar",
            Manifest::Server(_) => "server",
            Manifest::Runtime(_) => "runtime",
        }
    }

    pub fn version(&self) -> &str {
        match self {
            Manifest::Grammar(g) => &g.version,
            Manifest::Server(s) => &s.version,
            Manifest::Runtime(r) => &r.version,
        }
    }

    pub fn label(&self) -> &str {
        match self {
            Manifest::Grammar(g) => &g.label,
            Manifest::Server(s) => &s.label,
            Manifest::Runtime(r) => &r.label,
        }
    }

    pub fn languages(&self) -> Vec<String> {
        match self {
            Manifest::Grammar(g) => vec![g.language.clone()],
            Manifest::Server(s) => s.languages.clone(),
            Manifest::Runtime(_) => Vec::new(),
        }
    }

    pub fn min_orrery_version(&self) -> Option<&str> {
        match self {
            Manifest::Grammar(g) => g.min_orrery_version.as_deref(),
            Manifest::Server(s) => s.min_orrery_version.as_deref(),
            Manifest::Runtime(r) => r.min_orrery_version.as_deref(),
        }
    }

    /// Grammar ABI (`None` for servers/runtimes — no ABI gate applies).
    pub fn abi(&self) -> Option<u32> {
        match self {
            Manifest::Grammar(g) => Some(g.abi),
            Manifest::Server(_) | Manifest::Runtime(_) => None,
        }
    }

    /// Pack ids this one needs installed (servers only today).
    pub fn requires(&self) -> &[String] {
        match self {
            Manifest::Server(s) => &s.requires,
            Manifest::Grammar(_) | Manifest::Runtime(_) => &[],
        }
    }

    /// The pack's own `target` claim (`""` = any).
    pub fn target(&self) -> &str {
        match self {
            Manifest::Grammar(g) => &g.target,
            Manifest::Runtime(r) => &r.target,
            Manifest::Server(_) => "",
        }
    }
}

/// One path segment that can't escape its directory: `[A-Za-z0-9._-]+`,
/// not `.`/`..`.
pub fn is_safe_segment(s: &str) -> bool {
    !s.is_empty()
        && s != "."
        && s != ".."
        && s
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || matches!(c, '.' | '_' | '-'))
}

/// A `/`-joined relative path made only of safe segments.
pub fn is_safe_relative(p: &str) -> bool {
    !p.is_empty() && !p.contains('\\') && p.split('/').all(is_safe_segment)
}

/// [`is_safe_relative`] where a segment may also carry `*` wildcards
/// (`plugins/org.eclipse.equinox.launcher_*.jar`).
pub fn is_safe_relative_glob(p: &str) -> bool {
    !p.is_empty()
        && !p.contains('\\')
        && p.split('/').all(|seg| {
            let bare: String = seg.chars().filter(|c| *c != '*').collect();
            seg != "*" && !bare.is_empty() && is_safe_segment(&bare)
        })
}

fn is_c_identifier(s: &str) -> bool {
    let mut chars = s.chars();
    matches!(chars.next(), Some(c) if c == '_' || c.is_ascii_alphabetic())
        && chars.all(|c| c == '_' || c.is_ascii_alphanumeric())
}

/// Pack ids are dotted lowercase words (`grammar.java`, `server.jdtls`) and
/// double as directory names.
pub fn validate_id(id: &str) -> Result<(), String> {
    if !is_safe_segment(id) || !id.contains('.') {
        return Err(format!("manifest: unsafe id {id:?}"));
    }
    Ok(())
}

// ---- registry index ----

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RegistryTarget {
    pub url: String,
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct RegistryPack {
    pub id: String,
    pub kind: String,
    pub version: String,
    #[serde(default)]
    pub name: Option<String>,
    #[serde(default)]
    pub label: Option<String>,
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub languages: Vec<String>,
    #[serde(default)]
    pub abi: Option<u32>,
    #[serde(default)]
    pub min_orrery_version: Option<String>,
    /// Mirrors the manifest's `requires` so the host can plan an install
    /// (deps first) before a single byte of the pack is fetched.
    #[serde(default)]
    pub requires: Vec<String>,
    /// `"<target>" | "any"` → artifact.
    #[serde(default)]
    pub targets: BTreeMap<String, RegistryTarget>,
}

impl RegistryPack {
    pub fn display_name(&self) -> String {
        self.name
            .clone()
            .or_else(|| self.label.clone())
            .filter(|s| !s.trim().is_empty())
            .unwrap_or_else(|| self.id.clone())
    }

    fn validate(&self) -> Result<(), String> {
        validate_id(&self.id)?;
        if !matches!(self.kind.as_str(), "grammar" | "server" | "runtime") {
            return Err(format!(
                "index: pack {} has unknown kind {:?}",
                self.id, self.kind
            ));
        }
        if !is_safe_segment(&self.version) {
            return Err(format!(
                "index: pack {} has unsafe version {:?}",
                self.id, self.version
            ));
        }
        for (t, a) in &self.targets {
            if a.url.is_empty() || a.sha256.len() != 64 {
                return Err(format!(
                    "index: pack {} target {t} needs url + sha256",
                    self.id
                ));
            }
        }
        for dep in &self.requires {
            validate_id(dep)?;
        }
        Ok(())
    }
}

/// Every pack `id` needs, transitively, in install order (deepest first),
/// deduplicated and cycle-safe; `id` itself is not listed. A dependency the
/// index does not know is an error — the install would fail later anyway,
/// better before anything is downloaded.
pub fn resolve_requires(index: &RegistryIndex, id: &str) -> Result<Vec<String>, String> {
    fn walk(
        index: &RegistryIndex,
        id: &str,
        stack: &mut Vec<String>,
        out: &mut Vec<String>,
    ) -> Result<(), String> {
        let pack = index
            .packs
            .iter()
            .find(|p| p.id == id)
            .ok_or_else(|| format!("{id}: not in the registry"))?;
        for dep in &pack.requires {
            if stack.iter().any(|s| s == dep) || out.iter().any(|o| o == dep) {
                continue; // cycle or already planned
            }
            stack.push(dep.clone());
            walk(index, dep, stack, out)?;
            stack.pop();
            if !out.iter().any(|o| o == dep) {
                out.push(dep.clone());
            }
        }
        Ok(())
    }
    let mut out = Vec::new();
    let mut stack = vec![id.to_string()];
    walk(index, id, &mut stack, &mut out)?;
    out.retain(|d| d != id);
    Ok(out)
}

#[derive(Debug, Clone, PartialEq, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase", default)]
pub struct RegistryIndex {
    pub schema: u32,
    pub generated: Option<String>,
    pub packs: Vec<RegistryPack>,
}

impl RegistryIndex {
    /// Parse `index.json`. A pack that fails validation is dropped (logged),
    /// not fatal: one bad entry must not hide the whole catalog.
    pub fn parse(json: &str) -> Result<Self, String> {
        let mut idx: RegistryIndex =
            serde_json::from_str(json).map_err(|e| format!("index: invalid json: {e}"))?;
        if idx.schema != SCHEMA {
            return Err(format!(
                "index: unsupported schema {} (want {SCHEMA})",
                idx.schema
            ));
        }
        idx.packs.retain(|p| match p.validate() {
            Ok(()) => true,
            Err(e) => {
                log::warn!("extensions: dropping registry pack: {e}");
                false
            }
        });
        Ok(idx)
    }
}

#[cfg(test)]
pub(crate) mod fixtures {
    pub const GRAMMAR: &str = r#"{
      "schema": 1, "id": "grammar.java", "kind": "grammar", "version": "0.23.4-1",
      "label": "Java grammar", "language": "java", "monacoLanguage": "java",
      "fileExtensions": ["java"], "symbol": "tree_sitter_java", "abi": 15,
      "library": "tree_sitter_java.dll", "queries": { "tags": "queries/tags.scm", "locals": null },
      "upstream": { "repo": "tree-sitter/tree-sitter-java", "tag": "v0.23.4" },
      "target": "windows-x86_64", "minOrreryVersion": "0.24.0", "futureKey": [1] }"#;

    pub const SERVER: &str = r#"{
      "schema": 1, "id": "server.jdtls", "kind": "server", "version": "1.40.0-1",
      "label": "Eclipse JDT LS", "languages": ["java"], "transport": "stdio",
      "program": "jdtls", "args": ["-data", "${workspaceStorage}/jdtls"],
      "detect": { "path": ["jdtls", "jdtls.bat"], "env": ["JDTLS_HOME"],
                  "requires": [{ "program": "java", "minVersion": "17", "env": ["JAVA_HOME"] }] },
      "download": { "windows-x86_64": { "url": "https://x/jdtls.tar.gz", "sha256": "0000000000000000000000000000000000000000000000000000000000000000", "unpack": "tar.gz", "entry": "bin/jdtls.bat" } },
      "initializationOptions": { "extendedClientCapabilities": { "classFileContentsSupport": true } },
      "settings": {}, "virtualSchemes": ["jdt"],
      "virtualRead": { "method": "java/classFileContents" }, "minOrreryVersion": "0.24.0" }"#;

    /// A self-contained (contract v2) server pack: launched by `runtime.node`.
    pub const SERVER_BUNDLED: &str = r#"{
      "schema": 1, "id": "server.typescript-language-server", "kind": "server", "version": "6.0.0-1",
      "label": "TypeScript Language Server", "languages": ["typescript", "tsx", "javascript"], "transport": "stdio",
      "program": "typescript-language-server", "args": ["--stdio"],
      "detect": { "path": ["typescript-language-server", "typescript-language-server.cmd"], "env": [],
                  "requires": [{ "program": "node", "minVersion": "18", "env": [] }] },
      "requires": ["runtime.node"],
      "launch": { "runtime": "node", "entry": "server/node_modules/typescript-language-server/lib/cli.mjs", "args": ["--stdio"] },
      "initializationOptions": { "tsserver": { "path": "${extDir}/server/node_modules/typescript/lib/tsserver.js", "useSyntaxServer": "never" } },
      "settings": {}, "virtualSchemes": [], "virtualRead": null, "minOrreryVersion": "0.24.0" }"#;

    /// jdtls as a self-contained pack: `runtime.java` + the equinox launcher glob.
    pub const SERVER_JDTLS_BUNDLED: &str = r#"{
      "schema": 1, "id": "server.jdtls", "kind": "server", "version": "1.61.0-1",
      "label": "Eclipse JDT LS", "languages": ["java"], "transport": "stdio",
      "program": "jdtls", "args": ["-data", "${workspaceStorage}/jdtls"],
      "detect": { "path": ["jdtls", "jdtls.bat"], "env": ["JDTLS_HOME"],
                  "requires": [{ "program": "java", "minVersion": "21", "env": ["JAVA_HOME"] }] },
      "requires": ["runtime.java"],
      "launch": { "runtime": "java", "entry": "plugins/org.eclipse.equinox.launcher_*.jar",
                  "jvmArgs": ["-Declipse.application=org.eclipse.jdt.ls.core.id1", "-Xms1G", "--add-modules=ALL-SYSTEM"],
                  "args": ["-configuration", "${extDir}/${jdtlsConfig}", "-data", "${workspaceStorage}"] },
      "initializationOptions": { "extendedClientCapabilities": { "classFileContentsSupport": true } },
      "settings": {}, "virtualSchemes": ["jdt"], "virtualRead": { "method": "java/classFileContents" },
      "minOrreryVersion": "0.24.0" }"#;

    pub const RUNTIME_NODE: &str = r#"{
      "schema": 1, "id": "runtime.node", "kind": "runtime", "version": "22.12.0-1",
      "label": "Node.js 22", "target": "windows-x86_64", "runtime": "node", "binary": "node.exe",
      "minOrreryVersion": "0.24.0" }"#;

    pub const RUNTIME_JAVA: &str = r#"{
      "schema": 1, "id": "runtime.java", "kind": "runtime", "version": "21.0.5-1",
      "label": "Temurin JRE 21", "target": "windows-x86_64", "runtime": "java", "binary": "bin/java.exe",
      "minOrreryVersion": "0.24.0" }"#;

    pub const INDEX: &str = r#"{
      "schema": 1, "generated": "2026-09-11T00:00:00Z",
      "packs": [
        { "id": "grammar.java", "kind": "grammar", "version": "0.23.4-1", "description": "Java grammar",
          "languages": ["java"], "abi": 15, "minOrreryVersion": "0.24.0",
          "targets": { "windows-x86_64": { "url": "https://x/java-win.zip", "sha256": "1111111111111111111111111111111111111111111111111111111111111111", "size": 1234 },
                       "macos-aarch64": { "url": "https://x/java-mac.zip", "sha256": "2222222222222222222222222222222222222222222222222222222222222222", "size": 2345 } } },
        { "id": "server.jdtls", "kind": "server", "version": "1.40.0-1", "description": "Eclipse JDT LS",
          "languages": ["java"], "minOrreryVersion": "0.24.0",
          "targets": { "any": { "url": "https://x/jdtls.zip", "sha256": "3333333333333333333333333333333333333333333333333333333333333333", "size": 10 } } },
        { "id": "bad/id", "kind": "grammar", "version": "1", "targets": {} }
      ] }"#;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parses_grammar_manifest_and_ignores_unknown_fields() {
        let m = Manifest::parse(fixtures::GRAMMAR).unwrap();
        let Manifest::Grammar(g) = m else {
            panic!("grammar")
        };
        assert_eq!(g.id, "grammar.java");
        assert_eq!(g.symbol, "tree_sitter_java");
        assert_eq!(g.abi, 15);
        assert_eq!(g.queries.tags.as_deref(), Some("queries/tags.scm"));
        assert_eq!(g.queries.locals, None);
        assert_eq!(g.file_extensions, vec!["java"]);
        assert_eq!(g.min_orrery_version.as_deref(), Some("0.24.0"));
    }

    #[test]
    fn parses_server_manifest() {
        let m = Manifest::parse(fixtures::SERVER).unwrap();
        let Manifest::Server(s) = m else {
            panic!("server")
        };
        assert_eq!(s.program, "jdtls");
        assert_eq!(s.detect.path, vec!["jdtls", "jdtls.bat"]);
        assert_eq!(s.detect.requires[0].min_version.as_deref(), Some("17"));
        assert_eq!(
            s.download["windows-x86_64"].entry.as_deref(),
            Some("bin/jdtls.bat")
        );
        assert_eq!(s.virtual_read.unwrap().method, "java/classFileContents");
        assert_eq!(s.languages, vec!["java"]);
    }

    #[test]
    fn old_server_shape_still_parses_without_requires_or_launch() {
        let Manifest::Server(s) = Manifest::parse(fixtures::SERVER).unwrap() else {
            panic!("server")
        };
        assert!(s.requires.is_empty());
        assert_eq!(s.launch, None);
        assert!(Manifest::parse(fixtures::SERVER).unwrap().requires().is_empty());
    }

    #[test]
    fn parses_bundled_server_manifest_with_launch() {
        let Manifest::Server(s) = Manifest::parse(fixtures::SERVER_BUNDLED).unwrap() else {
            panic!("server")
        };
        assert_eq!(s.requires, vec!["runtime.node"]);
        let l = s.launch.as_ref().unwrap();
        assert_eq!(l.runtime.as_deref(), Some("node"));
        assert_eq!(l.entry, "server/node_modules/typescript-language-server/lib/cli.mjs");
        assert_eq!(l.args, vec!["--stdio"]);
        assert!(l.jvm_args.is_empty());
        let Manifest::Server(j) = Manifest::parse(fixtures::SERVER_JDTLS_BUNDLED).unwrap() else {
            panic!("server")
        };
        let l = j.launch.unwrap();
        assert_eq!(l.runtime.as_deref(), Some("java"));
        assert_eq!(l.entry, "plugins/org.eclipse.equinox.launcher_*.jar");
        assert_eq!(l.jvm_args.len(), 3);
        assert_eq!(l.args[1], "${extDir}/${jdtlsConfig}");
        // a launch-only pack (no program, no download) is valid
        let mut v: Value = serde_json::from_str(fixtures::SERVER_BUNDLED).unwrap();
        v["program"] = "".into();
        v["download"] = serde_json::json!({});
        assert!(Manifest::parse(&v.to_string()).is_ok());
    }

    #[test]
    fn parses_runtime_manifest() {
        let Manifest::Runtime(r) = Manifest::parse(fixtures::RUNTIME_NODE).unwrap() else {
            panic!("runtime")
        };
        assert_eq!(r.id, "runtime.node");
        assert_eq!(r.runtime, "node");
        assert_eq!(r.binary, "node.exe");
        assert_eq!(r.target, "windows-x86_64");
        let m = Manifest::parse(fixtures::RUNTIME_JAVA).unwrap();
        assert_eq!(m.kind(), "runtime");
        assert_eq!(m.id(), "runtime.java");
        assert_eq!(m.version(), "21.0.5-1");
        assert_eq!(m.label(), "Temurin JRE 21");
        assert!(m.languages().is_empty());
        assert_eq!(m.abi(), None);
        assert_eq!(m.min_orrery_version(), Some("0.24.0"));
        assert_eq!(m.target(), "windows-x86_64");
    }

    #[test]
    fn rejects_bad_launch_runtime_and_requires() {
        let base: Value = serde_json::from_str(fixtures::SERVER_BUNDLED).unwrap();
        let with = |f: &dyn Fn(&mut Value)| {
            let mut m = base.clone();
            f(&mut m);
            Manifest::parse(&m.to_string())
        };
        assert!(with(&|m| m["launch"]["entry"] = "../x.mjs".into()).is_err());
        assert!(with(&|m| m["launch"]["entry"] = "*".into()).is_err(), "bare star");
        assert!(with(&|m| m["launch"]["entry"] = "plugins/*.jar".into()).is_ok());
        assert!(with(&|m| m["launch"]["runtime"] = "python".into()).is_err());
        assert!(
            with(&|m| m["requires"] = serde_json::json!([])).is_err(),
            "node launch without runtime.node in requires"
        );
        assert!(with(&|m| m["requires"] = serde_json::json!(["../evil"])).is_err());
        assert!(with(&|m| m["requires"] = serde_json::json!(["server.typescript-language-server"])).is_err(), "self");
        assert!(with(&|m| {
            m["launch"]["runtime"] = Value::Null;
            m["requires"] = serde_json::json!([]);
        })
        .is_ok());
        // runtime manifests
        let rt: Value = serde_json::from_str(fixtures::RUNTIME_NODE).unwrap();
        let with_rt = |k: &str, v: Value| {
            let mut m = rt.clone();
            m[k] = v;
            Manifest::parse(&m.to_string())
        };
        assert!(with_rt("runtime", "deno".into()).is_err());
        assert!(with_rt("binary", "../node.exe".into()).is_err());
        assert!(with_rt("id", "server.node".into()).is_err(), "kind/id prefix mismatch");
        assert!(with_rt("binary", "Contents/Home/bin/java".into()).is_ok());
    }

    #[test]
    fn resolves_requires_transitively_deduped_and_cycle_safe() {
        let idx = |packs: &[(&str, &[&str])]| RegistryIndex {
            schema: 1,
            generated: None,
            packs: packs
                .iter()
                .map(|(id, req)| RegistryPack {
                    id: id.to_string(),
                    kind: if id.starts_with("runtime.") { "runtime" } else { "server" }.into(),
                    version: "1".into(),
                    name: None,
                    label: None,
                    description: String::new(),
                    languages: vec![],
                    abi: None,
                    min_orrery_version: None,
                    requires: req.iter().map(|s| s.to_string()).collect(),
                    targets: BTreeMap::new(),
                })
                .collect(),
        };
        let i = idx(&[
            ("server.ts", &["runtime.node", "server.base"]),
            ("server.base", &["runtime.node"]),
            ("runtime.node", &[]),
            ("server.a", &["server.b"]),
            ("server.b", &["server.a"]),
            ("server.lonely", &[]),
            ("server.broken", &["runtime.missing"]),
        ]);
        assert_eq!(resolve_requires(&i, "server.ts").unwrap(), vec!["runtime.node", "server.base"]);
        assert_eq!(resolve_requires(&i, "server.base").unwrap(), vec!["runtime.node"]);
        assert!(resolve_requires(&i, "server.lonely").unwrap().is_empty());
        assert_eq!(resolve_requires(&i, "server.a").unwrap(), vec!["server.b"], "cycle: b, not a again");
        assert!(resolve_requires(&i, "server.broken").unwrap_err().contains("runtime.missing"));
        assert!(resolve_requires(&i, "server.nope").is_err());
    }

    #[test]
    fn index_entries_carry_requires() {
        let mut v: Value = serde_json::from_str(fixtures::INDEX).unwrap();
        v["packs"][1]["requires"] = serde_json::json!(["runtime.java"]);
        v["packs"].as_array_mut().unwrap().push(serde_json::json!({
            "id": "runtime.java", "kind": "runtime", "version": "21.0.5-1",
            "targets": { "windows-x86_64": { "url": "https://x/jre.zip", "sha256": "4".repeat(64), "size": 50000000 } }
        }));
        v["packs"].as_array_mut().unwrap().push(serde_json::json!({
            "id": "server.bad", "kind": "server", "version": "1", "requires": ["../x"], "targets": {}
        }));
        let idx = RegistryIndex::parse(&v.to_string()).unwrap();
        assert_eq!(idx.packs.len(), 3, "bad requires drops the entry");
        assert_eq!(idx.packs[1].requires, vec!["runtime.java"]);
        assert_eq!(idx.packs[2].kind, "runtime");
        assert!(idx.packs[0].requires.is_empty(), "grammar: default empty");
    }

    #[test]
    fn rejects_bad_kind_schema_and_unsafe_paths() {
        let base: Value = serde_json::from_str(fixtures::GRAMMAR).unwrap();
        let with = |k: &str, v: Value| {
            let mut m = base.clone();
            m[k] = v;
            Manifest::parse(&m.to_string())
        };
        assert!(with("kind", "plugin".into()).is_err());
        assert!(with("schema", 2.into()).is_err());
        assert!(with("id", "../evil".into()).is_err());
        assert!(
            with("id", "server.java".into()).is_err(),
            "kind/id prefix mismatch"
        );
        assert!(with("version", "1/2".into()).is_err());
        assert!(with("library", "../x.dll".into()).is_err());
        assert!(with("library", "sub/x.dll".into()).is_err());
        assert!(with("symbol", "tree sitter".into()).is_err());
        assert!(with("queries", serde_json::json!({ "tags": "../tags.scm" })).is_err());
        assert!(with("fileExtensions", serde_json::json!([".java"])).is_err());
        assert!(Manifest::parse("not json").is_err());
        assert!(Manifest::parse(r#"{"schema":1}"#).is_err(), "kind missing");
    }

    #[test]
    fn server_needs_program_or_download_and_safe_entry() {
        let base: Value = serde_json::from_str(fixtures::SERVER).unwrap();
        let mut m = base.clone();
        m["program"] = "".into();
        m["download"] = serde_json::json!({});
        assert!(Manifest::parse(&m.to_string()).is_err());
        let mut m = base.clone();
        m["download"]["windows-x86_64"]["entry"] = "../../x".into();
        assert!(Manifest::parse(&m.to_string()).is_err());
        let mut m = base;
        m["download"]["windows-x86_64"]["sha256"] = "abc".into();
        assert!(Manifest::parse(&m.to_string()).is_err());
    }

    #[test]
    fn parses_index_and_drops_invalid_packs() {
        let idx = RegistryIndex::parse(fixtures::INDEX).unwrap();
        assert_eq!(idx.packs.len(), 2, "the bad/id pack is dropped, not fatal");
        assert_eq!(idx.packs[0].id, "grammar.java");
        assert_eq!(idx.packs[0].targets["windows-x86_64"].size, 1234);
        assert_eq!(idx.packs[1].targets["any"].url, "https://x/jdtls.zip");
        assert_eq!(idx.packs[0].display_name(), "grammar.java", "no name → id");
        assert!(RegistryIndex::parse(r#"{"schema":2,"packs":[]}"#).is_err());
        assert!(RegistryIndex::parse("[]").is_err());
    }

    #[test]
    fn safe_segment_rules() {
        assert!(is_safe_segment("0.23.4-1"));
        assert!(is_safe_segment("tree_sitter_java.dll"));
        assert!(!is_safe_segment(".."));
        assert!(!is_safe_segment("a/b"));
        assert!(!is_safe_segment("a\\b"));
        assert!(!is_safe_segment(""));
        assert!(is_safe_relative("queries/tags.scm"));
        assert!(!is_safe_relative("/queries/tags.scm"));
        assert!(!is_safe_relative("queries/../tags.scm"));
        assert!(is_safe_relative_glob("plugins/org.eclipse.equinox.launcher_*.jar"));
        assert!(is_safe_relative_glob("rust-analyzer.exe"));
        assert!(!is_safe_relative_glob("plugins/*"));
        assert!(!is_safe_relative_glob("../*.jar"));
        assert!(!is_safe_relative_glob("a\\*.jar"));
    }
}
