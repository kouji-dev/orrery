//! Pure name resolution against the library index (M4): which fully
//! qualified names a bare `word` may denote given the asking file's header,
//! and how simple-name fallbacks rank (the asking project's own source
//! first, then the JDK).
//!
//! Java: explicit `import a.b.Name;` → same package → wildcard `import a.b.*;`
//! → `java.lang.Name` → nested `a.b.Outer$Name` for every explicit import.
//! A dotted word (`Map.Entry`) resolves its head and appends `$Entry`.
//! Rust: `use crate_name::path::Item` (braces, `as`, globs expanded) → the
//! crate at the version the project's `Cargo.lock` pins → the item's fq path.

use serde::Serialize;

use crate::symbols::store::LibDeclHit;

/// One library declaration as the router hands it to the frontend.
#[derive(Debug, Clone, Serialize, PartialEq, Eq)]
#[serde(rename_all = "camelCase")]
pub struct LibHit {
    /// `orrery-lib://<sourceId>/<entry>`.
    pub uri: String,
    pub line: u32,
    pub col: u32,
    pub fq_name: String,
    pub kind: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    /// Source label (`JDK 21 (openjdk21)`, `Cargo · orrery`, …).
    pub label: String,
    pub source_id: String,
    /// `<crate>/src/lib.rs`, `<jar>!/com/x/Y.java`, `java.base/java/util/Map.java`.
    pub entry: String,
    /// The artifact the entry lives in (`serde-1.0.219`, `gson-2.11.0-sources.jar`).
    pub artifact: String,
    pub lang: String,
}

pub fn lib_uri(source_id: &str, entry: &str) -> String {
    format!("orrery-lib://{source_id}/{entry}")
}

impl LibHit {
    pub fn from_decl(h: &LibDeclHit) -> Self {
        Self {
            uri: lib_uri(&h.source_id, &h.decl.entry),
            line: h.decl.line,
            col: 0,
            fq_name: h.decl.fq_name.clone(),
            kind: h.decl.kind.clone(),
            container: h.decl.container.clone(),
            label: h.source_label.clone(),
            source_id: h.source_id.clone(),
            entry: h.decl.entry.clone(),
            artifact: h.artifact.clone(),
            lang: h.decl.lang.clone(),
        }
    }
}

// ---- java ---------------------------------------------------------------------------

/// The asking file's header.
#[derive(Debug, Clone, Default)]
pub struct JavaCtx<'a> {
    pub package: Option<&'a str>,
    /// `a.b.Name`, `a.b.*` (static imports already stripped of `static`).
    pub imports: &'a [String],
}

fn push_unique(out: &mut Vec<String>, v: String) {
    if !out.contains(&v) {
        out.push(v);
    }
}

/// Ordered fully qualified candidates for `word`.
pub fn java_candidates(word: &str, ctx: &JavaCtx<'_>) -> Vec<String> {
    let word = word.trim();
    let (head, nested) = match word.split_once('.') {
        Some((h, rest)) => (h, Some(rest.replace('.', "$"))),
        None => (word, None),
    };
    if head.is_empty() || !head.chars().all(|c| c.is_alphanumeric() || c == '_' || c == '$') {
        return Vec::new();
    }
    let mut out = Vec::new();
    // 1. explicit import of exactly this name
    for imp in ctx.imports {
        if imp.ends_with(".*") {
            continue;
        }
        if imp.rsplit('.').next() == Some(head) {
            push_unique(&mut out, imp.clone());
        }
    }
    // 2. same package
    match ctx.package {
        Some(p) if !p.is_empty() => push_unique(&mut out, format!("{p}.{head}")),
        _ => push_unique(&mut out, head.to_string()),
    }
    // 3. wildcard imports
    for imp in ctx.imports {
        if let Some(pkg) = imp.strip_suffix(".*") {
            push_unique(&mut out, format!("{pkg}.{head}"));
        }
    }
    // 4. java.lang
    push_unique(&mut out, format!("java.lang.{head}"));
    // 5. nested in an explicitly imported type (`import java.util.Map;` → `Map$Entry`)
    for imp in ctx.imports {
        if imp.ends_with(".*") || imp.rsplit('.').next() == Some(head) {
            continue;
        }
        if imp.rsplit('.').next().is_some_and(|last| last.chars().next().is_some_and(char::is_uppercase)) {
            push_unique(&mut out, format!("{imp}${head}"));
        }
    }
    match nested {
        Some(rest) => out.into_iter().map(|c| format!("{c}${rest}")).collect(),
        None => out,
    }
}

// ---- rust ---------------------------------------------------------------------------

