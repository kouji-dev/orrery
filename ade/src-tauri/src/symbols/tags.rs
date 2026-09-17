//! Pure symbol extraction (M2): run a grammar pack's `tags.scm` over one
//! file's bytes and reduce the matches to a flat, serializable [`FileSymbols`]
//! — definitions with UTF-16 ranges + enclosing container, references, and
//! the package/import header read with a regex over the first lines.
//!
//! No tree is retained past this function; the output is what the index and
//! the content-addressed blob cache store. Bounded on purpose: files above
//! `MAX_BYTES` / `MAX_LINES` are rejected, and a parse that exceeds
//! `PARSE_BUDGET` (or sees the cancel flag) is abandoned via tree-sitter's
//! progress callback — a pathological file must never stall the indexer pool.

use std::ops::ControlFlow;
use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::OnceLock;
use std::time::{Duration, Instant};

use regex::Regex;
use serde::{Deserialize, Serialize};
use tree_sitter::{Node, ParseOptions, ParseState, Parser, QueryCursor, StreamingIterator};

use crate::extensions::loader::Loaded;

/// Files above this are generated/minified — never worth a parse.
pub const MAX_BYTES: usize = 512 * 1024;
pub const MAX_LINES: usize = 20_000;
/// Wall-clock budget for ONE parse.
pub const PARSE_BUDGET: Duration = Duration::from_millis(200);
/// Signature preview cap (chars).
pub const MAX_SIG: usize = 200;
/// Header scan depth for package/import extraction.
const HEADER_LINES: usize = 50;

/// One definition. Ranges are 0-based lines and UTF-16 columns (what Monaco
/// and JS strings index by). `line..end_line` spans the whole node, `sel_*`
/// the name identifier only. `container` indexes into the owning
/// `FileSymbols::defs` (nearest enclosing definition by byte range).
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Def {
    pub name: String,
    /// `class | interface | enum | struct | function | method | constructor |
    /// field | property | variable | constant | module | type | macro | trait
    /// | impl` — whatever follows `definition.` in the capture name.
    pub kind: String,
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub sel_line: u32,
    pub sel_col: u32,
    pub sel_end_line: u32,
    pub sel_end_col: u32,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub container: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub signature: Option<String>,
}

/// One reference (`reference.<kind>`); the range is the NAME node's.
#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct Ref {
    pub name: String,
    /// `call | class | type | implementation | field | …`.
    pub kind: String,
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(Debug, Clone, PartialEq, Eq, Default, Serialize, Deserialize)]
pub struct FileSymbols {
    #[serde(default)]
    pub defs: Vec<Def>,
    #[serde(default)]
    pub refs: Vec<Ref>,
    #[serde(default)]
    pub imports: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub package: Option<String>,
}

impl FileSymbols {
    pub fn encode(&self) -> Vec<u8> {
        serde_json::to_vec(self).unwrap_or_default()
    }

    pub fn decode(bytes: &[u8]) -> Option<Self> {
        serde_json::from_slice(bytes).ok()
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum ExtractError {
    TooLarge(usize),
    TooManyLines(usize),
    Language(String),
    Timeout,
    Cancelled,
    NoTree,
}

impl std::fmt::Display for ExtractError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            ExtractError::TooLarge(n) => write!(f, "file too large ({n} bytes)"),
            ExtractError::TooManyLines(n) => write!(f, "too many lines ({n})"),
            ExtractError::Language(e) => write!(f, "set_language: {e}"),
            ExtractError::Timeout => write!(f, "parse exceeded {PARSE_BUDGET:?}"),
            ExtractError::Cancelled => write!(f, "cancelled"),
            ExtractError::NoTree => write!(f, "parser returned no tree"),
        }
    }
}

/// Extract with a throwaway parser. The indexer uses [`extract_with`] and a
/// thread-local parser instead.
pub fn extract(bytes: &[u8], grammar: &Loaded) -> Result<FileSymbols, ExtractError> {
    let mut parser = Parser::new();
    extract_with(bytes, grammar, &mut parser, None)
}

