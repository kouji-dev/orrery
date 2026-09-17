//! Declaration extraction + the per-source index job (M4).
//!
//! Java (`jdk`/`maven` zips): every `*.java` entry → its package from the
//! entry directory (the JDK's leading module segment stripped) → types +
//! members via the java grammar pack when one is loaded, else a regex
//! fallback that tracks brace depth so members are only taken at a type's
//! body depth (statements inside method bodies never match).
//!
//! Rust (`cargo` dirs): `src/**/*.rs` → `pub` items + `macro_rules!`, fq
//! `crate_name::module::Name` from the file path, `impl`/`trait`/inline-`mod`
//! scopes tracked the same way.
//!
//! Rows go to `symbols.db` 2k per transaction under one artifact id;
//! progress is reported every `PROGRESS_EVERY` files; the cancel flag is
//! polled per file. A job past [`Limits::max_decls`] ends as
//! [`JobEnd::Skipped`] (the `windows` bindings crates would be a third of
//! the index on their own); the per-artifact FILE guard is applied by the
//! service before the job starts ([`rust_files`] / [`java_entry_count`]).

use std::io::{Read, Seek};
use std::path::Path;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;

use regex::Regex;

use crate::extensions::loader::Loaded;
use crate::symbols::store::{LibDecl, SymbolStore, LIB_BATCH};
use crate::symbols::tags::{self, FileSymbols};

/// Files between two progress reports.
pub const PROGRESS_EVERY: usize = 500;
/// A single entry beyond this is skipped (matches the virtual-read cap).
pub const MAX_ENTRY_BYTES: u64 = 2 * 1024 * 1024;

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Progress {
    pub done: usize,
    pub total: usize,
    pub decls: usize,
}

#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Stats {
    pub files: usize,
    pub decls: usize,
    pub bytes: u64,
}

#[derive(Debug, PartialEq, Eq)]
pub enum JobEnd {
    Done(Stats),
    Cancelled(Stats),
    /// Over a perf guard — rows written so far are the caller's to drop.
    Skipped(String),
}

/// Perf guards of one job (`0` = unlimited).
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Limits {
    pub max_decls: usize,
}

// ---- java: package from the entry name -------------------------------------------

/// `looks like a JPMS module name` (`java.base`, `jdk.compiler`).
fn is_module_segment(seg: &str) -> bool {
    let mut parts = seg.split('.');
    let first = parts.next().unwrap_or("");
    let mut n = 1;
    if first.is_empty() || !first.chars().next().is_some_and(|c| c.is_ascii_lowercase()) {
        return false;
    }
    for p in parts {
        n += 1;
        if p.is_empty() || !p.chars().all(|c| c.is_ascii_alphanumeric() || c == '_') {
            return false;
        }
    }
    n >= 2 && seg.chars().all(|c| c.is_ascii_alphanumeric() || c == '.' || c == '_')
}

/// `(module, package)` of a zip entry: `java.base/java/util/ArrayList.java`
/// → `(Some("java.base"), "java.util")` for the JDK layout;
/// `com/google/gson/Gson.java` → `(None, "com.google.gson")`.
pub fn java_package_of(entry: &str, jdk_layout: bool) -> (Option<String>, String) {
    let dir = match entry.rsplit_once('/') {
        Some((d, _)) => d,
        None => return (None, String::new()),
    };
    let mut segs: Vec<&str> = dir.split('/').filter(|s| !s.is_empty()).collect();
    let mut module = None;
    if jdk_layout && segs.first().is_some_and(|s| is_module_segment(s)) {
        module = Some(segs.remove(0).to_string());
    }
    (module, segs.join("."))
}

fn type_kind(k: &str) -> Option<&'static str> {
    Some(match k {
        "class" => "class",
        "interface" => "interface",
        "enum" => "enum",
        "record" => "record",
        "@interface" | "annotation" => "annotation",
        _ => return None,
    })
}

fn member_kind(k: &str) -> Option<&'static str> {
    Some(match k {
        "method" | "constructor" | "function" => "method",
        "field" | "constant" | "property" | "variable" => "field",
        _ => return None,
    })
}

// ---- java: tree-sitter path -----------------------------------------------------------

/// Declarations from a grammar-pack extraction: types nest with `$`,
/// members hang off their enclosing type with `.`.
pub fn java_from_symbols(fs: &FileSymbols, entry: &str, package: &str) -> Vec<LibDecl> {
    let n = fs.defs.len();
    // fq of every TYPE def (None for members / unknown kinds), resolved by
    // walking the container chain; memoised per index.
    let mut fq: Vec<Option<String>> = vec![None; n];
    fn type_fq(fs: &FileSymbols, fq: &mut Vec<Option<String>>, i: usize, package: &str, depth: usize) -> Option<String> {
        if let Some(v) = &fq[i] {
            return Some(v.clone());
        }
        let d = &fs.defs[i];
        type_kind(&d.kind)?;
        if depth > 64 {
            return None;
        }
        let parent = d
            .container
            .and_then(|c| (c as usize != i && (c as usize) < fs.defs.len()).then_some(c as usize));
        let v = match parent.and_then(|p| type_fq(fs, fq, p, package, depth + 1)) {
            Some(p) => format!("{p}${}", d.name),
            None if package.is_empty() => d.name.clone(),
            None => format!("{package}.{}", d.name),
        };
        fq[i] = Some(v.clone());
        Some(v)
    }
    let mut out = Vec::new();
    for i in 0..n {
        let d = &fs.defs[i];
        if let Some(kind) = type_kind(&d.kind) {
            let Some(name) = type_fq(fs, &mut fq, i, package, 0) else {
                continue;
            };
            let container = d
                .container
                .and_then(|c| fq.get(c as usize).cloned().flatten());
            out.push(LibDecl {
                fq_name: name,
                simple_name: d.name.clone(),
                kind: kind.into(),
                container,
                entry: entry.into(),
                line: d.sel_line,
                lang: "java".into(),
            });
        } else if let Some(kind) = member_kind(&d.kind) {
            let Some(owner) = d
                .container
                .and_then(|c| type_fq(fs, &mut fq, c as usize, package, 0))
            else {
                continue;
            };
            out.push(LibDecl {
                fq_name: format!("{owner}.{}", d.name),
                simple_name: d.name.clone(),
                kind: kind.into(),
                container: Some(owner),
                entry: entry.into(),
                line: d.sel_line,
                lang: "java".into(),
            });
        }
    }
    out
}

