//! In-memory symbol index, one per root (M2). Names, kinds, signatures and
//! directories are interned; a definition is a ~40-byte record keyed by name
//! id; references are a name → files posting list (the per-file blob in
//! `symbols.db` carries the positions). No syntax trees are retained.
//!
//! Every lookup is a hash probe plus a short sort: `nav_definition` ranks the
//! candidates for a name by proximity to the asking file (same file > same
//! dir > import/package match > kind compatibility > shorter path) and
//! `symbols_search` ranks names by exact > prefix > substring > subsequence.

use std::collections::HashMap;

use super::tags::FileSymbols;

/// Hard cap per root; beyond it the root flips to `state:"error"` rather
/// than eating memory without bound.
pub const MAX_DEFS: usize = 2_000_000;

#[derive(Default)]
pub struct Interner {
    map: HashMap<String, u32>,
    items: Vec<String>,
}

impl Interner {
    pub fn intern(&mut self, s: &str) -> u32 {
        if let Some(&id) = self.map.get(s) {
            return id;
        }
        let id = self.items.len() as u32;
        self.items.push(s.to_string());
        self.map.insert(s.to_string(), id);
        id
    }

    pub fn get(&self, s: &str) -> Option<u32> {
        self.map.get(s).copied()
    }

    pub fn resolve(&self, id: u32) -> &str {
        self.items.get(id as usize).map(String::as_str).unwrap_or("")
    }
}

/// One definition record. Positions are 0-based line / UTF-16 column.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct DefRec {
    pub file: u32,
    /// Interned kind string (`strs`).
    pub kind: u32,
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub sel_line: u32,
    pub sel_col: u32,
    pub sel_end_line: u32,
    pub sel_end_col: u32,
    /// Interned container NAME (`names`), if nested.
    pub container: Option<u32>,
    /// Interned signature (`strs`).
    pub sig: Option<u32>,
}

#[derive(Debug, Clone, Default)]
pub struct FileMeta {
    pub rel: String,
    pub lang: String,
    pub dir_id: u32,
    pub package: Option<String>,
    pub imports: Vec<String>,
    /// blake3 hex of the indexed content — the blob-cache key.
    pub hash: String,
    pub size: u64,
    pub mtime: i64,
    /// Name ids this file contributed defs / refs for (removal bookkeeping).
    def_names: Vec<u32>,
    ref_names: Vec<u32>,
    alive: bool,
}

#[derive(Debug)]
pub struct IndexFull;

impl std::fmt::Display for IndexFull {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "symbol index exceeds {MAX_DEFS} definitions")
    }
}

/// Where a lookup comes from — drives definition ranking.
#[derive(Debug, Clone, Default)]
pub struct LookupCtx<'a> {
    /// Root-relative path of the asking file (forward slashes).
    pub file: &'a str,
    pub package: Option<&'a str>,
    pub imports: &'a [String],
    /// `reference.<kind>` at the cursor when known (`call`, `class`, …).
    pub ref_kind: Option<&'a str>,
}

/// One search / definition hit, resolved to strings.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Hit {
    pub name: String,
    pub kind: String,
    pub path: String,
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub sel_line: u32,
    pub sel_col: u32,
    pub sel_end_line: u32,
    pub sel_end_col: u32,
    pub container: Option<String>,
    pub signature: Option<String>,
    /// Lower is better (search rank tier).
    pub rank: u8,
}

#[derive(Default)]
pub struct SymbolIndex {
    names: Interner,
    strs: Interner,
    dirs: Interner,
    defs: HashMap<u32, Vec<DefRec>>,
    refs_posting: HashMap<u32, Vec<u32>>,
    files: Vec<FileMeta>,
    by_rel: HashMap<String, u32>,
    free: Vec<u32>,
    def_count: usize,
}

pub fn dir_of(rel: &str) -> &str {
    rel.rsplit_once('/').map(|(d, _)| d).unwrap_or("")
}

