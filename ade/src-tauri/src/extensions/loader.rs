//! Grammar loading: dlopen the pack's parser dylib, pull the `tree_sitter_<x>`
//! symbol, gate on the language ABI, smoke-parse one line, compile the tags /
//! locals queries once. A grammar loaded for real is leaked for the process
//! lifetime — `Language`/`Query` values point into the mapped image, and
//! Windows keeps the file locked while mapped, which is why a newer version
//! goes to its own dir and waits for a restart (`pendingRestart`).

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::{Arc, RwLock};

use libloading::{Library, Symbol};
use tree_sitter::{Language, Parser, Query};
use tree_sitter_language::LanguageFn;

use super::manifest::GrammarManifest;

/// `[MIN_COMPATIBLE_LANGUAGE_VERSION, LANGUAGE_VERSION]` of the linked runtime.
pub fn abi_compatible(abi: usize) -> bool {
    (tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION..=tree_sitter::LANGUAGE_VERSION).contains(&abi)
}

/// One grammar, resident for the process lifetime. `language`/`tags`/`locals`
/// are what the M2 symbols indexer consumes; nothing reads them yet.
#[allow(dead_code)]
pub struct Loaded {
    pub id: String,
    pub version: String,
    pub dir: PathBuf,
    pub manifest: GrammarManifest,
    pub language: Language,
    pub tags: Option<Query>,
    pub locals: Option<Query>,
}

impl std::fmt::Debug for Loaded {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Loaded")
            .field("id", &self.id)
            .field("version", &self.version)
            .field("dir", &self.dir)
            .finish_non_exhaustive()
    }
}

type LanguageSymbol = unsafe extern "C" fn() -> *const ();

/// dlopen + symbol + ABI check. The returned `Language` is only valid while
/// `Library` is alive — callers either leak the library (real load) or drop
/// the language first (probe).
fn open(dir: &Path, m: &GrammarManifest) -> Result<(Library, Language), String> {
    if !abi_compatible(m.abi as usize) {
        return Err(format!(
            "{}: abi {} outside supported {}..={}",
            m.id,
            m.abi,
            tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION,
            tree_sitter::LANGUAGE_VERSION
        ));
    }
    let path = dir.join(&m.library);
    if !path.is_file() {
        return Err(format!("{}: library missing at {}", m.id, path.display()));
    }
    // SAFETY: the pack passed sha256 verification against the registry entry
    // and is a tree-sitter parser (no initializers beyond CRT setup). Loading
    // is inherently trusting the artifact; the registry is the trust root.
    let lib = unsafe { Library::new(&path) }
        .map_err(|e| format!("{}: load {}: {e}", m.id, path.display()))?;
    let language = {
        let sym: Symbol<LanguageSymbol> = unsafe { lib.get(m.symbol.as_bytes()) }
            .map_err(|e| format!("{}: symbol {}: {e}", m.id, m.symbol))?;
        // SAFETY: `sym` is the CLI-generated `tree_sitter_<lang>` entry point.
        Language::new(unsafe { LanguageFn::from_raw(*sym) })
    };
    let abi = language.abi_version();
    if !abi_compatible(abi) {
        return Err(format!(
            "{}: dylib reports abi {abi}, manifest says {} — outside supported range",
            m.id, m.abi
        ));
    }
    Ok((lib, language))
}

fn smoke_parse(language: &Language, id: &str) -> Result<(), String> {
    let mut parser = Parser::new();
    parser
        .set_language(language)
        .map_err(|e| format!("{id}: set_language: {e}"))?;
    parser
        .parse("x\n", None)
        .map(|_| ())
        .ok_or_else(|| format!("{id}: smoke parse returned no tree"))
}

fn compile_query(
    dir: &Path,
    language: &Language,
    rel: Option<&str>,
    what: &str,
    id: &str,
) -> Result<Option<Query>, String> {
    let Some(rel) = rel else {
        return Ok(None);
    };
    let path = dir.join(rel);
    let src = std::fs::read_to_string(&path)
        .map_err(|e| format!("{id}: {what} query {}: {e}", path.display()))?;
    Query::new(language, &src)
        .map(Some)
        .map_err(|e| format!("{id}: {what} query: {e}"))
}

/// Full load without keeping anything: dlopen, symbol, ABI, one-line parse,
/// query compile, then unload. Run on the staged dir BEFORE activation so a
/// broken artifact never replaces a working one.
pub fn probe(dir: &Path, m: &GrammarManifest) -> Result<(), String> {
    let (lib, language) = open(dir, m)?;
    smoke_parse(&language, &m.id)?;
    let tags = compile_query(dir, &language, m.queries.tags.as_deref(), "tags", &m.id)?;
    let locals = compile_query(dir, &language, m.queries.locals.as_deref(), "locals", &m.id)?;
    // Everything pointing into the image goes first; only then unmap.
    drop(tags);
    drop(locals);
    drop(language);
    drop(lib);
    Ok(())
}

/// Load for real. The library is leaked: nothing may ever unmap it while
/// `Language`/`Query` values derived from it exist anywhere in the process.
pub fn load(dir: &Path, m: &GrammarManifest) -> Result<Loaded, String> {
    let (lib, language) = open(dir, m)?;
    smoke_parse(&language, &m.id)?;
    let tags = compile_query(dir, &language, m.queries.tags.as_deref(), "tags", &m.id)?;
    let locals = compile_query(dir, &language, m.queries.locals.as_deref(), "locals", &m.id)?;
    std::mem::forget(lib);
    Ok(Loaded {
        id: m.id.clone(),
        version: m.version.clone(),
        dir: dir.to_path_buf(),
        manifest: m.clone(),
        language,
        tags,
        locals,
    })
}