// ---- java: regex fallback ----------------------------------------------------------

struct JavaRes {
    ty: Regex,
    field: Regex,
    method: Regex,
}

fn java_res() -> &'static JavaRes {
    static R: OnceLock<JavaRes> = OnceLock::new();
    R.get_or_init(|| JavaRes {
        ty: Regex::new(
            r"^\s*(?:(?:public|protected|private|static|final|abstract|strictfp|sealed|non-sealed)\s+)*(class|interface|enum|record|@interface)\s+(\w+)",
        )
        .unwrap(),
        field: Regex::new(
            r"^\s*(?:(?:public|protected|private|static|final|transient|volatile)\s+)*([\w.$]+(?:<.*?>)?(?:\[\])*)\s+(\w+)\s*(?:=|;|,|$)",
        )
        .unwrap(),
        method: Regex::new(
            r"^\s*(?:(?:public|protected|private|static|final|abstract|synchronized|native|default|strictfp)\s+)*(?:<.*?>\s+)?([\w.$]+(?:<.*?>)?(?:\[\])*)\s+(\w+)\s*\(",
        )
        .unwrap(),
    })
}

const JAVA_KEYWORDS: &[&str] = &[
    "return", "new", "throw", "else", "case", "if", "while", "for", "switch", "do", "try",
    "catch", "finally", "synchronized", "assert", "break", "continue", "package", "import",
    "extends", "implements", "throws", "this", "super", "yield", "instanceof",
];

/// Brace-aware line scanner shared by both regex fallbacks: yields
/// `(code_line, depth_before, depth_after)` with strings/comments blanked
/// out so braces inside them do not count.
struct Scanner<'a> {
    lines: std::str::Lines<'a>,
    in_block: bool,
    depth: i32,
    line_no: u32,
}

impl<'a> Scanner<'a> {
    fn new(text: &'a str) -> Self {
        Self {
            lines: text.lines(),
            in_block: false,
            depth: 0,
            line_no: 0,
        }
    }

    /// Next line as `(line_no, code, depth_before, depth_after)`.
    fn next_line(&mut self) -> Option<(u32, String, i32, i32)> {
        let raw = self.lines.next()?;
        let line_no = self.line_no;
        self.line_no += 1;
        let mut code = String::with_capacity(raw.len());
        let bytes = raw.as_bytes();
        let mut i = 0;
        let mut in_str: Option<u8> = None;
        while i < bytes.len() {
            let b = bytes[i];
            if self.in_block {
                if b == b'*' && bytes.get(i + 1) == Some(&b'/') {
                    self.in_block = false;
                    i += 2;
                } else {
                    i += 1;
                }
                continue;
            }
            if let Some(q) = in_str {
                if b == b'\\' {
                    i += 2;
                    continue;
                }
                if b == q {
                    in_str = None;
                }
                i += 1;
                continue;
            }
            match b {
                b'/' if bytes.get(i + 1) == Some(&b'/') => break,
                b'/' if bytes.get(i + 1) == Some(&b'*') => {
                    self.in_block = true;
                    i += 2;
                    code.push(' ');
                }
                b'"' => {
                    // rust raw / java text blocks are approximated as one-line strings
                    in_str = Some(b'"');
                    i += 1;
                    code.push('"');
                }
                b'\'' => {
                    // `'x'` / `'{'` / `'\''` char literal; a rust lifetime has no
                    // closing quote within reach and is skipped as one byte
                    let start = if bytes.get(i + 1) == Some(&b'\\') { i + 3 } else { i + 2 };
                    match bytes
                        .get(start.min(bytes.len())..)
                        .and_then(|s| s.iter().take(3).position(|&c| c == b'\''))
                    {
                        Some(p) => i = start + p + 1,
                        None => i += 1,
                    }
                }
                _ => {
                    code.push(b as char);
                    i += 1;
                }
            }
        }
        let before = self.depth;
        let opens = code.bytes().filter(|&c| c == b'{').count() as i32;
        let closes = code.bytes().filter(|&c| c == b'}').count() as i32;
        self.depth = (self.depth + opens - closes).max(0);
        Some((line_no, code, before, self.depth))
    }
}

struct Scope {
    fq: String,
    simple: String,
    /// Depth inside the body (members live exactly here).
    body: i32,
}

fn strip_generics(t: &str) -> &str {
    t.split('<').next().unwrap_or(t).trim()
}