/// Expand one `use` path (whitespace already removed by the header scan):
/// `serde::{Serialize,de::Deserialize as D}` → `[(serde::Serialize, Serialize),
/// (serde::de::Deserialize, D)]`; `serde::*` → `[(serde::*, *)]`.
fn expand_use(path: &str, out: &mut Vec<(String, String)>) {
    let path = path.trim().trim_start_matches("::");
    if let Some(open) = path.find('{') {
        let prefix = &path[..open];
        let Some(close) = path.rfind('}') else { return };
        let inner = &path[open + 1..close];
        // split on top-level commas only
        let mut depth = 0usize;
        let mut start = 0;
        let bytes = inner.as_bytes();
        for (i, &b) in bytes.iter().enumerate() {
            match b {
                b'{' => depth += 1,
                b'}' => depth = depth.saturating_sub(1),
                b',' if depth == 0 => {
                    expand_use(&format!("{prefix}{}", &inner[start..i]), out);
                    start = i + 1;
                }
                _ => {}
            }
        }
        expand_use(&format!("{prefix}{}", &inner[start..]), out);
        return;
    }
    let (path, alias) = match path.split_once(" as ") {
        Some((p, a)) => (p.trim(), Some(a.trim())),
        None => match path.rsplit_once("as") {
            // whitespace was stripped: `Deserializeas D` cannot happen, but
            // `…::DeserializeasD` can — only split when both sides are idents
            Some((p, a)) if !p.is_empty() && !a.is_empty() && p.ends_with(|c: char| c.is_alphanumeric() || c == '_') && a.chars().next().is_some_and(|c| c.is_uppercase() || c == '_') && !p.ends_with("::") => (p, Some(a)),
            _ => (path, None),
        },
    };
    let path = path.trim_end_matches("::self");
    if path.is_empty() {
        return;
    }
    let last = path.rsplit("::").next().unwrap_or(path).to_string();
    let name = alias.map(str::to_string).unwrap_or(last);
    out.push((path.to_string(), name));
}

/// Ordered `(crate, fq path)` candidates for `word` from the file's `use`
/// lines. `crate::`/`self::`/`super::`/`std`-family paths are project-local
/// or toolchain and never in the registry.
pub fn rust_candidates(word: &str, uses: &[String]) -> Vec<(String, String)> {
    let word = word.trim();
    if word.is_empty() || !word.chars().all(|c| c.is_alphanumeric() || c == '_') {
        return Vec::new();
    }
    let mut expanded = Vec::new();
    for u in uses {
        expand_use(u, &mut expanded);
    }
    let mut out: Vec<(String, String)> = Vec::new();
    let mut push = |krate: &str, fq: String| {
        let k = krate.replace('-', "_");
        if !matches!(k.as_str(), "crate" | "self" | "super" | "std" | "core" | "alloc")
            && !out.iter().any(|(_, f)| *f == fq)
        {
            out.push((k, fq));
        }
    };
    for (path, name) in &expanded {
        let Some(krate) = path.split("::").next() else { continue };
        if name == word {
            push(krate, path.clone());
        }
    }
    for (path, name) in &expanded {
        let Some(krate) = path.split("::").next() else { continue };
        if name == "*" {
            let base = path.trim_end_matches('*').trim_end_matches("::");
            push(krate, format!("{base}::{word}"));
        }
    }
    out
}

// ---- ranking -------------------------------------------------------------------------

/// Position of a hit's source in the preferred order (the asking project's
/// own source first, then the JDKs); unknown sources last.
pub fn source_rank(source_id: &str, order: &[String]) -> usize {
    order.iter().position(|s| s == source_id).unwrap_or(order.len())
}

fn kind_rank(kind: &str, type_word: bool) -> u8 {
    let is_type = matches!(
        kind,
        "class" | "interface" | "enum" | "record" | "annotation" | "struct" | "trait" | "type" | "union" | "mod"
    );
    match (type_word, is_type) {
        (true, true) | (false, false) => 0,
        _ => 1,
    }
}

