//! Shared fixtures for the config crate's integration tests.
#![allow(dead_code)]

use std::path::{Path, PathBuf};

use orrery_config::ConfigPaths;

/// A throwaway workspace with a user directory that is **outside** it, so the
/// trust store can never be project-writable.
pub struct Fixture {
    dir: tempfile::TempDir,
    root: PathBuf,
    home: PathBuf,
}

impl Fixture {
    pub fn new() -> Self {
        let dir = tempfile::tempdir().expect("a temp dir");
        let base = dunce::canonicalize(dir.path()).expect("canonical base");
        let root = base.join("workspace");
        let home = base.join("home");
        std::fs::create_dir_all(&root).unwrap();
        std::fs::create_dir_all(&home).unwrap();
        Self { dir, root, home }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn home(&self) -> &Path {
        &self.home
    }

    /// The base directory, which holds both the workspace and the home.
    pub fn base(&self) -> &Path {
        self.dir.path()
    }

    /// Write a file under the fixture base, creating parents.
    pub fn write(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.dir.path().join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        dunce::canonicalize(&path).unwrap_or(path)
    }

    /// Write `<workspace>/<rel>`.
    pub fn write_in_root(&self, rel: &str, text: &str) -> PathBuf {
        let path = self.root.join(rel);
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(&path, text).unwrap();
        dunce::canonicalize(&path).unwrap_or(path)
    }

    /// Paths with no OS-managed lookup at all: tests never touch a real
    /// `%ProgramData%` or a real `~`.
    pub fn paths(&self) -> ConfigPaths {
        ConfigPaths::sandboxed(&self.root, self.home.join(".orrery"))
    }
}