/// Regex fallback: types (with `$` nesting) + members at body depth.
pub fn java_from_regex(text: &str, entry: &str, package: &str) -> Vec<LibDecl> {
    let r = java_res();
    let mut out = Vec::new();
    let mut scopes: Vec<Scope> = Vec::new();
    let mut pending: Option<(String, String)> = None; // (fq, simple) waiting for its `{`
    let mut sc = Scanner::new(text);
    while let Some((line_no, code, before, after)) = sc.next_line() {
        let trimmed = code.trim_start();
        if trimmed.is_empty() || (trimmed.starts_with('@') && !trimmed.starts_with("@interface")) || trimmed.starts_with('*') {
            // still let braces on this line close scopes below
        } else if let Some(c) = r.ty.captures(&code) {
            let kind = type_kind(&c[1]).unwrap_or("class");
            let simple = c[2].to_string();
            let fq = match scopes.last() {
                Some(s) => format!("{}${simple}", s.fq),
                None if package.is_empty() => simple.clone(),
                None => format!("{package}.{simple}"),
            };
            out.push(LibDecl {
                fq_name: fq.clone(),
                simple_name: simple.clone(),
                kind: kind.into(),
                container: scopes.last().map(|s| s.fq.clone()),
                entry: entry.into(),
                line: line_no,
                lang: "java".into(),
            });
            pending = Some((fq, simple));
        } else if let Some(s) = scopes.last() {
            if before == s.body && pending.is_none() {
                if let Some(c) = r.field.captures(&code) {
                    let ty = strip_generics(&c[1]);
                    if !JAVA_KEYWORDS.contains(&ty) {
                        out.push(LibDecl {
                            fq_name: format!("{}.{}", s.fq, &c[2]),
                            simple_name: c[2].to_string(),
                            kind: "field".into(),
                            container: Some(s.fq.clone()),
                            entry: entry.into(),
                            line: line_no,
                            lang: "java".into(),
                        });
                    }
                } else if let Some(c) = r.method.captures(&code) {
                    // a constructor (`public Name(`) lands here with the access
                    // modifier captured as its "type" — wanted, as a method
                    let ty = strip_generics(&c[1]);
                    let name = &c[2];
                    let ctor = name == s.simple && matches!(ty, "public" | "protected" | "private");
                    if ctor || (!JAVA_KEYWORDS.contains(&ty) && !JAVA_KEYWORDS.contains(&name)) {
                        out.push(LibDecl {
                            fq_name: format!("{}.{}", s.fq, name),
                            simple_name: name.to_string(),
                            kind: "method".into(),
                            container: Some(s.fq.clone()),
                            entry: entry.into(),
                            line: line_no,
                            lang: "java".into(),
                        });
                    }
                }
            }
        }
        // scope bookkeeping from this line's braces
        if after > before {
            if let Some((fq, simple)) = pending.take() {
                scopes.push(Scope {
                    fq,
                    simple,
                    body: before + 1,
                });
            }
        } else if code.contains('{') {
            pending = None; // `record P(int x) {}` — opened and closed here
        }
        while scopes.last().is_some_and(|s| after < s.body) {
            scopes.pop();
        }
    }
    out
}

/// Types + members of one Java file: the grammar when given, else regex.
pub fn extract_java(bytes: &[u8], entry: &str, package: &str, grammar: Option<&Loaded>) -> Vec<LibDecl> {
    if let Some(g) = grammar {
        if let Ok(fs) = tags::extract(bytes, g) {
            let decls = java_from_symbols(&fs, entry, package);
            if !decls.is_empty() {
                return decls;
            }
        }
    }
    java_from_regex(&String::from_utf8_lossy(bytes), entry, package)
}

// ---- rust ---------------------------------------------------------------------------

/// `serde-json` → `serde_json`.
pub fn normalise_crate(name: &str) -> String {
    name.replace('-', "_")
}

/// Module path of a crate file: `src/lib.rs` → `[]`, `src/de/mod.rs` →
/// `["de"]`, `src/de/value.rs` → `["de", "value"]`. `None` for files outside
/// `src/` or under `src/bin/`.
pub fn rust_module_path(rel: &str) -> Option<Vec<String>> {
    let rel = rel.replace('\\', "/");
    let rest = rel.strip_prefix("src/")?;
    let rest = rest.strip_suffix(".rs")?;
    let mut segs: Vec<&str> = rest.split('/').collect();
    if segs.first() == Some(&"bin") {
        return None;
    }
    match segs.last().copied() {
        Some("lib") | Some("main") if segs.len() == 1 => {
            segs.pop();
        }
        Some("mod") => {
            segs.pop();
        }
        _ => {}
    }
    Some(segs.into_iter().map(String::from).collect())
}

struct RustRes {
    item: Regex,
    mac: Regex,
    imp: Regex,
    inline_mod: Regex,
}

fn rust_res() -> &'static RustRes {
    static R: OnceLock<RustRes> = OnceLock::new();
    R.get_or_init(|| RustRes {
        item: Regex::new(
            r"^\s*(pub(?:\([^)]*\))?\s+)?(?:(?:unsafe|async|const|default|extern\s+(?:\x22[^\x22]*\x22\s+)?)\s+)*(fn|struct|enum|trait|type|mod|const|static|union)\s+(\w+)",
        )
        .unwrap(),
        mac: Regex::new(r"^\s*macro_rules!\s+(\w+)").unwrap(),
        imp: Regex::new(r"^\s*(?:unsafe\s+)?impl\s*(?:<.*?>)?\s*(?:[\w:]+(?:<.*?>)?\s+for\s+)?([\w:]+)").unwrap(),
        inline_mod: Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?mod\s+(\w+)\s*\{").unwrap(),
    })
}

enum RustScopeKind {
    Mod,
    /// `impl X` / `trait X`: inner items are members of `fq`; `all_fns`
    /// (trait defs + trait impls) records non-`pub` fns too.
    Owner { all_fns: bool },
    /// A `fn` body or anything else: swallows everything inside.
    Opaque,
}

struct RustScope {
    kind: RustScopeKind,
    /// Module path (for `Mod`) or owner fq (for `Owner`).
    fq: String,
    body: i32,
}