fn stem_of(rel: &str) -> &str {
    let base = rel.rsplit('/').next().unwrap_or(rel);
    base.rsplit_once('.').map(|(s, _)| s).unwrap_or(base)
}

/// `reference.<ref_kind>` → acceptable definition kinds. Unknown reference
/// kinds accept everything.
pub fn kind_compatible(ref_kind: &str, def_kind: &str) -> bool {
    match ref_kind {
        "call" => matches!(def_kind, "function" | "method" | "constructor" | "macro" | "class"),
        "class" | "type" | "implementation" => matches!(
            def_kind,
            "class" | "interface" | "enum" | "struct" | "trait" | "type" | "module" | "impl"
        ),
        "field" => matches!(def_kind, "field" | "property" | "variable" | "constant"),
        _ => true,
    }
}

/// Does the asking file import (or share a package with) the file holding
/// `name`? Java-style dotted packages and path-style specifiers both count.
pub fn import_match(ctx: &LookupCtx<'_>, meta: &FileMeta, name: &str) -> bool {
    if let Some(p) = meta.package.as_deref() {
        if ctx.package == Some(p) {
            return true;
        }
        for i in ctx.imports {
            if i.len() > p.len()
                && i.starts_with(p)
                && matches!(i.as_bytes()[p.len()], b'.' | b':')
            {
                let tail = i[p.len()..].trim_start_matches(['.', ':']);
                if tail == name || tail == "*" || tail.ends_with(&format!(".{name}")) {
                    return true;
                }
            }
        }
    }
    let stem = stem_of(&meta.rel);
    ctx.imports.iter().any(|i| {
        let last = i.trim_end_matches('/').rsplit(['/', ':']).next().unwrap_or(i);
        let last = last.rsplit_once('.').map(|(s, _)| s).unwrap_or(last);
        !last.is_empty() && (last == stem || last == name)
    })
}

/// (tier, !kind-compatible, path length, line) — the rank key of one hit.
type Scored<'a> = ((u8, bool, usize, u32), &'a DefRec);

impl SymbolIndex {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn def_count(&self) -> usize {
        self.def_count
    }

    pub fn file_count(&self) -> usize {
        self.files.iter().filter(|f| f.alive).count()
    }

    pub fn file_id(&self, rel: &str) -> Option<u32> {
        self.by_rel.get(rel).copied()
    }

    pub fn file(&self, id: u32) -> Option<&FileMeta> {
        self.files.get(id as usize).filter(|f| f.alive)
    }

    pub fn file_by_rel(&self, rel: &str) -> Option<&FileMeta> {
        self.file_id(rel).and_then(|id| self.file(id))
    }

    /// Replace (or add) one file's symbols.
    pub fn update_file(
        &mut self,
        rel: &str,
        lang: &str,
        hash: &str,
        size: u64,
        mtime: i64,
        syms: &FileSymbols,
    ) -> Result<(), IndexFull> {
        self.remove_file(rel);
        if self.def_count + syms.defs.len() > MAX_DEFS {
            return Err(IndexFull);
        }
        let id = match self.free.pop() {
            Some(id) => id,
            None => {
                self.files.push(FileMeta::default());
                (self.files.len() - 1) as u32
            }
        };
        let dir_id = self.dirs.intern(dir_of(rel));
        let mut def_names = Vec::with_capacity(syms.defs.len());
        for d in &syms.defs {
            let name = self.names.intern(&d.name);
            let kind = self.strs.intern(&d.kind);
            let container = d
                .container
                .and_then(|c| syms.defs.get(c as usize))
                .map(|c| self.names.intern(&c.name));
            let sig = d.signature.as_deref().map(|s| self.strs.intern(s));
            self.defs.entry(name).or_default().push(DefRec {
                file: id,
                kind,
                line: d.line,
                col: d.col,
                end_line: d.end_line,
                end_col: d.end_col,
                sel_line: d.sel_line,
                sel_col: d.sel_col,
                sel_end_line: d.sel_end_line,
                sel_end_col: d.sel_end_col,
                container,
                sig,
            });
            def_names.push(name);
        }
        def_names.sort_unstable();
        def_names.dedup();
        let mut ref_names: Vec<u32> = syms.refs.iter().map(|r| self.names.intern(&r.name)).collect();
        ref_names.sort_unstable();
        ref_names.dedup();
        for &n in &ref_names {
            self.refs_posting.entry(n).or_default().push(id);
        }
        self.def_count += syms.defs.len();
        self.files[id as usize] = FileMeta {
            rel: rel.to_string(),
            lang: lang.to_string(),
            dir_id,
            package: syms.package.clone(),
            imports: syms.imports.clone(),
            hash: hash.to_string(),
            size,
            mtime,
            def_names,
            ref_names,
            alive: true,
        };
        self.by_rel.insert(rel.to_string(), id);
        Ok(())
    }