#[derive(Default)]
struct Maps {
    by_id: HashMap<String, Arc<Loaded>>,
    by_ext: HashMap<String, Arc<Loaded>>,
    by_lang: HashMap<String, Arc<Loaded>>,
}

/// The process-wide set of resident grammars, keyed by pack id, file
/// extension and language name. Removing an entry only forgets it (the dylib
/// stays mapped) — that is what makes replacement a restart affair.
#[derive(Default)]
pub struct GrammarRegistry {
    maps: RwLock<Maps>,
}

fn norm_ext(ext: &str) -> String {
    ext.trim().trim_start_matches('.').to_ascii_lowercase()
}

impl GrammarRegistry {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn insert(&self, loaded: Loaded) -> Arc<Loaded> {
        let loaded = Arc::new(loaded);
        let mut m = self.maps.write().unwrap();
        if let Some(old) = m.by_id.insert(loaded.id.clone(), loaded.clone()) {
            m.by_ext.retain(|_, v| !Arc::ptr_eq(v, &old));
            m.by_lang.retain(|_, v| !Arc::ptr_eq(v, &old));
        }
        for ext in &loaded.manifest.file_extensions {
            m.by_ext.insert(norm_ext(ext), loaded.clone());
        }
        m.by_lang
            .insert(loaded.manifest.language.to_ascii_lowercase(), loaded.clone());
        loaded
    }

    /// Forget a grammar (disable/uninstall). The image stays mapped.
    pub fn remove(&self, id: &str) -> bool {
        let mut m = self.maps.write().unwrap();
        let Some(old) = m.by_id.remove(id) else {
            return false;
        };
        m.by_ext.retain(|_, v| !Arc::ptr_eq(v, &old));
        m.by_lang.retain(|_, v| !Arc::ptr_eq(v, &old));
        true
    }

    pub fn is_loaded(&self, id: &str) -> bool {
        self.maps.read().unwrap().by_id.contains_key(id)
    }

    pub fn loaded_version(&self, id: &str) -> Option<String> {
        self.maps
            .read()
            .unwrap()
            .by_id
            .get(id)
            .map(|l| l.version.clone())
    }

    #[allow(dead_code)] // M2 consumer surface
    pub fn get(&self, id: &str) -> Option<Arc<Loaded>> {
        self.maps.read().unwrap().by_id.get(id).cloned()
    }

    /// Grammar for a file extension (`"java"` or `".java"`, any case).
    #[allow(dead_code)] // M2 consumer surface
    pub fn grammar_for_extension(&self, ext: &str) -> Option<Arc<Loaded>> {
        self.maps.read().unwrap().by_ext.get(&norm_ext(ext)).cloned()
    }

    #[allow(dead_code)] // M2 consumer surface
    pub fn grammar_for_language(&self, lang: &str) -> Option<Arc<Loaded>> {
        self.maps
            .read()
            .unwrap()
            .by_lang
            .get(&lang.to_ascii_lowercase())
            .cloned()
    }

    pub fn loaded_ids(&self) -> Vec<String> {
        let mut ids: Vec<String> = self.maps.read().unwrap().by_id.keys().cloned().collect();
        ids.sort();
        ids
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::extensions::manifest::{fixtures, Manifest};

    #[test]
    fn abi_range_matches_runtime_constants() {
        assert!(abi_compatible(tree_sitter::LANGUAGE_VERSION));
        assert!(abi_compatible(tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION));
        assert!(!abi_compatible(tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION - 1));
        assert!(!abi_compatible(tree_sitter::LANGUAGE_VERSION + 1));
    }

    #[test]
    fn open_refuses_out_of_range_abi_before_touching_the_file() {
        let Manifest::Grammar(mut m) = Manifest::parse(fixtures::GRAMMAR).unwrap() else {
            unreachable!()
        };
        m.abi = (tree_sitter::LANGUAGE_VERSION + 1) as u32;
        let err = probe(Path::new("Z:/does/not/exist"), &m).unwrap_err();
        assert!(err.contains("abi"), "{err}");
        m.abi = tree_sitter::LANGUAGE_VERSION as u32;
        let err = probe(Path::new("Z:/does/not/exist"), &m).unwrap_err();
        assert!(err.contains("library missing"), "{err}");
    }

    /// Real dylib round-trip. Needs a built grammar pack dir (manifest.json +
    /// the dylib + queries) — produced by `pnpm ext:build`; point
    /// `ORRERY_TS_FIXTURE_DIR` at it and run with `--ignored`.
    #[test]
    #[ignore]
    fn loads_a_real_grammar_pack_when_fixture_dir_is_set() {
        let Some(dir) = std::env::var_os("ORRERY_TS_FIXTURE_DIR") else {
            eprintln!("ORRERY_TS_FIXTURE_DIR unset — skipping");
            return;
        };
        let dir = PathBuf::from(dir);
        let json = std::fs::read_to_string(dir.join("manifest.json")).unwrap();
        let Manifest::Grammar(m) = Manifest::parse(&json).unwrap() else {
            panic!("fixture must be a grammar pack")
        };
        probe(&dir, &m).unwrap();
        let loaded = load(&dir, &m).unwrap();
        assert_eq!(loaded.language.abi_version() as u32, m.abi);
        let reg = GrammarRegistry::new();
        reg.insert(loaded);
        let ext = m.file_extensions[0].clone();
        let g = reg.grammar_for_extension(&format!(".{ext}")).expect("by ext");
        assert_eq!(g.id, m.id);
        assert!(reg.grammar_for_language(&m.language).is_some());
        let mut p = Parser::new();
        p.set_language(&g.language).unwrap();
        assert!(p.parse("class A {}", None).is_some());
        assert!(reg.remove(&m.id));
        assert!(reg.grammar_for_extension(&ext).is_none());
    }
}