/// `pub` items + macros of one Rust file. `module` is `crate_name::a::b`.
pub fn rust_from_regex(text: &str, entry: &str, module: &str) -> Vec<LibDecl> {
    let r = rust_res();
    let mut out = Vec::new();
    let mut scopes: Vec<RustScope> = Vec::new();
    let mut pending: Option<RustScope> = None;
    let mut sc = Scanner::new(text);
    let cur_mod = |scopes: &[RustScope], module: &str| -> String {
        scopes
            .iter()
            .rev()
            .find(|s| matches!(s.kind, RustScopeKind::Mod))
            .map(|s| s.fq.clone())
            .unwrap_or_else(|| module.to_string())
    };
    while let Some((line_no, code, before, after)) = sc.next_line() {
        let opaque = scopes.last().is_some_and(|s| matches!(s.kind, RustScopeKind::Opaque));
        let at_body = scopes.last().map(|s| s.body == before).unwrap_or(before == 0);
        if !opaque && at_body && pending.is_none() {
            let (owner, all_fns) = match scopes.last() {
                Some(RustScope {
                    kind: RustScopeKind::Owner { all_fns },
                    fq,
                    ..
                }) => (Some(fq.clone()), *all_fns),
                _ => (None, false),
            };
            if let Some(c) = r.inline_mod.captures(&code) {
                let m = format!("{}::{}", cur_mod(&scopes, module), &c[1]);
                if code.trim_start().starts_with("pub") {
                    out.push(LibDecl {
                        fq_name: m.clone(),
                        simple_name: c[1].to_string(),
                        kind: "mod".into(),
                        container: None,
                        entry: entry.into(),
                        line: line_no,
                        lang: "rust".into(),
                    });
                }
                pending = Some(RustScope {
                    kind: RustScopeKind::Mod,
                    fq: m,
                    body: before + 1,
                });
            } else if let Some(c) = r.mac.captures(&code) {
                out.push(LibDecl {
                    fq_name: format!("{}::{}", cur_mod(&scopes, module), &c[1]),
                    simple_name: c[1].to_string(),
                    kind: "macro".into(),
                    container: None,
                    entry: entry.into(),
                    line: line_no,
                    lang: "rust".into(),
                });
                pending = Some(RustScope {
                    kind: RustScopeKind::Opaque,
                    fq: String::new(),
                    body: before + 1,
                });
            } else if let Some(c) = r.item.captures(&code) {
                let is_pub = c.get(1).is_some();
                let kind = c[2].to_string();
                let name = c[3].to_string();
                let visible = is_pub || (all_fns && kind == "fn");
                let fq = match &owner {
                    Some(o) => format!("{o}::{name}"),
                    None => format!("{}::{name}", cur_mod(&scopes, module)),
                };
                if visible {
                    out.push(LibDecl {
                        fq_name: fq.clone(),
                        simple_name: name.clone(),
                        kind: kind.clone(),
                        container: owner.clone(),
                        entry: entry.into(),
                        line: line_no,
                        lang: "rust".into(),
                    });
                }
                if kind == "trait" {
                    pending = Some(RustScope {
                        kind: RustScopeKind::Owner { all_fns: true },
                        fq,
                        body: before + 1,
                    });
                } else if kind == "mod" && code.contains('{') {
                    pending = Some(RustScope {
                        kind: RustScopeKind::Mod,
                        fq,
                        body: before + 1,
                    });
                } else {
                    pending = Some(RustScope {
                        kind: RustScopeKind::Opaque,
                        fq: String::new(),
                        body: before + 1,
                    });
                }
            } else if let Some(c) = r.imp.captures(&code) {
                let target = c[1].rsplit("::").next().unwrap_or(&c[1]).to_string();
                // inherent impls: `pub` items only; trait impls (`impl X for Y`):
                // nothing — `fn fmt`/`fn clone`/… are navigated via the trait
                // and would be the bulk of a registry's rows otherwise
                pending = Some(RustScope {
                    kind: RustScopeKind::Owner { all_fns: false },
                    fq: format!("{}::{target}", cur_mod(&scopes, module)),
                    body: before + 1,
                });
            } else if code.contains('{') {
                pending = Some(RustScope {
                    kind: RustScopeKind::Opaque,
                    fq: String::new(),
                    body: before + 1,
                });
            }
        }
        if after > before {
            if let Some(p) = pending.take() {
                scopes.push(p);
            }
        } else if code.contains('{') || code.trim_end().ends_with(';') {
            // `pub struct S { x: i32 }` opened and closed here, or
            // `pub fn f();` / `pub struct S;` / `pub mod m;` — no body follows
            pending = None;
        }
        while scopes.last().is_some_and(|s| after < s.body) {
            scopes.pop();
        }
    }
    out
}

// ---- job driver ------------------------------------------------------------------------

/// Sink of batched rows: the store in production, a Vec in tests.
pub trait DeclSink {
    fn write(&mut self, decls: &[LibDecl]) -> Result<(), String>;
}

/// `(store, artifact row id)`.
impl DeclSink for (&SymbolStore, i64) {
    fn write(&mut self, decls: &[LibDecl]) -> Result<(), String> {
        self.0
            .lib_insert_decls(self.1, decls)
            .map_err(|e| format!("lib_decls insert: {e}"))
    }
}

impl DeclSink for Vec<LibDecl> {
    fn write(&mut self, decls: &[LibDecl]) -> Result<(), String> {
        self.extend_from_slice(decls);
        Ok(())
    }
}

struct Batcher<'a> {
    sink: &'a mut dyn DeclSink,
    buf: Vec<LibDecl>,
    stats: Stats,
    limits: Limits,
}

impl<'a> Batcher<'a> {
    fn new(sink: &'a mut dyn DeclSink, limits: Limits) -> Self {
        Self {
            sink,
            buf: Vec::with_capacity(LIB_BATCH),
            stats: Stats::default(),
            limits,
        }
    }

    /// `Ok(true)` = keep going; `Ok(false)` = the declaration guard tripped.
    fn push(&mut self, decls: Vec<LibDecl>) -> Result<bool, String> {
        self.stats.decls += decls.len();
        self.buf.extend(decls);
        if self.buf.len() >= LIB_BATCH {
            self.flush()?;
        }
        Ok(self.limits.max_decls == 0 || self.stats.decls <= self.limits.max_decls)
    }