    pub fn remove_file(&mut self, rel: &str) -> bool {
        let Some(id) = self.by_rel.remove(rel) else {
            return false;
        };
        let meta = std::mem::take(&mut self.files[id as usize]);
        for n in &meta.def_names {
            if let Some(v) = self.defs.get_mut(n) {
                let before = v.len();
                v.retain(|d| d.file != id);
                self.def_count -= before - v.len();
                if v.is_empty() {
                    self.defs.remove(n);
                }
            }
        }
        for n in &meta.ref_names {
            if let Some(v) = self.refs_posting.get_mut(n) {
                v.retain(|f| *f != id);
                if v.is_empty() {
                    self.refs_posting.remove(n);
                }
            }
        }
        self.free.push(id);
        true
    }

    pub fn clear(&mut self) {
        *self = Self::default();
    }

    /// Raw definition records for an exact name.
    pub fn definitions(&self, name: &str) -> &[DefRec] {
        self.names
            .get(name)
            .and_then(|n| self.defs.get(&n))
            .map(Vec::as_slice)
            .unwrap_or(&[])
    }

    /// Files whose references include `name`.
    pub fn files_referencing(&self, name: &str) -> Vec<u32> {
        self.names
            .get(name)
            .and_then(|n| self.refs_posting.get(&n))
            .cloned()
            .unwrap_or_default()
    }

    fn hit(&self, name: &str, d: &DefRec, rank: u8) -> Hit {
        let meta = &self.files[d.file as usize];
        Hit {
            name: name.to_string(),
            kind: self.strs.resolve(d.kind).to_string(),
            path: meta.rel.clone(),
            line: d.line,
            col: d.col,
            end_line: d.end_line,
            end_col: d.end_col,
            sel_line: d.sel_line,
            sel_col: d.sel_col,
            sel_end_line: d.sel_end_line,
            sel_end_col: d.sel_end_col,
            container: d.container.map(|c| self.names.resolve(c).to_string()),
            signature: d.sig.map(|s| self.strs.resolve(s).to_string()),
            rank,
        }
    }

    /// Definitions of `name` ranked for `ctx`: same file (0), then same dir
    /// (1), then import/package match (2), then the rest (3); within a tier
    /// kind-compatible first, then shorter path, then line.
    pub fn rank_definitions(&self, name: &str, ctx: &LookupCtx<'_>) -> Vec<Hit> {
        let cands = self.definitions(name);
        if cands.is_empty() {
            return Vec::new();
        }
        let cur_dir = self.dirs.get(dir_of(ctx.file));
        let mut scored: Vec<Scored<'_>> = cands
            .iter()
            .map(|d| {
                let meta = &self.files[d.file as usize];
                let tier = if meta.rel == ctx.file {
                    0
                } else if cur_dir == Some(meta.dir_id) {
                    1
                } else if import_match(ctx, meta, name) {
                    2
                } else {
                    3
                };
                let kind_ok = ctx
                    .ref_kind
                    .map(|rk| kind_compatible(rk, self.strs.resolve(d.kind)))
                    .unwrap_or(true);
                ((tier, !kind_ok, meta.rel.len(), d.line), d)
            })
            .collect();
        scored.sort_by_key(|a| a.0);
        scored
            .into_iter()
            .map(|(k, d)| self.hit(name, d, k.0))
            .collect()
    }

