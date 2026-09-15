//! Read-only virtual documents served by a language server (`jdt://…` from
//! jdtls' `java/classFileContents`, …): scheme → owning server via the
//! manifest's `virtualSchemes` + `virtualRead.method`, answered text cached in
//! a 20-entry LRU. `orrery-lib://` is reserved for the M4 library index.

use std::collections::VecDeque;
use std::sync::Mutex;

use serde::Serialize;

/// Entries kept per service.
const LRU: usize = 20;
/// A class-file source beyond this is truncated (the UI shows a note).
pub const MAX_TEXT: usize = 2 * 1024 * 1024;

#[derive(Debug, Clone, Serialize, PartialEq)]
#[serde(rename_all = "camelCase")]
pub struct VirtualDoc {
    pub uri: String,
    /// Monaco language id.
    pub language: String,
    pub text: String,
    /// Tab title: the last path segment of the uri.
    pub title: String,
}

/// Where a virtual uri must be served from.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Dispatch<'a> {
    /// A language server owning this scheme.
    Server(&'a str),
    /// The M4 library index — not wired yet.
    Library,
}

/// `scheme` of `scheme://rest` (`None` when there is no `://`).
pub fn scheme_of(uri: &str) -> Option<&str> {
    let (scheme, _) = uri.split_once("://")?;
    if scheme.is_empty() || !scheme.chars().all(|c| c.is_ascii_alphanumeric() || matches!(c, '+' | '-' | '.')) {
        return None;
    }
    Some(scheme)
}

pub fn dispatch(uri: &str) -> Result<Dispatch<'_>, String> {
    match scheme_of(uri) {
        None => Err(format!("not a virtual document uri: {uri}")),
        Some("orrery-lib") => Ok(Dispatch::Library),
        Some("orrery") | Some("file") => Err(format!("{uri}: a worktree file, not a virtual document")),
        Some(s) => Ok(Dispatch::Server(s)),
    }
}

/// Last path segment, without a query/fragment; falls back to the uri.
pub fn title_of(uri: &str) -> String {
    let body = uri.split_once("://").map(|(_, r)| r).unwrap_or(uri);
    let body = body.split(['?', '#']).next().unwrap_or(body);
    let seg = body.trim_end_matches('/').rsplit('/').next().unwrap_or(body);
    if seg.is_empty() {
        uri.to_string()
    } else {
        seg.to_string()
    }
}

#[derive(Default)]
pub struct VirtualDocs {
    lru: Mutex<VecDeque<VirtualDoc>>,
}

impl VirtualDocs {
    pub fn get(&self, uri: &str) -> Option<VirtualDoc> {
        let mut lru = self.lru.lock().unwrap();
        let pos = lru.iter().position(|d| d.uri == uri)?;
        let doc = lru.remove(pos)?;
        lru.push_front(doc.clone());
        Some(doc)
    }

    pub fn put(&self, doc: VirtualDoc) {
        let mut lru = self.lru.lock().unwrap();
        lru.retain(|d| d.uri != doc.uri);
        lru.push_front(doc);
        lru.truncate(LRU);
    }

    #[cfg(test)]
    fn len(&self) -> usize {
        self.lru.lock().unwrap().len()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scheme_dispatch_and_titles() {
        assert_eq!(scheme_of("jdt://contents/rt.jar/java.lang/String.class?x"), Some("jdt"));
        assert_eq!(scheme_of("src/A.java"), None);
        assert_eq!(scheme_of("://x"), None);
        assert_eq!(dispatch("jdt://contents/x/String.class"), Ok(Dispatch::Server("jdt")));
        assert_eq!(dispatch("orrery-lib://jdk21/java/lang/String.java"), Ok(Dispatch::Library));
        assert!(dispatch("orrery://id/src/A.java").is_err());
        assert!(dispatch("plain/path.rs").is_err());
        assert_eq!(title_of("jdt://contents/rt.jar/java.lang/String.class?=x=/y"), "String.class");
        assert_eq!(title_of("jdt://contents/"), "contents");
        assert_eq!(title_of("x://"), "x://");
    }

    #[test]
    fn lru_keeps_twenty_most_recent() {
        let docs = VirtualDocs::default();
        let doc = |i: usize| VirtualDoc {
            uri: format!("jdt://c/{i}.class"),
            language: "java".into(),
            text: String::new(),
            title: format!("{i}.class"),
        };
        for i in 0..25 {
            docs.put(doc(i));
        }
        assert_eq!(docs.len(), 20);
        assert!(docs.get("jdt://c/0.class").is_none(), "evicted");
        assert!(docs.get("jdt://c/5.class").is_some());
        // touching 5 made it most recent: adding 20 more evicts 6.., not 5
        for i in 100..119 {
            docs.put(doc(i));
        }
        assert!(docs.get("jdt://c/5.class").is_some());
        assert!(docs.get("jdt://c/6.class").is_none());
        // re-put replaces in place (no duplicate)
        docs.put(doc(5));
        assert_eq!(docs.len(), 20);
    }
}