    fn skipped(&self) -> JobEnd {
        JobEnd::Skipped(format!(
            "{} declarations after {} files — over the {} limit",
            self.stats.decls, self.stats.files, self.limits.max_decls
        ))
    }
    fn flush(&mut self) -> Result<(), String> {
        if !self.buf.is_empty() {
            self.sink.write(&self.buf)?;
            self.buf.clear();
        }
        Ok(())
    }
}

fn skip_java_entry(name: &str) -> bool {
    !name.ends_with(".java")
        || name.ends_with("module-info.java")
        || name.ends_with("package-info.java")
        || name.starts_with("META-INF/")
}

/// The indexable `*.java` entries of an archive, by index.
fn java_entries<R: Read + Seek>(archive: &zip::ZipArchive<R>) -> Vec<(usize, String)> {
    (0..archive.len())
        .filter_map(|i| archive.name_for_index(i).map(|n| (i, n.to_string())))
        .filter(|(_, n)| !skip_java_entry(n))
        .collect()
}

/// How many `*.java` entries a job over `archive` would read (the file
/// guard + the progress total, before the job starts).
pub fn java_entry_count<R: Read + Seek>(archive: &zip::ZipArchive<R>) -> usize {
    java_entries(archive).len()
}

/// Index every `*.java` entry of `archive`. `jdk_layout` strips the module
/// segment from packages. Progress is reported every [`PROGRESS_EVERY`]
/// files and at the end.
pub fn index_java_zip<R: Read + Seek>(
    archive: &mut zip::ZipArchive<R>,
    jdk_layout: bool,
    grammar: Option<&Loaded>,
    cancel: &AtomicBool,
    sink: &mut dyn DeclSink,
    on_progress: &mut dyn FnMut(Progress),
    limits: Limits,
) -> Result<JobEnd, String> {
    let names = java_entries(archive);
    let total = names.len();
    let mut b = Batcher::new(sink, limits);
    let mut buf: Vec<u8> = Vec::new();
    for (done, (i, name)) in names.into_iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            b.flush()?;
            return Ok(JobEnd::Cancelled(b.stats));
        }
        let mut f = archive.by_index(i).map_err(|e| format!("{name}: {e}"))?;
        if !f.is_file() || f.size() > MAX_ENTRY_BYTES {
            continue;
        }
        buf.clear();
        if f.read_to_end(&mut buf).is_err() {
            continue;
        }
        b.stats.files += 1;
        b.stats.bytes += buf.len() as u64;
        let (_, package) = java_package_of(&name, jdk_layout);
        let decls = extract_java(&buf, &name, &package, grammar);
        if !b.push(decls)? {
            b.flush()?;
            return Ok(b.skipped());
        }
        if (done + 1) % PROGRESS_EVERY == 0 {
            on_progress(Progress {
                done: done + 1,
                total,
                decls: b.stats.decls,
            });
        }
    }
    b.flush()?;
    on_progress(Progress {
        done: total,
        total,
        decls: b.stats.decls,
    });
    Ok(JobEnd::Done(b.stats))
}

