//! `symbols_*` + `nav_*` commands (M2, index source only). `id` is an agent
//! uuid OR a project id (the project pseudo-agent record, exactly like
//! `agents/fs_commands.rs`) → its worktree root; `path` is root-relative with
//! forward slashes and goes through `safe_join`. Lines/columns are 0-based
//! UTF-16 in and out. The `nav_*` commands carry the M3 seam: `lsp_first`
//! answers first once a language server is wired, the index is the floor.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use serde::{Deserialize, Serialize};
use tauri::State;
use uuid::Uuid;

use crate::agents::fs_commands::safe_join;
use crate::agents::service::AgentService;
use crate::core::errors::{AppError, AppResult};
use crate::libsrc::resolve::LibHit;
use crate::libsrc::LibSrcService;
use crate::lsp::{self, LspService};
use crate::projects::service::ProjectService;

use super::index::{Hit, LookupCtx};
use super::tags::{self, FileSymbols};
use super::{norm_rel, IndexStatus, RootState, SymbolService};

/// Definitions returned per lookup at most.
const MAX_LOCATIONS: usize = 200;
/// Reference lookup caps: files consulted, locations returned.
const MAX_REF_FILES: usize = 400;
const MAX_REFERENCES: usize = 2000;
const DEFAULT_SEARCH_LIMIT: usize = 50;

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DocSymbol {
    pub name: String,
    pub kind: String,
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
    pub end_col: u32,
    pub sel_line: u32,
    pub sel_col: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    pub children: Vec<DocSymbol>,
}