/// Parse + tag `bytes` with `grammar`. `cancel` is polled from the parse
/// progress callback so a stop request lands mid-file, not at the next one.
pub fn extract_with(
    bytes: &[u8],
    grammar: &Loaded,
    parser: &mut Parser,
    cancel: Option<&AtomicBool>,
) -> Result<FileSymbols, ExtractError> {
    if bytes.len() > MAX_BYTES {
        return Err(ExtractError::TooLarge(bytes.len()));
    }
    let lines = bytes.iter().filter(|b| **b == b'\n').count() + 1;
    if lines > MAX_LINES {
        return Err(ExtractError::TooManyLines(lines));
    }
    let (package, imports) = header(bytes, &grammar.manifest.language);
    let mut out = FileSymbols {
        package,
        imports,
        ..Default::default()
    };
    let Some(query) = grammar.tags.as_ref() else {
        return Ok(out); // pack ships no tags query: header only
    };
    parser
        .set_language(&grammar.language)
        .map_err(|e| ExtractError::Language(e.to_string()))?;

    let started = Instant::now();
    let mut aborted_by_cancel = false;
    let mut progress = |_: &ParseState| {
        if cancel.is_some_and(|c| c.load(Ordering::Relaxed)) {
            aborted_by_cancel = true;
            return ControlFlow::Break(());
        }
        if started.elapsed() > PARSE_BUDGET {
            return ControlFlow::Break(());
        }
        ControlFlow::Continue(())
    };
    let opts = ParseOptions::new().progress_callback(&mut progress);
    let tree = parser.parse_with_options(
        &mut |offset, _| {
            if offset < bytes.len() {
                &bytes[offset..]
            } else {
                &[]
            }
        },
        None,
        Some(opts),
    );
    // Never leave a parser mid-parse for the next caller.
    parser.reset();
    let Some(tree) = tree else {
        return Err(if aborted_by_cancel {
            ExtractError::Cancelled
        } else if started.elapsed() > PARSE_BUDGET {
            ExtractError::Timeout
        } else {
            ExtractError::NoTree
        });
    };

    let names = query.capture_names();
    let mut cursor = QueryCursor::new();
    let mut matches = cursor.matches(query, tree.root_node(), bytes);
    // (start_byte, end_byte, Def) — containers resolved after the sweep.
    let mut raw: Vec<(usize, usize, Def)> = Vec::new();
    let mut seen_def: std::collections::HashSet<(usize, usize, String)> =
        std::collections::HashSet::new();
    while let Some(m) = matches.next() {
        let mut name_node: Option<Node> = None;
        let mut def: Option<(&str, Node)> = None;
        let mut reference: Option<&str> = None;
        for c in m.captures() {
            let cname = names[c.index as usize];
            if cname == "name" {
                name_node = Some(c.node);
            } else if let Some(k) = cname.strip_prefix("definition.") {
                def = Some((k, c.node));
            } else if let Some(k) = cname.strip_prefix("reference.") {
                reference = Some(k);
            }
        }
        let Some(nn) = name_node else {
            continue;
        };
        let Ok(name) = nn.utf8_text(bytes) else {
            continue;
        };
        if name.is_empty() {
            continue;
        }
        if let Some((kind, node)) = def {
            let key = (node.start_byte(), kind.len(), name.to_string());
            if !seen_def.insert(key) {
                continue;
            }
            let (line, col) = pos16(bytes, node.start_byte(), node.start_position());
            let (end_line, end_col) = pos16(bytes, node.end_byte(), node.end_position());
            let (sel_line, sel_col) = pos16(bytes, nn.start_byte(), nn.start_position());
            let (sel_end_line, sel_end_col) = pos16(bytes, nn.end_byte(), nn.end_position());
            raw.push((
                node.start_byte(),
                node.end_byte(),
                Def {
                    name: name.to_string(),
                    kind: kind.to_string(),
                    line,
                    col,
                    end_line,
                    end_col,
                    sel_line,
                    sel_col,
                    sel_end_line,
                    sel_end_col,
                    container: None,
                    signature: signature(&bytes[node.byte_range()]),
                },
            ));
        } else if let Some(kind) = reference {
            let (line, col) = pos16(bytes, nn.start_byte(), nn.start_position());
            let (end_line, end_col) = pos16(bytes, nn.end_byte(), nn.end_position());
            out.refs.push(Ref {
                name: name.to_string(),
                kind: kind.to_string(),
                line,
                col,
                end_line,
                end_col,
            });
        }
    }
    drop(matches);

    // Containers: outer-first order, then a stack of open ranges.
    raw.sort_by(|a, b| a.0.cmp(&b.0).then(b.1.cmp(&a.1)));
    let mut stack: Vec<(usize, u32)> = Vec::new(); // (end_byte, index)
    for (i, (start, end, def)) in raw.iter_mut().enumerate() {
        while stack.last().is_some_and(|(e, _)| *e <= *start) {
            stack.pop();
        }
        def.container = stack.last().map(|(_, idx)| *idx);
        stack.push((*end, i as u32));
    }
    out.defs = raw.into_iter().map(|(_, _, d)| d).collect();
    Ok(out)
}