fn walk_rs(dir: &Path, out: &mut Vec<std::path::PathBuf>, depth: usize) {
    if depth > 32 {
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.flatten().collect();
    entries.sort_by_key(|e| e.file_name());
    for e in entries {
        let p = e.path();
        let Ok(ft) = e.file_type() else { continue };
        if ft.is_dir() {
            walk_rs(&p, out, depth + 1);
        } else if ft.is_file() && p.extension().is_some_and(|x| x == "rs") {
            out.push(p);
        }
    }
}

/// The `*.rs` files a job over `<crate dir>` would read — `src/**` only:
/// `tests/`, `benches/`, `examples/` never count (the file guard + the
/// progress total, before the job starts).
pub fn rust_files(dir: &Path) -> Vec<std::path::PathBuf> {
    let mut files = Vec::new();
    walk_rs(&dir.join("src"), &mut files, 0);
    files
}

/// Index `<crate dir>/src/**/*.rs`. `crate_name` is the registry dir's crate
/// part (hyphens allowed; normalised here).
pub fn index_cargo_dir(
    dir: &Path,
    crate_name: &str,
    cancel: &AtomicBool,
    sink: &mut dyn DeclSink,
    on_progress: &mut dyn FnMut(Progress),
    limits: Limits,
) -> Result<JobEnd, String> {
    let krate = normalise_crate(crate_name);
    let files = rust_files(dir);
    let total = files.len();
    let mut b = Batcher::new(sink, limits);
    for (done, abs) in files.iter().enumerate() {
        if cancel.load(Ordering::Relaxed) {
            b.flush()?;
            return Ok(JobEnd::Cancelled(b.stats));
        }
        let rel = abs
            .strip_prefix(dir)
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .unwrap_or_default();
        let Some(segs) = rust_module_path(&rel) else {
            continue;
        };
        let Ok(md) = std::fs::metadata(abs) else { continue };
        if md.len() > MAX_ENTRY_BYTES {
            continue;
        }
        let Ok(bytes) = std::fs::read(abs) else { continue };
        b.stats.files += 1;
        b.stats.bytes += bytes.len() as u64;
        let module = std::iter::once(krate.clone())
            .chain(segs)
            .collect::<Vec<_>>()
            .join("::");
        let decls = rust_from_regex(&String::from_utf8_lossy(&bytes), &rel, &module);
        if !b.push(decls)? {
            b.flush()?;
            return Ok(b.skipped());
        }
        if (done + 1) % PROGRESS_EVERY == 0 {
            on_progress(Progress {
                done: done + 1,
                total,
                decls: b.stats.decls,
            });
        }
    }
    b.flush()?;
    on_progress(Progress {
        done: total,
        total,
        decls: b.stats.decls,
    });
    Ok(JobEnd::Done(b.stats))
}

#[cfg(test)]
pub(crate) mod fixture {
    //! An in-memory JDK-layout zip: 3 java files, `Map` with a nested `Entry`.
    use std::io::{Cursor, Write};

    pub const ARRAY_LIST: &str = r#"/*
 * Copyright (c) 1997, 2023, Oracle and/or its affiliates. { not a brace }
 */
package java.util;

import java.util.function.Consumer;

/**
 * Resizable-array implementation of the {@code List} interface.
 */
public class ArrayList<E> extends AbstractList<E>
        implements List<E>, RandomAccess, Cloneable, java.io.Serializable {
    private static final int DEFAULT_CAPACITY = 10;
    transient Object[] elementData; // "{" in a comment
    private int size;

    public ArrayList(int initialCapacity) {
        if (initialCapacity > 0) {
            this.elementData = new Object[initialCapacity];
        }
    }

    @Override
    public int size() {
        return size;
    }

    public E get(int index) {
        Objects.checkIndex(index, size);
        return elementData(index);
    }

    E elementData(int index) {
        return (E) elementData[index];
    }

    private class Itr implements Iterator<E> {
        int cursor;
        public boolean hasNext() {
            return cursor != size;
        }
    }
}
"#;

    pub const STRING: &str = r#"package java.lang;

public final class String
    implements java.io.Serializable, Comparable<String>, CharSequence {
    private final byte[] value;
    public static final Comparator<String> CASE_INSENSITIVE_ORDER
                                         = new CaseInsensitiveComparator();
    public int length() {
        return value.length >> coder();
    }
    public static String valueOf(Object obj) {
        return (obj == null) ? "null" : obj.toString();
    }
}
"#;

    pub const MAP: &str = r#"package java.util;

public interface Map<K, V> {
    int size();
    boolean isEmpty();
    V get(Object key);
    interface Entry<K, V> {
        K getKey();
        V getValue();
        public static <K extends Comparable<? super K>, V> Comparator<Map.Entry<K, V>> comparingByKey() {
            return (Comparator<Map.Entry<K, V>> & Serializable)
                (c1, c2) -> c1.getKey().compareTo(c2.getKey());
        }
    }
    static <K, V> Map<K, V> of() {
        return ImmutableCollections.emptyMap();
    }
}
"#;

    pub fn jdk_zip() -> Cursor<Vec<u8>> {
        let mut w = zip::ZipWriter::new(Cursor::new(Vec::new()));
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, text) in [
            ("java.base/module-info.java", "module java.base {}\n"),
            ("java.base/java/util/ArrayList.java", ARRAY_LIST),
            ("java.base/java/lang/String.java", STRING),
            ("java.base/java/util/Map.java", MAP),
            ("META-INF/MANIFEST.MF", "Manifest-Version: 1.0\n"),
        ] {
            w.start_file(name, opts).unwrap();
            w.write_all(text.as_bytes()).unwrap();
        }
        let mut c = w.finish().unwrap();
        c.set_position(0);
        c
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicBool;

    fn names(decls: &[LibDecl]) -> Vec<(String, String)> {
        decls.iter().map(|d| (d.kind.clone(), d.fq_name.clone())).collect()
    }

    #[test]
    fn package_from_entry_strips_the_jdk_module() {
        assert_eq!(
            java_package_of("java.base/java/util/ArrayList.java", true),
            (Some("java.base".into()), "java.util".into())
        );
        assert_eq!(
            java_package_of("jdk.compiler/com/sun/tools/javac/Main.java", true),
            (Some("jdk.compiler".into()), "com.sun.tools.javac".into())
        );
        assert_eq!(java_package_of("com/google/gson/Gson.java", false), (None, "com.google.gson".into()));
        // a maven jar whose first dir happens to be dotted keeps it
        assert_eq!(java_package_of("com/google/gson/Gson.java", true), (None, "com.google.gson".into()));
        assert_eq!(java_package_of("Top.java", true), (None, String::new()));
        assert!(skip_java_entry("java.base/module-info.java"));
        assert!(skip_java_entry("META-INF/x.java"));
        assert!(!skip_java_entry("a/B.java"));
    }

    #[test]
    fn java_regex_types_members_and_nesting() {
        let d = java_from_regex(fixture::ARRAY_LIST, "java.base/java/util/ArrayList.java", "java.util");
        let n = names(&d);
        assert!(n.contains(&("class".into(), "java.util.ArrayList".into())));
        assert!(n.contains(&("field".into(), "java.util.ArrayList.DEFAULT_CAPACITY".into())));
        assert!(n.contains(&("field".into(), "java.util.ArrayList.elementData".into())));
        assert!(n.contains(&("field".into(), "java.util.ArrayList.size".into())));
        assert!(n.contains(&("method".into(), "java.util.ArrayList.ArrayList".into())), "constructor");
        assert!(n.contains(&("method".into(), "java.util.ArrayList.size".into())));
        assert!(n.contains(&("method".into(), "java.util.ArrayList.get".into())));
        assert!(n.contains(&("method".into(), "java.util.ArrayList.elementData".into())));
        assert!(n.contains(&("class".into(), "java.util.ArrayList$Itr".into())));
        assert!(n.contains(&("field".into(), "java.util.ArrayList$Itr.cursor".into())));
        assert!(n.contains(&("method".into(), "java.util.ArrayList$Itr.hasNext".into())));
        // nothing from method bodies
        assert!(!n.iter().any(|(_, f)| f.ends_with(".checkIndex") || f.ends_with(".this")));
        let itr = d.iter().find(|x| x.simple_name == "Itr").unwrap();
        assert_eq!(itr.container.as_deref(), Some("java.util.ArrayList"));
        assert_eq!(itr.line, 36);
        let top = d.iter().find(|x| x.simple_name == "ArrayList" && x.kind == "class").unwrap();
        assert_eq!(top.container, None);
        assert_eq!(top.line, 10);

        let m = names(&java_from_regex(fixture::MAP, "java.base/java/util/Map.java", "java.util"));
        assert!(m.contains(&("interface".into(), "java.util.Map".into())));
        assert!(m.contains(&("interface".into(), "java.util.Map$Entry".into())));
        assert!(m.contains(&("method".into(), "java.util.Map.size".into())));
        assert!(m.contains(&("method".into(), "java.util.Map$Entry.getKey".into())));
        assert!(m.contains(&("method".into(), "java.util.Map$Entry.comparingByKey".into())));
        assert!(m.contains(&("method".into(), "java.util.Map.of".into())));

        let s = names(&java_from_regex(fixture::STRING, "x/String.java", "java.lang"));
        assert!(s.contains(&("class".into(), "java.lang.String".into())));
        assert!(s.contains(&("field".into(), "java.lang.String.value".into())));
        assert!(s.contains(&("method".into(), "java.lang.String.length".into())));
        assert!(s.contains(&("method".into(), "java.lang.String.valueOf".into())));

        let a = java_from_regex("package p;\npublic @interface Marker {\n  String value() default \"\";\n}\n", "p/Marker.java", "p");
        assert_eq!(names(&a), vec![("annotation".into(), "p.Marker".into()), ("method".into(), "p.Marker.value".into())]);
        let e = java_from_regex("public enum Color { RED, GREEN;\n  public int code() { return 1; } }\n", "Color.java", "");
        assert_eq!(names(&e)[0], ("enum".into(), "Color".into()));
        assert!(names(&e).contains(&("method".into(), "Color.code".into())));
        let r = java_from_regex("package p;\nrecord Point(int x, int y) {}\n", "p/Point.java", "p");
        assert_eq!(names(&r), vec![("record".into(), "p.Point".into())]);
    }

    #[test]
    fn scanner_ignores_braces_in_strings_and_comments() {
        let mut sc = Scanner::new("a { /* } */ \"}\" '}' // }\n/* multi\n line } */ }\n");
        let (_, _, b0, a0) = sc.next_line().unwrap();
        assert_eq!((b0, a0), (0, 1));
        let (_, _, b1, a1) = sc.next_line().unwrap();
        assert_eq!((b1, a1), (1, 1));
        let (_, _, b2, a2) = sc.next_line().unwrap();
        assert_eq!((b2, a2), (1, 0));
    }

    #[test]
    fn rust_module_paths_and_items() {
        assert_eq!(rust_module_path("src/lib.rs"), Some(vec![]));
        assert_eq!(rust_module_path("src/main.rs"), Some(vec![]));
        assert_eq!(rust_module_path("src/de/mod.rs"), Some(vec!["de".into()]));
        assert_eq!(rust_module_path("src/de/value.rs"), Some(vec!["de".into(), "value".into()]));
        assert_eq!(rust_module_path("src\\de\\value.rs"), Some(vec!["de".into(), "value".into()]));
        assert_eq!(rust_module_path("src/bin/tool.rs"), None);
        assert_eq!(rust_module_path("tests/x.rs"), None);
        assert_eq!(rust_module_path("build.rs"), None);
        assert_eq!(normalise_crate("serde-json"), "serde_json");

        let src = r#"//! docs
use std::fmt;

pub struct Value { x: i32 }
struct Private;
pub(crate) enum Kind { A, B }
pub trait Ser {
    fn serialize(&self);
    fn helper() {
        fn nested() {}
    }
}
impl Value {
    pub fn new() -> Self { Value { x: 0 } }
    fn private_fn(&self) {}
    pub const LIMIT: usize = 1;
}
impl<'a> fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result { Ok(()) }
}
pub fn free(s: &str) -> char { '{' }
pub unsafe fn danger() {}
pub async fn later() {}
pub mod inner {
    pub fn deep() {}
    pub struct Deep;
}
mod hidden {
    pub fn not_reachable_but_indexed() {}
}
pub mod external;
pub type Alias = Value;
pub const C: u8 = 0;
pub static S: u8 = 0;
#[macro_export]
macro_rules! json {
    ($e:expr) => { $e };
}
"#;
        let d = rust_from_regex(src, "src/lib.rs", "serde_json");
        let n = names(&d);
        assert!(n.contains(&("struct".into(), "serde_json::Value".into())));
        assert!(!n.iter().any(|(_, f)| f.ends_with("::Private")));
        assert!(n.contains(&("enum".into(), "serde_json::Kind".into())));
        assert!(n.contains(&("trait".into(), "serde_json::Ser".into())));
        assert!(n.contains(&("fn".into(), "serde_json::Ser::serialize".into())));
        assert!(n.contains(&("fn".into(), "serde_json::Ser::helper".into())));
        assert!(!n.iter().any(|(_, f)| f.ends_with("::nested")));
        assert!(n.contains(&("fn".into(), "serde_json::Value::new".into())));
        assert!(!n.iter().any(|(_, f)| f.ends_with("::private_fn")), "non-pub impl fn");
        assert!(n.contains(&("const".into(), "serde_json::Value::LIMIT".into())));
        assert!(!n.iter().any(|(_, f)| f.ends_with("::fmt")), "trait impl methods are not rows");
        assert!(n.contains(&("fn".into(), "serde_json::free".into())));
        assert!(n.contains(&("fn".into(), "serde_json::danger".into())));
        assert!(n.contains(&("fn".into(), "serde_json::later".into())));
        assert!(n.contains(&("mod".into(), "serde_json::inner".into())));
        assert!(n.contains(&("fn".into(), "serde_json::inner::deep".into())));
        assert!(n.contains(&("struct".into(), "serde_json::inner::Deep".into())));
        assert!(n.contains(&("fn".into(), "serde_json::hidden::not_reachable_but_indexed".into())));
        assert!(n.contains(&("mod".into(), "serde_json::external".into())));
        assert!(n.contains(&("type".into(), "serde_json::Alias".into())));
        assert!(n.contains(&("const".into(), "serde_json::C".into())));
        assert!(n.contains(&("static".into(), "serde_json::S".into())));
        assert!(n.contains(&("macro".into(), "serde_json::json".into())));
        let new = d.iter().find(|x| x.simple_name == "new").unwrap();
        assert_eq!(new.container.as_deref(), Some("serde_json::Value"));
        assert_eq!(new.line, 13);
        assert_eq!(d.iter().find(|x| x.simple_name == "Value").unwrap().line, 3);
    }

    #[test]
    fn zip_job_indexes_java_files_and_reports_progress() {
        let mut archive = zip::ZipArchive::new(fixture::jdk_zip()).unwrap();
        let cancel = AtomicBool::new(false);
        let mut rows: Vec<LibDecl> = Vec::new();
        let mut progress = Vec::new();
        assert_eq!(java_entry_count(&archive), 3);
        let end = index_java_zip(&mut archive, true, None, &cancel, &mut rows, &mut |p| progress.push(p), Limits::default()).unwrap();
        let JobEnd::Done(stats) = end else { panic!("{end:?}") };
        assert_eq!(stats.files, 3);
        assert_eq!(stats.decls, rows.len());
        assert_eq!(progress.last().unwrap().done, 3);
        assert_eq!(progress.last().unwrap().total, 3);
        let fqs: Vec<&str> = rows.iter().map(|d| d.fq_name.as_str()).collect();
        assert!(fqs.contains(&"java.util.ArrayList"));
        assert!(fqs.contains(&"java.lang.String"));
        assert!(fqs.contains(&"java.util.Map$Entry"));
        let entry = rows.iter().find(|d| d.fq_name == "java.util.Map$Entry").unwrap();
        assert_eq!(entry.entry, "java.base/java/util/Map.java");
        assert_eq!(entry.lang, "java");
        assert!(rows.iter().all(|d| d.lang == "java"));
        // cancel before the first file → nothing
        cancel.store(true, Ordering::Relaxed);
        let mut none: Vec<LibDecl> = Vec::new();
        let end = index_java_zip(&mut archive, true, None, &cancel, &mut none, &mut |_| {}, Limits::default()).unwrap();
        assert!(matches!(end, JobEnd::Cancelled(s) if s.files == 0));
        assert!(none.is_empty());
        // the declaration guard: the fixture has > 5 rows in its first file
        cancel.store(false, Ordering::Relaxed);
        let mut few: Vec<LibDecl> = Vec::new();
        let end = index_java_zip(&mut archive, true, None, &cancel, &mut few, &mut |_| {}, Limits { max_decls: 5 }).unwrap();
        let JobEnd::Skipped(reason) = end else { panic!("{end:?}") };
        assert!(reason.contains("over the 5 limit"), "{reason}");
        assert!(!few.is_empty(), "rows written before the guard are the caller's to drop");
    }

    #[test]
    fn cargo_job_walks_src_and_names_by_path() {
        let t = tempfile::tempdir().unwrap();
        let dir = t.path().join("serde-json-1.0.0");
        std::fs::create_dir_all(dir.join("src").join("de")).unwrap();
        std::fs::create_dir_all(dir.join("src").join("bin")).unwrap();
        std::fs::create_dir_all(dir.join("tests")).unwrap();
        std::fs::write(dir.join("src").join("lib.rs"), "pub mod de;\npub struct Value;\n").unwrap();
        std::fs::write(dir.join("src").join("de").join("mod.rs"), "pub fn from_str() {}\n").unwrap();
        std::fs::write(dir.join("src").join("de").join("read.rs"), "pub trait Read {}\n").unwrap();
        std::fs::write(dir.join("src").join("bin").join("x.rs"), "pub fn main_ish() {}\n").unwrap();
        std::fs::write(dir.join("tests").join("t.rs"), "pub fn test_only() {}\n").unwrap();
        std::fs::create_dir_all(dir.join("benches")).unwrap();
        std::fs::write(dir.join("benches").join("b.rs"), "pub fn bench_only() {}\n").unwrap();
        assert_eq!(rust_files(&dir).len(), 4, "src/** only (bin included in the walk, dropped by the module path)");
        let cancel = AtomicBool::new(false);
        let mut rows: Vec<LibDecl> = Vec::new();
        let end = index_cargo_dir(&dir, "serde-json", &cancel, &mut rows, &mut |_| {}, Limits::default()).unwrap();
        let JobEnd::Done(stats) = end else { panic!("{end:?}") };
        assert_eq!(stats.files, 3);
        let mut fqs: Vec<(&str, &str)> = rows.iter().map(|d| (d.fq_name.as_str(), d.entry.as_str())).collect();
        fqs.sort();
        assert_eq!(
            fqs,
            vec![
                ("serde_json::Value", "src/lib.rs"),
                ("serde_json::de", "src/lib.rs"),
                ("serde_json::de::from_str", "src/de/mod.rs"),
                ("serde_json::de::read::Read", "src/de/read.rs"),
            ]
        );
        assert!(rows.iter().all(|d| d.lang == "rust"));
        let mut few: Vec<LibDecl> = Vec::new();
        let end = index_cargo_dir(&dir, "serde-json", &cancel, &mut few, &mut |_| {}, Limits { max_decls: 1 }).unwrap();
        assert!(matches!(end, JobEnd::Skipped(_)), "{end:?}");
    }
}