#[derive(Debug, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolScope {
    /// `worktree | project | all` — same semantics as find-in-files.
    pub kind: String,
    pub agent_id: Option<Uuid>,
    pub project_id: Option<Uuid>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SymbolHit {
    pub name: String,
    pub kind: String,
    pub path: String,
    pub line: u32,
    pub col: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    /// Owning agent; `null` = the project checkout.
    pub agent_id: Option<Uuid>,
    /// Root label (agent name); `null` = the project checkout.
    pub root: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NavLocation {
    /// `orrery://<id>/<path>` — the Monaco model uri; a raw server uri
    /// (`jdt://…`) for a virtual document, with `id`/`path` null.
    pub uri: String,
    pub id: Option<Uuid>,
    pub path: Option<String>,
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
    pub end_col: u32,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub kind: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub container: Option<String>,
    /// Signature line of a definition.
    #[serde(skip_serializing_if = "Option::is_none")]
    pub preview: Option<String>,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct NavResult {
    /// `"index"` here; `"lsp"` once M3 answers first.
    pub source: String,
    pub locations: Vec<NavLocation>,
    /// `starting | timeout | unavailable` from the LSP router; `null` for the index.
    pub lsp_state: Option<String>,
    /// Why an EMPTY definition answer may be empty, when the library index
    /// can tell (M4): `jdk-missing` (no JDK on this machine) or
    /// `sources-missing` (the pom names jars without a `-sources.jar`).
    pub lib_hint: Option<String>,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NavRange {
    pub line: u32,
    pub col: u32,
    pub end_line: u32,
    pub end_col: u32,
}

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct NavHover {
    pub source: String,
    /// Markdown.
    pub contents: String,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub range: Option<NavRange>,
}

// ---- M3 seam ----------------------------------------------------------------

/// What the LSP router is asked before the index answers.
pub enum NavKind {
    Definition,
    References { include_declaration: bool },
    Hover,
}

pub enum LspAnswer {
    Nav(NavResult),
    Hover(Option<NavHover>),
    /// The index answers, tagged with what the server was doing
    /// (`starting | timeout | unavailable`, or `None` for an empty reply).
    Fallback(Option<String>),
}

/// LSP-first router. `Some(Nav | Hover)` when a language server for
/// `(language of `rel`, owning project)` answered within budget; `Fallback`
/// when one exists but could not answer now; `None` when no installed +
/// enabled server pack claims the file (the index is the only source).
#[allow(clippy::too_many_arguments)]
fn lsp_first(
    lsp: &LspService,
    agents: &AgentService,
    projects: &ProjectService,
    id: Uuid,
    kind: &NavKind,
    worktree: &Path,
    rel: &str,
    line: u32,
    col: u32,
    text: Option<&str>,
) -> Option<LspAnswer> {
    let lang = lsp::language_of(rel)?;
    let Ok((_, project)) = lsp::commands::resolve_project(agents, projects, id) else {
        return None;
    };
    let name = project.name.clone();
    let handle = match lsp.acquire(lang.id, project.id, &project.root, &|| name.clone()) {
        lsp::Acquire::NotInstalled => return None,
        lsp::Acquire::Starting(_) => return Some(LspAnswer::Fallback(Some("starting".into()))),
        lsp::Acquire::Unavailable => return Some(LspAnswer::Fallback(Some("unavailable".into()))),
        lsp::Acquire::Ready(h) => h,
    };
    let abs = worktree.join(rel);
    // roots for `file://` → `orrery://`: the project checkout + its worktrees
    let mut roots: Vec<(Uuid, PathBuf)> = vec![(project.id, project.root.clone())];
    if let Ok(list) = agents.list() {
        roots.extend(
            list.into_iter()
                .filter(|a| a.project_id == project.id && !a.worktree.is_empty())
                .map(|a| (a.id, PathBuf::from(a.worktree))),
        );
    }
    let ctx = lsp::NavCtx {
        abs: &abs,
        line,
        col,
        text,
        language_id: lang.lsp_id,
        roots: &roots,
    };
    Some(match lsp::route(&handle, kind, &ctx) {
        lsp::Routed::Locations(locations) => LspAnswer::Nav(NavResult {
            source: "lsp".into(),
            locations,
            lsp_state: None,
            lib_hint: None,
        }),
        lsp::Routed::Hover(h) => LspAnswer::Hover(h),
        lsp::Routed::Empty => LspAnswer::Fallback(None),
        lsp::Routed::Timeout => LspAnswer::Fallback(Some("timeout".into())),
        lsp::Routed::Failed => LspAnswer::Fallback(Some("unavailable".into())),
    })
}

// ---- helpers ------------------------------------------------------------------

fn root_of(agents: &AgentService, id: Uuid) -> AppResult<PathBuf> {
    Ok(PathBuf::from(agents.get(id)?.worktree))
}

/// `(worktree root, owning project id)` of an agent / project id — the
/// library sources (M4) are the PROJECT's.
fn root_and_project(agents: &AgentService, id: Uuid) -> AppResult<(PathBuf, String)> {
    let a = agents.get(id)?;
    Ok((PathBuf::from(a.worktree), a.project_id.to_string()))
}

/// `(absolute, normalized root-relative)` or an error for an escaping path.
fn resolve(root: &Path, path: &str) -> AppResult<(PathBuf, String)> {
    let rel = norm_rel(path);
    let abs = safe_join(root, &rel)?;
    Ok((abs, rel))
}

fn index_result(locations: Vec<NavLocation>) -> NavResult {
    NavResult {
        source: "index".into(),
        locations,
        lsp_state: None,
        lib_hint: None,
    }
}

fn uri_of(id: Uuid, rel: &str) -> String {
    format!("orrery://{id}/{rel}")
}

fn def_location(id: Uuid, h: &Hit) -> NavLocation {
    NavLocation {
        uri: uri_of(id, &h.path),
        id: Some(id),
        path: Some(h.path.clone()),
        line: h.sel_line,
        col: h.sel_col,
        end_line: h.sel_end_line,
        end_col: h.sel_end_col,
        kind: Some(h.kind.clone()),
        container: h.container.clone(),
        preview: h.signature.clone(),
    }
}

/// A library declaration as a nav location: raw `orrery-lib://` uri, no
/// owning id/path, the fq name as the preview line.
fn lib_location(h: &LibHit) -> NavLocation {
    NavLocation {
        uri: h.uri.clone(),
        id: None,
        path: None,
        line: h.line,
        col: h.col,
        end_line: h.line,
        end_col: h.col,
        kind: Some(h.kind.clone()),
        container: h.container.clone(),
        preview: Some(h.fq_name.clone()),
    }
}

/// Library hits join the answer when the project index has nothing, or when
/// the word is capitalised (a type — `List` in a project is rarely the
/// `java.util.List` the cursor meant, so both are offered).
fn wants_library(project_hits: usize, word: &str) -> bool {
    project_hits == 0 || word.chars().next().is_some_and(char::is_uppercase)
}

/// Hierarchy from the flat `container` indices.
pub fn doc_symbols(fs: &FileSymbols) -> Vec<DocSymbol> {
    let n = fs.defs.len();
    let mut children: Vec<Vec<usize>> = vec![Vec::new(); n];
    let mut roots = Vec::new();
    for (i, d) in fs.defs.iter().enumerate() {
        match d.container {
            Some(c) if (c as usize) < n && (c as usize) != i => children[c as usize].push(i),
            _ => roots.push(i),
        }
    }
    fn build(fs: &FileSymbols, children: &[Vec<usize>], i: usize, depth: usize) -> DocSymbol {
        let d = &fs.defs[i];
        DocSymbol {
            name: d.name.clone(),
            kind: d.kind.clone(),
            line: d.line,
            col: d.col,
            end_line: d.end_line,
            end_col: d.end_col,
            sel_line: d.sel_line,
            sel_col: d.sel_col,
            container: d
                .container
                .and_then(|c| fs.defs.get(c as usize))
                .map(|c| c.name.clone()),
            children: if depth < 64 {
                children[i]
                    .iter()
                    .map(|&c| build(fs, children, c, depth + 1))
                    .collect()
            } else {
                Vec::new()
            },
        }
    }
    roots.into_iter().map(|i| build(fs, &children, i, 0)).collect()
}

/// The `reference.<kind>` under the cursor, when the file's symbols say so.
fn ref_kind_at(fs: &FileSymbols, line: u32, col: u32, word: &str) -> Option<String> {
    fs.refs
        .iter()
        .find(|r| r.name == word && r.line == line && r.col <= col && col <= r.end_col)
        .map(|r| r.kind.clone())
}

/// The name range under the cursor (reference or definition) for hover.
fn range_at(fs: &FileSymbols, line: u32, col: u32, word: &str) -> Option<NavRange> {
    if let Some(r) = fs
        .refs
        .iter()
        .find(|r| r.name == word && r.line == line && r.col <= col && col <= r.end_col)
    {
        return Some(NavRange {
            line: r.line,
            col: r.col,
            end_line: r.end_line,
            end_col: r.end_col,
        });
    }
    fs.defs
        .iter()
        .find(|d| d.name == word && d.sel_line == line && d.sel_col <= col && col <= d.sel_end_col)
        .map(|d| NavRange {
            line: d.sel_line,
            col: d.sel_col,
            end_line: d.sel_end_line,
            end_col: d.sel_end_col,
        })
}

/// Symbols of the asking file: the live buffer when `text` is given (ad hoc
/// parse, never cached), else the indexed blob.
fn current_symbols(
    symbols: &SymbolService,
    st: &RootState,
    rel: &str,
    text: Option<&str>,
) -> Option<Arc<FileSymbols>> {
    match text {
        Some(t) => {
            let g = symbols.grammar_for_path(rel)?;
            tags::extract(t.as_bytes(), &g).ok().map(Arc::new)
        }
        None => symbols.file_symbols(st, rel),
    }
}

/// Ranked definitions of `word` for a lookup from `rel`; with a live buffer
/// the same-file candidates come from it instead of the (stale) index.
fn ranked_definitions(
    st: &RootState,
    rel: &str,
    word: &str,
    cur: Option<&FileSymbols>,
    live: bool,
    ref_kind: Option<&str>,
) -> Vec<Hit> {
    let (package, imports) = match cur {
        Some(c) => (c.package.clone(), c.imports.clone()),
        None => {
            let ix = st.index.read().unwrap();
            ix.file_by_rel(rel)
                .map(|m| (m.package.clone(), m.imports.clone()))
                .unwrap_or_default()
        }
    };
    let ctx = LookupCtx {
        file: rel,
        package: package.as_deref(),
        imports: &imports,
        ref_kind,
    };
    let mut hits = st.index.read().unwrap().rank_definitions(word, &ctx);
    if live {
        if let Some(c) = cur {
            hits.retain(|h| h.path != rel);
            let mut same: Vec<Hit> = c
                .defs
                .iter()
                .filter(|d| d.name == word)
                .map(|d| Hit {
                    name: d.name.clone(),
                    kind: d.kind.clone(),
                    path: rel.to_string(),
                    line: d.line,
                    col: d.col,
                    end_line: d.end_line,
                    end_col: d.end_col,
                    sel_line: d.sel_line,
                    sel_col: d.sel_col,
                    sel_end_line: d.sel_end_line,
                    sel_end_col: d.sel_end_col,
                    container: d
                        .container
                        .and_then(|ci| c.defs.get(ci as usize))
                        .map(|p| p.name.clone()),
                    signature: d.signature.clone(),
                    rank: 0,
                })
                .collect();
            same.extend(hits);
            hits = same;
        }
    }
    hits
}

// ---- index lifecycle ----------------------------------------------------------

#[tauri::command(async)]
pub fn symbols_index_status(
    symbols: State<'_, SymbolService>,
    agents: State<'_, AgentService>,
    id: Uuid,
) -> AppResult<IndexStatus> {
    crate::perf::timed("symbols_index_status", || {
        let root = root_of(&agents, id)?;
        Ok(symbols.status(&root))
    })
}

/// Start (or restart) indexing; `force` re-hashes every file. Also kicks
/// the library sources the owning project needs (its `Cargo.lock` crates,
/// its `pom.xml` jars + the JDK) — a no-op once they are indexed.
#[tauri::command(async)]
pub fn symbols_index_start(
    symbols: State<'_, SymbolService>,
    agents: State<'_, AgentService>,
    libsrc: State<'_, LibSrcService>,
    id: Uuid,
    force: Option<bool>,
) -> AppResult<IndexStatus> {
    crate::perf::timed("symbols_index_start", || {
        let (root, project) = root_and_project(&agents, id)?;
        symbols.start(&root, force.unwrap_or(false));
        libsrc.ensure_for_project(&project);
        Ok(symbols.status(&root))
    })
}

#[tauri::command(async)]
pub fn symbols_index_stop(
    symbols: State<'_, SymbolService>,
    agents: State<'_, AgentService>,
    id: Uuid,
) -> AppResult<IndexStatus> {
    crate::perf::timed("symbols_index_stop", || {
        let root = root_of(&agents, id)?;
        symbols.stop(&root);
        Ok(symbols.status(&root))
    })
}

// ---- document symbols ---------------------------------------------------------

fn document_symbols(
    symbols: &SymbolService,
    agents: &AgentService,
    id: Uuid,
    path: &str,
    text: Option<String>,
) -> AppResult<Vec<DocSymbol>> {
    let root = root_of(agents, id)?;
    let (abs, rel) = resolve(&root, path)?;
    let Some(g) = symbols.grammar_for_path(&rel) else {
        return Ok(Vec::new()); // no grammar for this file type
    };
    let bytes = match text {
        Some(t) => t.into_bytes(),
        None => std::fs::read(&abs).map_err(|e| AppError::Other(format!("read '{rel}': {e}")))?,
    };
    match tags::extract(&bytes, &g) {
        Ok(fs) => Ok(doc_symbols(&fs)),
        Err(e) => {
            log::debug!("symbols_document {rel}: {e}");
            Ok(Vec::new())
        }
    }
}

/// Outline of one file (parsed ad hoc from `text` when given, else from
/// disk — never cached, never touches the index).
#[tauri::command(async)]
pub fn symbols_document(
    symbols: State<'_, SymbolService>,
    agents: State<'_, AgentService>,
    id: Uuid,
    path: String,
    text: Option<String>,
) -> AppResult<Vec<DocSymbol>> {
    crate::perf::timed("symbols_document", || {
        document_symbols(&symbols, &agents, id, &path, text)
    })
}

/// Alias of `symbols_document` under the nav namespace.
#[tauri::command(async)]
pub fn nav_document_symbols(
    symbols: State<'_, SymbolService>,
    agents: State<'_, AgentService>,
    id: Uuid,
    path: String,
    text: Option<String>,
) -> AppResult<Vec<DocSymbol>> {
    crate::perf::timed("nav_document_symbols", || {
        document_symbols(&symbols, &agents, id, &path, text)
    })
}

// ---- search -------------------------------------------------------------------

/// Symbol search across the scope's roots; every root is indexed on first
/// sight and answers with whatever it has so far.
#[tauri::command(async)]
pub fn symbols_search(
    symbols: State<'_, SymbolService>,
    agents: State<'_, AgentService>,
    projects: State<'_, ProjectService>,
    scope: SymbolScope,
    query: String,
    limit: Option<usize>,
) -> AppResult<Vec<SymbolHit>> {
    crate::perf::timed("symbols_search", || {
        let roots = crate::search::commands::resolve_scope(
            &scope.kind,
            scope.agent_id,
            scope.project_id,
            &agents,
            &projects,
        )?;
        let limit = limit.unwrap_or(DEFAULT_SEARCH_LIMIT).clamp(1, 500);
        let query = query.trim();
        if query.is_empty() {
            return Ok(Vec::new());
        }
        let mut all: Vec<(u8, usize, SymbolHit)> = Vec::new();
        for r in roots {
            let st = symbols.ensure_indexed(&r.path);
            let hits = st.index.read().unwrap().search(query, limit);
            for h in hits {
                all.push((
                    h.rank,
                    h.name.len(),
                    SymbolHit {
                        name: h.name,
                        kind: h.kind,
                        path: h.path,
                        line: h.line,
                        col: h.col,
                        container: h.container,
                        agent_id: r.agent_id,
                        root: r.label.clone(),
                    },
                ));
            }
        }
        all.sort_by_key(|a| (a.0, a.1));
        all.truncate(limit);
        Ok(all.into_iter().map(|(_, _, h)| h).collect())
    })
}

// ---- navigation ---------------------------------------------------------------

/// Go to definition of `word` at (`line`, `col`) in `path`.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
pub fn nav_definition(
    symbols: State<'_, SymbolService>,
    agents: State<'_, AgentService>,
    projects: State<'_, ProjectService>,
    lsp: State<'_, LspService>,
    libsrc: State<'_, LibSrcService>,
    id: Uuid,
    path: String,
    line: u32,
    col: u32,
    word: String,
    text: Option<String>,
) -> AppResult<NavResult> {
    crate::perf::timed("nav_definition", || {
        let (root, project) = root_and_project(&agents, id)?;
        let (_, rel) = resolve(&root, &path)?;
        let word = word.trim();
        if word.is_empty() {
            return Ok(index_result(Vec::new()));
        }
        let lsp_state = match lsp_first(
            &lsp,
            &agents,
            &projects,
            id,
            &NavKind::Definition,
            &root,
            &rel,
            line,
            col,
            text.as_deref(),
        ) {
            Some(LspAnswer::Nav(r)) => return Ok(r),
            Some(LspAnswer::Fallback(s)) => s,
            _ => None,
        };
        let st = symbols.ensure_indexed(&root);
        let cur = current_symbols(&symbols, &st, &rel, text.as_deref());
        let ref_kind = cur.as_deref().and_then(|c| ref_kind_at(c, line, col, word));
        let hits = ranked_definitions(&st, &rel, word, cur.as_deref(), text.is_some(), ref_kind.as_deref());
        let mut locations: Vec<NavLocation> = hits
            .iter()
            .take(MAX_LOCATIONS)
            .map(|h| def_location(id, h))
            .collect();
        // library declarations (M4) after the project's own
        if wants_library(locations.len(), word) {
            locations.extend(
                libsrc
                    .hits_for(Some(&project), &root, &rel, word, text.as_deref())
                    .iter()
                    .map(lib_location),
            );
        }
        let mut r = index_result(locations);
        r.lsp_state = lsp_state;
        if r.locations.is_empty() {
            r.lib_hint = libsrc.hint_for(Some(&project), &rel).map(String::from);
        }
        Ok(r)
    })
}

/// Every reference to `word` across the root (files from the posting list,
/// positions from each file's blob), optionally with the definitions first.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
pub fn nav_references(
    symbols: State<'_, SymbolService>,
    agents: State<'_, AgentService>,
    projects: State<'_, ProjectService>,
    lsp: State<'_, LspService>,
    id: Uuid,
    path: String,
    line: u32,
    col: u32,
    word: String,
    include_declaration: Option<bool>,
) -> AppResult<NavResult> {
    crate::perf::timed("nav_references", || {
        let root = root_of(&agents, id)?;
        let (_, rel) = resolve(&root, &path)?;
        let word = word.trim();
        if word.is_empty() {
            return Ok(index_result(Vec::new()));
        }
        let include_declaration = include_declaration.unwrap_or(true);
        let lsp_state = match lsp_first(
            &lsp,
            &agents,
            &projects,
            id,
            &NavKind::References { include_declaration },
            &root,
            &rel,
            line,
            col,
            None,
        ) {
            Some(LspAnswer::Nav(r)) => return Ok(r),
            Some(LspAnswer::Fallback(s)) => s,
            _ => None,
        };
        let st = symbols.ensure_indexed(&root);
        let mut locations = Vec::new();
        if include_declaration {
            let hits = ranked_definitions(&st, &rel, word, None, false, None);
            locations.extend(hits.iter().take(MAX_LOCATIONS).map(|h| def_location(id, h)));
        }
        let files: Vec<String> = {
            let ix = st.index.read().unwrap();
            let mut ids = ix.files_referencing(word);
            ids.sort_unstable();
            ids.dedup();
            ids.iter()
                .filter_map(|f| ix.file(*f).map(|m| m.rel.clone()))
                .collect()
        };
        'files: for file in files.iter().take(MAX_REF_FILES) {
            let Some(fs) = symbols.file_symbols(&st, file) else {
                continue;
            };
            for r in fs.refs.iter().filter(|r| r.name == word) {
                locations.push(NavLocation {
                    uri: uri_of(id, file),
                    id: Some(id),
                    path: Some(file.clone()),
                    line: r.line,
                    col: r.col,
                    end_line: r.end_line,
                    end_col: r.end_col,
                    kind: Some(format!("reference.{}", r.kind)),
                    container: None,
                    preview: None,
                });
                if locations.len() >= MAX_REFERENCES {
                    break 'files;
                }
            }
        }
        let mut r = index_result(locations);
        r.lsp_state = lsp_state;
        Ok(r)
    })
}

/// Hover card for `word`: fenced signature + kind · container + path.
#[tauri::command(async)]
#[allow(clippy::too_many_arguments)]
pub fn nav_hover(
    symbols: State<'_, SymbolService>,
    agents: State<'_, AgentService>,
    projects: State<'_, ProjectService>,
    lsp: State<'_, LspService>,
    libsrc: State<'_, LibSrcService>,
    id: Uuid,
    path: String,
    line: u32,
    col: u32,
    word: String,
) -> AppResult<Option<NavHover>> {
    crate::perf::timed("nav_hover", || {
        let (root, project) = root_and_project(&agents, id)?;
        let (_, rel) = resolve(&root, &path)?;
        let word = word.trim();
        if word.is_empty() {
            return Ok(None);
        }
        if let Some(LspAnswer::Hover(h)) = lsp_first(
            &lsp,
            &agents,
            &projects,
            id,
            &NavKind::Hover,
            &root,
            &rel,
            line,
            col,
            None,
        ) {
            return Ok(h);
        }
        let st = symbols.ensure_indexed(&root);
        let cur = current_symbols(&symbols, &st, &rel, None);
        let ref_kind = cur.as_deref().and_then(|c| ref_kind_at(c, line, col, word));
        let hits = ranked_definitions(&st, &rel, word, cur.as_deref(), false, ref_kind.as_deref());
        let Some(best) = hits.first() else {
            // library fallback (M4): `kind · fq_name` + the source label
            let lib = libsrc.hits_for(Some(&project), &root, &rel, word, None);
            let Some(h) = lib.first() else {
                return Ok(None);
            };
            let mut contents = format!("```{}\n{}\n```\n{} · {}", h.lang, h.fq_name, h.kind, h.fq_name);
            contents.push('\n');
            contents.push_str(&h.label);
            if lib.len() > 1 {
                contents.push_str(&format!(" (+{} more)", lib.len() - 1));
            }
            return Ok(Some(NavHover {
                source: "index".into(),
                contents,
                range: cur.as_deref().and_then(|c| range_at(c, line, col, word)),
            }));
        };
        let lang = symbols
            .grammar_for_path(&best.path)
            .map(|g| {
                g.manifest
                    .monaco_language
                    .clone()
                    .unwrap_or_else(|| g.manifest.language.clone())
            })
            .unwrap_or_default();
        let sig = best.signature.clone().unwrap_or_else(|| word.to_string());
        let mut contents = format!("```{lang}\n{sig}\n```\n{}", best.kind);
        if let Some(c) = &best.container {
            contents.push_str(&format!(" · {c}"));
        }
        contents.push('\n');
        contents.push_str(&best.path);
        if hits.len() > 1 {
            contents.push_str(&format!(" (+{} more)", hits.len() - 1));
        }
        Ok(Some(NavHover {
            source: "index".into(),
            contents,
            range: cur.as_deref().and_then(|c| range_at(c, line, col, word)),
        }))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::symbols::index::tests::{def, reff, syms};

    #[test]
    fn doc_symbols_builds_the_hierarchy() {
        let mut m = def("run", "method", 2);
        m.container = Some(0);
        let mut inner = def("Inner", "class", 5);
        inner.container = Some(0);
        let mut deep = def("go", "method", 6);
        deep.container = Some(2);
        let fs = syms(
            None,
            &[],
            vec![def("Outer", "class", 0), m, inner, deep, def("top", "function", 20)],
            vec![],
        );
        let tree = doc_symbols(&fs);
        assert_eq!(tree.len(), 2);
        assert_eq!(tree[0].name, "Outer");
        assert_eq!(tree[0].children.len(), 2);
        assert_eq!(tree[0].children[0].name, "run");
        assert_eq!(tree[0].children[0].container.as_deref(), Some("Outer"));
        assert_eq!(tree[0].children[1].children[0].name, "go");
        assert_eq!(tree[1].name, "top");
        assert!(tree[1].children.is_empty());
        // a self/out-of-range container never loops
        let mut bad = def("x", "f", 0);
        bad.container = Some(0);
        let mut oob = def("y", "f", 1);
        oob.container = Some(99);
        let tree = doc_symbols(&syms(None, &[], vec![bad, oob], vec![]));
        assert_eq!(tree.len(), 2);
    }

    #[test]
    fn ref_kind_and_range_at_cursor() {
        let fs = syms(
            None,
            &[],
            vec![def("Foo", "class", 0)],
            vec![reff("bar", "call", 3)], // col 4..7
        );
        assert_eq!(ref_kind_at(&fs, 3, 5, "bar").as_deref(), Some("call"));
        assert_eq!(ref_kind_at(&fs, 3, 9, "bar"), None);
        assert_eq!(ref_kind_at(&fs, 3, 5, "baz"), None);
        let r = range_at(&fs, 3, 7, "bar").unwrap();
        assert_eq!((r.line, r.col, r.end_col), (3, 4, 7));
        let d = range_at(&fs, 0, 6, "Foo").unwrap(); // sel col 6..9
        assert_eq!((d.line, d.col, d.end_col), (0, 6, 9));
        assert!(range_at(&fs, 1, 0, "Foo").is_none());
    }

    #[test]
    fn library_hits_join_after_project_hits() {
        assert!(wants_library(0, "size"));
        assert!(wants_library(3, "ArrayList"), "a type is offered from the library too");
        assert!(!wants_library(3, "size"));
        let h = LibHit {
            uri: "orrery-lib://0a1b/java.base/java/util/Map.java".into(),
            line: 6,
            col: 0,
            fq_name: "java.util.Map$Entry".into(),
            kind: "interface".into(),
            container: Some("java.util.Map".into()),
            label: "JDK 21 (x)".into(),
            source_id: "0a1b".into(),
            entry: "java.base/java/util/Map.java".into(),
            artifact: "src.zip".into(),
            lang: "java".into(),
        };
        let loc = lib_location(&h);
        assert_eq!(loc.uri, h.uri);
        assert!(loc.id.is_none() && loc.path.is_none());
        assert_eq!((loc.line, loc.col, loc.end_line, loc.end_col), (6, 0, 6, 0));
        assert_eq!(loc.kind.as_deref(), Some("interface"));
        assert_eq!(loc.preview.as_deref(), Some("java.util.Map$Entry"));
        let v = serde_json::to_value(&loc).unwrap();
        assert_eq!(v["id"], serde_json::Value::Null);
        assert_eq!(v["path"], serde_json::Value::Null);
    }

    #[test]
    fn resolve_rejects_escapes_and_normalizes() {
        let root = Path::new("C:/w");
        let (abs, rel) = resolve(root, ".\\src\\A.java").unwrap();
        assert_eq!(rel, "src/A.java");
        assert!(abs.ends_with(Path::new("src").join("A.java")));
        assert!(resolve(root, "../x").is_err());
        assert!(resolve(root, "C:/abs").is_err());
        assert!(resolve(root, "").is_err());
    }

    #[test]
    fn contract_shapes_serialize_camel_case() {
        let loc = NavLocation {
            uri: "orrery://x/a.java".into(),
            id: Some(Uuid::nil()),
            path: Some("a.java".into()),
            line: 1,
            col: 2,
            end_line: 1,
            end_col: 5,
            kind: Some("class".into()),
            container: None,
            preview: Some("class A".into()),
        };
        let v = serde_json::to_value(index_result(vec![loc])).unwrap();
        assert_eq!(v["source"], "index");
        assert_eq!(v["lspState"], serde_json::Value::Null);
        let l = &v["locations"][0];
        for k in ["uri", "id", "path", "line", "col", "endLine", "endCol", "kind", "preview"] {
            assert!(l.get(k).is_some(), "{k}");
        }
        assert!(l.get("container").is_none());
        let hit = serde_json::to_value(SymbolHit {
            name: "n".into(),
            kind: "k".into(),
            path: "p".into(),
            line: 0,
            col: 0,
            container: None,
            agent_id: None,
            root: None,
        })
        .unwrap();
        assert_eq!(hit["agentId"], serde_json::Value::Null);
        assert_eq!(hit["root"], serde_json::Value::Null);
        let hover = serde_json::to_value(NavHover {
            source: "index".into(),
            contents: "x".into(),
            range: None,
        })
        .unwrap();
        assert!(hover.get("range").is_none());
        assert_eq!(uri_of(Uuid::nil(), "src/A.java"), format!("orrery://{}/src/A.java", Uuid::nil()));
    }
}