/// Order simple-name fallbacks: source order, then kind fit, then the
/// shorter fq name. Stable for equal keys.
pub fn rank_hits(hits: &mut [LibDeclHit], word: &str, order: &[String]) {
    let type_word = word.chars().next().is_some_and(char::is_uppercase);
    hits.sort_by_key(|h| {
        (
            source_rank(&h.source_id, order),
            kind_rank(&h.decl.kind, type_word),
            h.decl.fq_name.len(),
            h.decl.fq_name.clone(),
        )
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::store::LibDecl;

    fn ctx<'a>(package: Option<&'a str>, imports: &'a [String]) -> JavaCtx<'a> {
        JavaCtx { package, imports }
    }

    fn s(v: &[&str]) -> Vec<String> {
        v.iter().map(|x| x.to_string()).collect()
    }

    #[test]
    fn java_order_explicit_package_wildcard_lang_nested() {
        let imports = s(&["java.util.Map", "java.util.List", "com.acme.util.*", "java.io.IOException"]);
        let c = java_candidates("List", &ctx(Some("com.acme.shop"), &imports));
        assert_eq!(
            c,
            s(&[
                "java.util.List",
                "com.acme.shop.List",
                "com.acme.util.List",
                "java.lang.List",
                "java.util.Map$List",
                "java.io.IOException$List",
            ])
        );
        // nested in an imported type
        let c = java_candidates("Entry", &ctx(Some("com.acme.shop"), &imports));
        assert_eq!(c[0], "com.acme.shop.Entry");
        assert!(c.contains(&"java.util.Map$Entry".to_string()));
        // no import, default package → `String` reaches java.lang
        let c = java_candidates("String", &ctx(None, &[]));
        assert_eq!(c, s(&["String", "java.lang.String"]));
        // dotted word
        let c = java_candidates("Map.Entry", &ctx(Some("p"), &s(&["java.util.Map"])));
        assert_eq!(c[0], "java.util.Map$Entry");
        assert!(c.contains(&"p.Map$Entry".to_string()));
        assert!(java_candidates("", &ctx(None, &[])).is_empty());
        assert!(java_candidates("a-b", &ctx(None, &[])).is_empty());
    }

    #[test]
    fn rust_use_expansion_and_candidates() {
        let uses = s(&[
            "serde::{Serialize,Deserialize}",
            "serde_json::Value",
            "tokio::sync::{mpsc,oneshot::{Sender,Receiver}}",
            "regex::Regex as Re",
            "anyhow::*",
            "crate::store::Store",
            "std::collections::HashMap",
            "gix::prelude::*",
        ]);
        let c = rust_candidates("Serialize", &uses);
        assert_eq!(c[0], ("serde".to_string(), "serde::Serialize".to_string()), "exact use first");
        assert_eq!(c.len(), 3, "then the two globs as fallbacks");
        assert_eq!(rust_candidates("Value", &uses)[0], ("serde_json".to_string(), "serde_json::Value".to_string()));
        assert_eq!(rust_candidates("Receiver", &uses)[0].1, "tokio::sync::oneshot::Receiver");
        assert_eq!(rust_candidates("mpsc", &uses)[0].1, "tokio::sync::mpsc");
        assert_eq!(rust_candidates("Re", &uses)[0].1, "regex::Regex");
        assert!(rust_candidates("Regex", &uses).is_empty() || rust_candidates("Regex", &uses)[0].1 != "regex::Regex");
        // globs come after exact matches, project/std paths are dropped
        let c = rust_candidates("Store", &uses);
        assert_eq!(c, vec![("anyhow".to_string(), "anyhow::Store".to_string()), ("gix".to_string(), "gix::prelude::Store".to_string())]);
        assert!(rust_candidates("HashMap", &uses).iter().all(|(k, _)| k != "std"));
        // hyphenated crate dirs normalise
        let c = rust_candidates("X", &s(&["windows-sys::X"]));
        assert_eq!(c[0].0, "windows_sys");
        assert!(rust_candidates("", &uses).is_empty());
    }

    #[test]
    fn ranking_follows_the_source_order_then_kind_fit() {
        let order = s(&["maven:p1", "jdk:a"]);
        assert_eq!(source_rank("maven:p1", &order), 0);
        assert_eq!(source_rank("jdk:a", &order), 1);
        assert_eq!(source_rank("maven:other", &order), 2);
        let hit = |src: &str, dkind: &str, fq: &str| LibDeclHit {
            source_id: src.into(),
            source_kind: if src.starts_with("jdk") { "jdk" } else { "maven" }.into(),
            source_label: src.into(),
            artifact: "a".into(),
            decl: LibDecl {
                fq_name: fq.into(),
                simple_name: "List".into(),
                kind: dkind.into(),
                container: None,
                entry: "e".into(),
                line: 0,
                lang: "java".into(),
            },
        };
        let mut hits = vec![
            hit("maven:other", "class", "o.List"),
            hit("jdk:a", "method", "java.util.X.List"),
            hit("jdk:a", "interface", "java.util.List"),
            hit("maven:p1", "class", "com.google.gson.List"),
        ];
        rank_hits(&mut hits, "List", &order);
        let fqs: Vec<&str> = hits.iter().map(|h| h.decl.fq_name.as_str()).collect();
        assert_eq!(fqs, vec!["com.google.gson.List", "java.util.List", "java.util.X.List", "o.List"]);
        let h = LibHit::from_decl(&hits[1]);
        assert_eq!(h.uri, "orrery-lib://jdk:a/e");
        assert_eq!(h.artifact, "a");
    }
}