    /// Search names: exact (0) > prefix (1) > substring (2) > subsequence (3),
    /// case-insensitive; ties by name length then path. `limit` caps hits.
    pub fn search(&self, query: &str, limit: usize) -> Vec<Hit> {
        let q = query.trim().to_lowercase();
        if q.is_empty() || limit == 0 {
            return Vec::new();
        }
        let mut ranked: Vec<(u8, usize, u32)> = Vec::new();
        for (id, name) in self.names.items.iter().enumerate() {
            let id = id as u32;
            if !self.defs.contains_key(&id) {
                continue; // reference-only names have nothing to jump to
            }
            let lower = name.to_lowercase();
            let rank = if lower == q {
                0
            } else if lower.starts_with(&q) {
                1
            } else if lower.contains(&q) {
                2
            } else if is_subsequence(&q, &lower) {
                3
            } else {
                continue;
            };
            ranked.push((rank, name.len(), id));
        }
        ranked.sort_unstable();
        let mut out = Vec::new();
        'outer: for (rank, _, id) in ranked {
            let name = self.names.resolve(id);
            let mut defs: Vec<&DefRec> = self.defs[&id].iter().collect();
            defs.sort_by(|a, b| {
                self.files[a.file as usize]
                    .rel
                    .cmp(&self.files[b.file as usize].rel)
                    .then(a.line.cmp(&b.line))
            });
            for d in defs {
                out.push(self.hit(name, d, rank));
                if out.len() >= limit {
                    break 'outer;
                }
            }
        }
        out
    }
}