/// tree-sitter reports columns in BYTES; convert to UTF-16 units by
/// re-decoding the line prefix (the same rule `search/mod.rs` applies to
/// match ranges).
fn pos16(bytes: &[u8], byte_offset: usize, p: tree_sitter::Point) -> (u32, u32) {
    let start = byte_offset.saturating_sub(p.column);
    let prefix = &bytes[start.min(bytes.len())..byte_offset.min(bytes.len())];
    let units = String::from_utf8_lossy(prefix).encode_utf16().count();
    (p.row as u32, units as u32)
}

/// First line of the node text, trimmed, ≤ `MAX_SIG` chars.
fn signature(node_bytes: &[u8]) -> Option<String> {
    let text = String::from_utf8_lossy(node_bytes);
    let first = text.lines().next()?.trim();
    if first.is_empty() {
        return None;
    }
    let mut s: String = first.chars().take(MAX_SIG).collect();
    if first.chars().count() > MAX_SIG {
        s.push('…');
    }
    Some(s)
}

struct HeaderRes {
    java_pkg: Regex,
    java_import: Regex,
    ts_from: Regex,
    ts_bare: Regex,
    ts_require: Regex,
    rust_use: Regex,
    py_import: Regex,
    py_from: Regex,
    go_pkg: Regex,
    go_import: Regex,
}