fn is_subsequence(needle: &str, hay: &str) -> bool {
    let mut it = hay.chars();
    needle.chars().all(|c| it.any(|h| h == c))
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;
    use crate::symbols::tags::{Def, FileSymbols, Ref};

    pub fn def(name: &str, kind: &str, line: u32) -> Def {
        Def {
            name: name.into(),
            kind: kind.into(),
            line,
            col: 0,
            end_line: line + 2,
            end_col: 1,
            sel_line: line,
            sel_col: 6,
            sel_end_line: line,
            sel_end_col: 6 + name.len() as u32,
            container: None,
            signature: Some(format!("{kind} {name}")),
        }
    }

    pub fn reff(name: &str, kind: &str, line: u32) -> Ref {
        Ref {
            name: name.into(),
            kind: kind.into(),
            line,
            col: 4,
            end_line: line,
            end_col: 4 + name.len() as u32,
        }
    }

    pub fn syms(package: Option<&str>, imports: &[&str], defs: Vec<Def>, refs: Vec<Ref>) -> FileSymbols {
        FileSymbols {
            defs,
            refs,
            imports: imports.iter().map(|s| s.to_string()).collect(),
            package: package.map(str::to_string),
        }
    }

    fn add(ix: &mut SymbolIndex, rel: &str, s: &FileSymbols) {
        ix.update_file(rel, "java", &format!("h-{rel}"), 1, 1, s).unwrap();
    }

    /// Four `Total` defs: same file, same dir, imported package, elsewhere.
    fn ranking_fixture() -> SymbolIndex {
        let mut ix = SymbolIndex::new();
        add(
            &mut ix,
            "src/a/Cart.java",
            &syms(
                Some("a"),
                &["b.Total", "c.*"],
                vec![def("Cart", "class", 0), def("Total", "method", 10)],
                vec![reff("Total", "call", 3)],
            ),
        );
        add(&mut ix, "src/a/Other.java", &syms(Some("a"), &[], vec![def("Total", "class", 0)], vec![]));
        add(&mut ix, "src/b/Total.java", &syms(Some("b"), &[], vec![def("Total", "class", 0)], vec![]));
        add(&mut ix, "src/c/Tot.java", &syms(Some("c"), &[], vec![def("Total", "class", 0)], vec![]));
        add(&mut ix, "lib/z/Zed.java", &syms(Some("z"), &[], vec![def("Total", "class", 0)], vec![reff("Total", "class", 1)]));
        ix
    }

    #[test]
    fn ranking_same_file_dir_import_then_rest() {
        let ix = ranking_fixture();
        let cur = ix.file_by_rel("src/a/Cart.java").unwrap().clone();
        let ctx = LookupCtx {
            file: "src/a/Cart.java",
            package: cur.package.as_deref(),
            imports: &cur.imports,
            ref_kind: None,
        };
        let hits = ix.rank_definitions("Total", &ctx);
        let paths: Vec<&str> = hits.iter().map(|h| h.path.as_str()).collect();
        assert_eq!(
            paths,
            vec![
                "src/a/Cart.java",
                "src/a/Other.java",
                "src/c/Tot.java",   // wildcard import c.* — same tier, shorter path
                "src/b/Total.java", // explicit import b.Total
                "lib/z/Zed.java",
            ]
        );
        assert_eq!(hits[0].rank, 0);
        assert_eq!(hits[1].rank, 1);
        assert_eq!(hits[2].rank, 2);
        assert_eq!(hits[3].rank, 2);
        assert_eq!(hits[4].rank, 3);
        assert_eq!(hits[0].kind, "method");
        assert_eq!(hits[0].signature.as_deref(), Some("method Total"));
        // kind compatibility reorders WITHIN a tier only: a `class`
        // reference from an unrelated file prefers class defs, but the
        // same-file method still wins the same-file tier.
        let ctx2 = LookupCtx {
            file: "docs/README.java",
            package: None,
            imports: &[],
            ref_kind: Some("class"),
        };
        let hits = ix.rank_definitions("Total", &ctx2);
        assert!(hits.iter().take(4).all(|h| h.kind == "class"), "{hits:?}");
        assert_eq!(hits.last().unwrap().kind, "method");
        // shorter path breaks ties inside a tier
        let lens: Vec<usize> = hits.iter().take(4).map(|h| h.path.len()).collect();
        assert!(lens.windows(2).all(|w| w[0] <= w[1]), "{lens:?}");
        assert_eq!(hits[0].path.len(), "src/c/Tot.java".len());
        assert!(ix.rank_definitions("Nope", &ctx).is_empty());
    }

    #[test]
    fn import_match_rules() {
        let meta = FileMeta {
            rel: "src/util/helpers.ts".into(),
            package: None,
            ..Default::default()
        };
        let imports = vec!["./util/helpers".to_string(), "@scope/pkg".to_string()];
        let ctx = LookupCtx {
            file: "src/app.ts",
            package: None,
            imports: &imports,
            ref_kind: None,
        };
        assert!(import_match(&ctx, &meta, "fmt"));
        let other = FileMeta {
            rel: "src/util/other.ts".into(),
            ..Default::default()
        };
        assert!(!import_match(&ctx, &other, "fmt"));
        let java = FileMeta {
            rel: "x/Y.java".into(),
            package: Some("com.acme".into()),
            ..Default::default()
        };
        let imports = vec!["com.acme.Y".to_string()];
        let ctx = LookupCtx {
            file: "z/Z.java",
            package: Some("other"),
            imports: &imports,
            ref_kind: None,
        };
        assert!(import_match(&ctx, &java, "Y"));
        assert!(!import_match(&ctx, &java, "Q"));
        let same_pkg = LookupCtx {
            file: "z/Z.java",
            package: Some("com.acme"),
            imports: &[],
            ref_kind: None,
        };
        assert!(import_match(&same_pkg, &java, "Q"));
    }

    #[test]
    fn references_posting_and_incremental_update_remove() {
        let mut ix = ranking_fixture();
        let mut files = ix.files_referencing("Total");
        files.sort();
        let rels: Vec<&str> = files.iter().map(|f| ix.file(*f).unwrap().rel.as_str()).collect();
        assert_eq!(rels, vec!["src/a/Cart.java", "lib/z/Zed.java"]);
        assert_eq!(ix.def_count(), 6);
        assert_eq!(ix.file_count(), 5);
        // update: Cart.java loses its Total method + reference, gains Foo
        add(
            &mut ix,
            "src/a/Cart.java",
            &syms(Some("a"), &[], vec![def("Cart", "class", 0), def("Foo", "function", 5)], vec![]),
        );
        assert_eq!(ix.def_count(), 6);
        assert_eq!(ix.file_count(), 5);
        assert_eq!(ix.definitions("Total").len(), 4);
        assert_eq!(ix.files_referencing("Total").len(), 1);
        assert_eq!(ix.definitions("Foo").len(), 1);
        // remove
        assert!(ix.remove_file("lib/z/Zed.java"));
        assert!(!ix.remove_file("lib/z/Zed.java"));
        assert_eq!(ix.definitions("Total").len(), 3);
        assert!(ix.files_referencing("Total").is_empty());
        assert_eq!(ix.file_count(), 4);
        assert!(ix.file_by_rel("lib/z/Zed.java").is_none());
        // the freed slot is reused
        add(&mut ix, "new/N.java", &syms(None, &[], vec![def("N", "class", 0)], vec![]));
        assert_eq!(ix.files.len(), 5);
        assert_eq!(ix.file_by_rel("new/N.java").unwrap().hash, "h-new/N.java");
    }

    #[test]
    fn search_ranks_exact_prefix_substring_subsequence() {
        let mut ix = SymbolIndex::new();
        add(
            &mut ix,
            "a.java",
            &syms(
                None,
                &[],
                vec![
                    def("getUserName", "method", 1),
                    def("user", "field", 2),
                    def("UserService", "class", 3),
                    def("gun", "method", 4),
                    def("zzz", "method", 5),
                ],
                vec![reff("onlyRef", "call", 9)],
            ),
        );
        let names: Vec<String> = ix.search("user", 10).into_iter().map(|h| h.name).collect();
        assert_eq!(names, vec!["user", "UserService", "getUserName"]);
        let names: Vec<String> = ix.search("gun", 10).into_iter().map(|h| h.name).collect();
        assert_eq!(names, vec!["gun", "getUserName"]);
        assert!(ix.search("onlyRef", 10).is_empty(), "ref-only names are not hits");
        assert_eq!(ix.search("user", 2).len(), 2);
        assert!(ix.search("   ", 10).is_empty());
        let h = &ix.search("UserService", 1)[0];
        assert_eq!((h.rank, h.kind.as_str(), h.path.as_str(), h.line), (0, "class", "a.java", 3));
    }

    #[test]
    fn container_names_resolve_through_the_interner() {
        let mut ix = SymbolIndex::new();
        let mut inner = def("run", "method", 2);
        inner.container = Some(0);
        add(&mut ix, "a.java", &syms(None, &[], vec![def("Outer", "class", 0), inner], vec![]));
        let hit = &ix.search("run", 1)[0];
        assert_eq!(hit.container.as_deref(), Some("Outer"));
    }

    #[test]
    fn cap_rejects_past_max_defs() {
        let mut ix = SymbolIndex::new();
        ix.def_count = MAX_DEFS - 1;
        let two = syms(None, &[], vec![def("a", "f", 0), def("b", "f", 1)], vec![]);
        assert!(ix.update_file("x", "java", "h", 0, 0, &two).is_err());
        let one = syms(None, &[], vec![def("a", "f", 0)], vec![]);
        assert!(ix.update_file("x", "java", "h", 0, 0, &one).is_ok());
    }
}