fn res() -> &'static HeaderRes {
    static RES: OnceLock<HeaderRes> = OnceLock::new();
    RES.get_or_init(|| HeaderRes {
        java_pkg: Regex::new(r"^\s*package\s+([\w.]+)\s*;").unwrap(),
        java_import: Regex::new(r"^\s*import\s+(?:static\s+)?([\w.]+(?:\.\*)?)\s*;").unwrap(),
        ts_from: Regex::new(r#"^\s*(?:import|export)\b[^'"]*?\bfrom\s+['"]([^'"]+)['"]"#).unwrap(),
        ts_bare: Regex::new(r#"^\s*import\s+['"]([^'"]+)['"]"#).unwrap(),
        ts_require: Regex::new(r#"require\(\s*['"]([^'"]+)['"]\s*\)"#).unwrap(),
        rust_use: Regex::new(r"^\s*(?:pub(?:\([^)]*\))?\s+)?use\s+([^;]+);").unwrap(),
        py_import: Regex::new(r"^\s*import\s+([\w.]+(?:\s*,\s*[\w.]+)*)").unwrap(),
        py_from: Regex::new(r"^\s*from\s+([\w.]+)\s+import\b").unwrap(),
        go_pkg: Regex::new(r"^\s*package\s+(\w+)").unwrap(),
        go_import: Regex::new(r#"^\s*(?:import\s+)?(?:\w+\s+)?"([^"]+)"\s*$"#).unwrap(),
    })
}

/// `(package, imports)` from the first `HEADER_LINES` lines, by language.
pub fn header(bytes: &[u8], language: &str) -> (Option<String>, Vec<String>) {
    let head_len = bytes
        .iter()
        .enumerate()
        .filter(|(_, b)| **b == b'\n')
        .nth(HEADER_LINES - 1)
        .map(|(i, _)| i)
        .unwrap_or(bytes.len());
    let head = String::from_utf8_lossy(&bytes[..head_len]);
    let r = res();
    let mut package = None;
    let mut imports = Vec::new();
    let lang = language.to_ascii_lowercase();
    for line in head.lines() {
        match lang.as_str() {
            "java" | "kotlin" | "scala" => {
                if let Some(c) = r.java_pkg.captures(line) {
                    package.get_or_insert_with(|| c[1].to_string());
                } else if let Some(c) = r.java_import.captures(line) {
                    imports.push(c[1].to_string());
                }
            }
            "typescript" | "tsx" | "javascript" | "jsx" => {
                if let Some(c) = r.ts_from.captures(line) {
                    imports.push(c[1].to_string());
                } else if let Some(c) = r.ts_bare.captures(line) {
                    imports.push(c[1].to_string());
                } else if let Some(c) = r.ts_require.captures(line) {
                    imports.push(c[1].to_string());
                }
            }
            "rust" => {
                if let Some(c) = r.rust_use.captures(line) {
                    imports.push(c[1].split_whitespace().collect::<String>());
                }
            }
            "python" => {
                if let Some(c) = r.py_from.captures(line) {
                    imports.push(c[1].to_string());
                } else if let Some(c) = r.py_import.captures(line) {
                    for m in c[1].split(',') {
                        let m = m.trim();
                        if !m.is_empty() {
                            imports.push(m.to_string());
                        }
                    }
                }
            }
            "go" => {
                if let Some(c) = r.go_pkg.captures(line) {
                    package.get_or_insert_with(|| c[1].to_string());
                } else if let Some(c) = r.go_import.captures(line) {
                    imports.push(c[1].to_string());
                }
            }
            _ => {}
        }
    }
    (package, imports)
}

#[cfg(test)]
pub(crate) mod fixture {
    //! Grammar-gated test support: `ORRERY_TS_FIXTURE_DIR` points either at a
    //! built pack dir (manifest.json inside) or at a dir of packs
    //! (`<dir>/grammar.java/manifest.json`).
    use std::path::PathBuf;
    use std::sync::{Arc, OnceLock};

    use crate::extensions::loader::{load, Loaded};
    use crate::extensions::manifest::Manifest;

    pub fn java() -> Option<Arc<Loaded>> {
        static G: OnceLock<Option<Arc<Loaded>>> = OnceLock::new();
        G.get_or_init(|| {
            let dir = PathBuf::from(std::env::var_os("ORRERY_TS_FIXTURE_DIR")?);
            let candidates = [dir.clone(), dir.join("grammar.java")];
            let dir = candidates.into_iter().find(|d| d.join("manifest.json").is_file())?;
            let json = std::fs::read_to_string(dir.join("manifest.json")).ok()?;
            let Manifest::Grammar(m) = Manifest::parse(&json).ok()? else {
                return None;
            };
            if m.language != "java" {
                return None;
            }
            load(&dir, &m).ok().map(Arc::new)
        })
        .clone()
    }

    /// Skip (returning None) with a message when the fixture is absent.
    pub fn java_or_skip(test: &str) -> Option<Arc<Loaded>> {
        let g = java();
        if g.is_none() {
            eprintln!("{test}: ORRERY_TS_FIXTURE_DIR unset or no java pack — skipping");
        }
        g
    }

    // Raw string on purpose: a `\`-continued literal strips the next line's
    // leading spaces, which silently shifts every indented column below.
    pub const JAVA_SRC: &str = r#"package com.acme.shop;
import java.util.List;
import com.acme.core.*;

public class Cart extends Base implements Priced {
    private int count;
    public Cart() { super(); }
    /* 𝒳 */ public int total() { return sum(count); }
    static class Line {
        int qty() { return 1; }
    }
}
interface Priced { int total(); }
"#;
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn header_java_package_and_imports() {
        let (p, i) = header(fixture::JAVA_SRC.as_bytes(), "java");
        assert_eq!(p.as_deref(), Some("com.acme.shop"));
        assert_eq!(i, vec!["java.util.List", "com.acme.core.*"]);
    }

    #[test]
    fn header_ts_rust_python_go() {
        let ts = b"import { A } from \"./a\";\nimport * as B from 'lib/b';\nimport 'side';\nconst c = require(\"c\");\n";
        let (p, i) = header(ts, "typescript");
        assert_eq!(p, None);
        assert_eq!(i, vec!["./a", "lib/b", "side", "c"]);
        let rs = b"use std::collections::HashMap;\npub use crate::a::{B, C};\nfn x() {}\n";
        let (_, i) = header(rs, "rust");
        assert_eq!(i, vec!["std::collections::HashMap", "crate::a::{B,C}"]);
        let py = b"import os, sys\nfrom a.b import c\n";
        let (_, i) = header(py, "python");
        assert_eq!(i, vec!["os", "sys", "a.b"]);
        let go = b"package main\nimport (\n\t\"fmt\"\n\tstr \"strings\"\n)\n";
        let (p, i) = header(go, "go");
        assert_eq!(p.as_deref(), Some("main"));
        assert_eq!(i, vec!["fmt", "strings"]);
    }

    #[test]
    fn header_stops_after_fifty_lines() {
        let mut src = String::new();
        for _ in 0..60 {
            src.push_str("// filler\n");
        }
        src.push_str("import late.Thing;\n");
        let (_, i) = header(src.as_bytes(), "java");
        assert!(i.is_empty());
    }

    #[test]
    fn signature_is_first_line_capped() {
        assert_eq!(signature(b"  class A {\n  int x;\n}").as_deref(), Some("class A {"));
        assert_eq!(signature(b"\n\n"), None);
        let long = "x".repeat(300);
        let s = signature(long.as_bytes()).unwrap();
        assert_eq!(s.chars().count(), MAX_SIG + 1);
        assert!(s.ends_with('…'));
    }

    #[test]
    fn pos16_counts_utf16_units_not_bytes() {
        // "é" 2 bytes → 1 unit; "𐍈" 4 bytes → 2 units
        let line = "é𐍈abc";
        let bytes = line.as_bytes();
        let byte_col = "é𐍈".len(); // 6
        let (row, col) = pos16(
            bytes,
            byte_col,
            tree_sitter::Point {
                row: 0,
                column: byte_col,
            },
        );
        assert_eq!((row, col), (0, 3));
    }

    #[test]
    fn blob_round_trip() {
        let fs = FileSymbols {
            defs: vec![Def {
                name: "A".into(),
                kind: "class".into(),
                line: 1,
                col: 2,
                end_line: 3,
                end_col: 4,
                sel_line: 1,
                sel_col: 8,
                sel_end_line: 1,
                sel_end_col: 9,
                container: Some(0),
                signature: Some("class A".into()),
            }],
            refs: vec![Ref {
                name: "b".into(),
                kind: "call".into(),
                line: 5,
                col: 6,
                end_line: 5,
                end_col: 7,
            }],
            imports: vec!["x.y".into()],
            package: Some("p".into()),
        };
        let bytes = fs.encode();
        assert_eq!(FileSymbols::decode(&bytes).unwrap(), fs);
        assert!(FileSymbols::decode(b"not json").is_none());
    }

    #[test]
    #[ignore]
    fn java_extraction_kinds_containers_ranges_and_header() {
        let Some(g) = fixture::java_or_skip("java_extraction") else {
            return;
        };
        let fs = extract(fixture::JAVA_SRC.as_bytes(), &g).unwrap();
        assert_eq!(fs.package.as_deref(), Some("com.acme.shop"));
        assert_eq!(fs.imports, vec!["java.util.List", "com.acme.core.*"]);
        let find = |name: &str, kind: &str| {
            fs.defs
                .iter()
                .position(|d| d.name == name && d.kind == kind)
                .unwrap_or_else(|| panic!("{kind} {name} in {:?}", fs.defs))
        };
        let cart = find("Cart", "class");
        let total = find("total", "method");
        let line = find("Line", "class");
        let qty = find("qty", "method");
        let priced = find("Priced", "interface");
        assert_eq!(fs.defs[cart].container, None);
        assert_eq!(fs.defs[total].container, Some(cart as u32));
        assert_eq!(fs.defs[line].container, Some(cart as u32));
        assert_eq!(fs.defs[qty].container, Some(line as u32));
        assert_eq!(fs.defs[priced].container, None);
        // upstream java tags.scm has no field pattern — `count` is not a def
        assert!(!fs.defs.iter().any(|d| d.name == "count"));
        // ranges: class starts at line 4 col 0; `total` sits after a non-BMP
        // comment: "    /* 𝒳 */ public int total()" — 𝒳 is 4 bytes / 2 units.
        assert_eq!((fs.defs[cart].line, fs.defs[cart].col), (4, 0));
        assert_eq!(fs.defs[cart].sel_col, "public class ".len() as u32);
        let t = &fs.defs[total];
        assert_eq!(t.line, 7);
        assert_eq!(t.col, "    /* 𝒳 */ ".encode_utf16().count() as u32);
        assert_eq!(t.sel_col, "    /* 𝒳 */ public int ".encode_utf16().count() as u32);
        assert_eq!(t.sel_end_col, t.sel_col + 5);
        assert_eq!(t.signature.as_deref(), Some("public int total() { return sum(count); }"));
        assert_eq!(fs.defs[cart].signature.as_deref(), Some("public class Cart extends Base implements Priced {"));
        // references: call sum, class Base (superclass), implementation Priced
        let r = |name: &str, kind: &str| fs.refs.iter().find(|r| r.name == name && r.kind == kind);
        assert!(r("sum", "call").is_some(), "{:?}", fs.refs);
        assert!(r("Base", "class").is_some(), "{:?}", fs.refs);
        assert!(r("Priced", "implementation").is_some(), "{:?}", fs.refs);
        let sum = r("sum", "call").unwrap();
        assert_eq!(sum.line, 7);
        assert_eq!(sum.col, "    /* 𝒳 */ public int total() { return ".encode_utf16().count() as u32);
    }

    #[test]
    #[ignore]
    fn java_extraction_limits_and_cancel() {
        let Some(g) = fixture::java_or_skip("java_limits") else {
            return;
        };
        let big = vec![b'x'; MAX_BYTES + 1];
        assert!(matches!(extract(&big, &g), Err(ExtractError::TooLarge(_))));
        let many = "\n".repeat(MAX_LINES + 1);
        assert!(matches!(extract(many.as_bytes(), &g), Err(ExtractError::TooManyLines(_))));
        // tree-sitter polls the progress callback every ~100 parse operations,
        // so the cancel needs a file big enough to be polled at all.
        let mut src = String::from("class Big {
");
        for i in 0..2000 {
            src.push_str(&format!("  int m{i}(int a) {{ return a + {i}; }}
"));
        }
        src.push_str("}
");
        let cancel = AtomicBool::new(true);
        let mut p = Parser::new();
        let r = extract_with(src.as_bytes(), &g, &mut p, Some(&cancel));
        assert!(matches!(r, Err(ExtractError::Cancelled)), "{r:?}");
        // the parser is reusable afterwards
        assert!(extract_with(fixture::JAVA_SRC.as_bytes(), &g, &mut p, None).is_ok());
    }
}
