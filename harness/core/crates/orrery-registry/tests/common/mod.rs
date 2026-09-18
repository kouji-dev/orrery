//! Fixtures, built rather than committed.
//!
//! Plan 15 asks for a signed index, a tampered index and key pairs under
//! `tests/fixtures/`. They are **built here from committed seeds** instead of
//! committed as bytes, for one reason: a signature fixture that is committed
//! cannot be regenerated when a canonical form changes, so the first time
//! somebody reorders a field the whole suite goes red with nothing to tell them
//! why. The seeds are what is committed; a seed is 32 bytes of nothing in
//! particular, and there is no secret anywhere in this repository.
//!
//! Everything here is **offline**. A local directory stands in for crates.io and
//! npm; a local bare git repository stands in for GitHub. No test in this crate
//! opens a socket.

#![allow(dead_code)]

use std::path::{Path, PathBuf};

use orrery_registry::index::{Entry, EntrySource, Index};
use orrery_registry::verify::signing::SigningKey;
use orrery_registry::{Keyring, PublicKey, Timestamp, tree_sha256};

/// The org's current key.
pub const SEED_CURRENT: [u8; 32] = [7u8; 32];
/// The key it is rotating away from.
pub const SEED_PREVIOUS: [u8; 32] = [9u8; 32];
/// A key nobody distributed.
pub const SEED_STRANGER: [u8; 32] = [13u8; 32];

/// The index URL every fixture claims to have come from.
pub const INDEX_URL: &str = "https://registry.corp.internal/orrery/index.toml";

/// The instant every fixture is checked against. No clock, ever.
#[must_use]
pub fn now() -> Timestamp {
    Timestamp::parse("now", "2026-09-18T12:00:00Z").expect("a literal")
}

/// A signing key from one of the seeds.
#[must_use]
pub fn key(id: &str, seed: &[u8; 32]) -> SigningKey {
    SigningKey::from_seed(id, seed).expect("a 32-byte seed")
}

/// A keyring holding one key, in force across the fixture window.
#[must_use]
pub fn keyring(keys: &[&SigningKey]) -> Keyring {
    Keyring::new(
        keys.iter()
            .map(|k| {
                PublicKey::new(
                    k.id(),
                    &k.public_hex(),
                    "2026-01-01T00:00:00Z",
                    "2027-01-01T00:00:00Z",
                )
                .expect("a well-formed key")
            })
            .collect(),
    )
}

/// Write a package: a manifest, and a file with some bytes in it.
///
/// `requires` goes in verbatim, so a test can make the manifest ask for
/// something the index does not list.
pub fn write_package(root: &Path, id: &str, version: &str, requires: &str) -> PathBuf {
    let dir = root.join(id);
    std::fs::create_dir_all(dir.join("src")).expect("a tempdir");
    std::fs::write(
        dir.join("orrery.toml"),
        format!(
            "api = \"orrery-ext/1\"\n\
             runtime = \"native\"\n\
             \n\
             [extension]\n\
             id = \"{id}\"\n\
             version = \"{version}\"\n\
             \n\
             [provides]\n\
             tools = [\"build\"]\n\
             \n\
             [requires]\n\
             {requires}\n"
        ),
    )
    .expect("a tempdir");
    std::fs::write(dir.join("src/lib.rs"), format!("// {id} {version}\n")).expect("a tempdir");
    dir
}

/// A package with a build script that would touch a sentinel if anything ever
/// ran it. Nothing here runs it; that is the point.
pub fn write_package_with_build_script(
    root: &Path,
    id: &str,
    version: &str,
    sentinel: &Path,
) -> PathBuf {
    let dir = write_package(root, id, version, "read = [\"$WORKSPACE/**\"]");
    std::fs::write(
        dir.join("build.rs"),
        format!(
            "fn main() {{ std::fs::write(r\"{}\", b\"the build script ran\").unwrap(); }}\n",
            sentinel.display()
        ),
    )
    .expect("a tempdir");
    dir
}

/// One index entry over a package directory, signed.
pub fn entry_for(
    signer: &SigningKey,
    dir: &Path,
    id: &str,
    version: &str,
    requires: &[&str],
) -> Entry {
    let mut entry = Entry {
        id: id.to_owned(),
        version: version.parse().expect("a semver"),
        source: EntrySource::CratesIo {
            name: format!("orrery-ext-{id}"),
        },
        sha256: tree_sha256(dir).expect("a readable tree"),
        sig: String::new(),
        requires: requires.iter().map(|s| (*s).to_owned()).collect(),
    };
    entry.sig = signer.sign(&entry.signed_bytes());
    entry
}

/// A whole index document over some entries.
#[must_use]
pub fn index_of(entries: Vec<Entry>) -> Index {
    Index {
        schema: 1,
        issued: "2026-09-18T00:00:00Z".to_owned(),
        expires: "2026-12-18T00:00:00Z".to_owned(),
        extensions: entries,
    }
}

/// The index text, and the detached signature over it.
#[must_use]
pub fn sign_index(signer: &SigningKey, index: &Index) -> (String, String) {
    let text = index.to_toml().expect("an index that serialises");
    let sig = signer.sign(text.as_bytes());
    (text, sig)
}

/// A crates.io-shaped mirror directory: `<root>/orrery-ext-<id>/` is the
/// package, which is what [`orrery_registry::DirFetcher`] reads.
pub fn mirror(root: &Path, id: &str, from: &Path) -> PathBuf {
    let dest = root.join(format!("orrery-ext-{id}"));
    orrery_registry::fetch::copy_tree(from, &dest).expect("a tempdir");
    dest
}

/// A bare git repository with one commit, standing in for GitHub.
///
/// Returns the repository path and the commit sha, so a test can assert that a
/// tag install records the sha it actually resolved to.
pub fn bare_repo_with(root: &Path, name: &str, package: &Path, tag: &str) -> (PathBuf, String) {
    let work = root.join(format!("{name}-work"));
    orrery_registry::fetch::copy_tree(package, &work).expect("a tempdir");
    git(&["init", "--quiet", "--initial-branch=main", "."], &work);
    git(&["config", "user.email", "fixture@example.invalid"], &work);
    git(&["config", "user.name", "Fixture"], &work);
    git(&["add", "-A"], &work);
    git(&["commit", "--quiet", "-m", "the extension"], &work);
    git(&["tag", tag], &work);
    let sha = git(&["rev-parse", "HEAD"], &work).trim().to_owned();

    let bare = root.join(format!("{name}.git"));
    git(
        &[
            "clone",
            "--quiet",
            "--bare",
            &work.to_string_lossy(),
            &bare.to_string_lossy(),
        ],
        root,
    );
    (bare, sha)
}

/// Whether there is a usable `git` on this machine at all.
#[must_use]
pub fn git_available() -> bool {
    std::process::Command::new("git")
        .arg("--version")
        .output()
        .is_ok_and(|o| o.status.success())
}

fn git(args: &[&str], cwd: &Path) -> String {
    let out = std::process::Command::new("git")
        .args(args)
        .current_dir(cwd)
        .output()
        .expect("git on PATH");
    assert!(
        out.status.success(),
        "git {args:?} failed: {}",
        String::from_utf8_lossy(&out.stderr)
    );
    String::from_utf8_lossy(&out.stdout).into_owned()
}

/// The string `git` takes as a remote for a local repository.
///
/// A plain path, not a `file://` URL: `file:` is the *path* source form, and
/// giving one string two meanings is exactly what `source.rs` refuses to do.
#[must_use]
pub fn local_remote(path: &Path) -> String {
    path.display().to_string().replace('\\', "/")
}

/// Everything an [`orrery_registry::Installer`] needs, owned in one place.
///
/// Built around a tempdir so that a user layer, a workspace and a package
/// mirror all exist and none of them is anywhere near a real `~`.
pub struct Fixture {
    pub tmp: tempfile::TempDir,
    pub signer: SigningKey,
    pub ring: Keyring,
    pub index: Index,
    pub managed: Option<orrery_registry::ManagedRegistry>,
    pub user_unpinned: Option<orrery_registry::Unpinned>,
    pub fetcher: orrery_registry::DirFetcher,
    pub git: orrery_registry::SystemGit,
    pub hooks: orrery_registry::NoHooks,
    pub layout: orrery_registry::Layout,
}

impl Fixture {
    /// A fixture with an empty index and no managed layer.
    #[must_use]
    pub fn new() -> Self {
        let tmp = tempfile::tempdir().expect("a tempdir");
        let signer = key("org-2026", &SEED_CURRENT);
        let ring = keyring(&[&signer]);
        let user_dir = tmp.path().join("home/.orrery");
        let workspace = tmp.path().join("workspace");
        std::fs::create_dir_all(&user_dir).expect("a tempdir");
        std::fs::create_dir_all(&workspace).expect("a tempdir");
        let layout = orrery_registry::Layout::new(&user_dir, &workspace);
        let fetcher = orrery_registry::DirFetcher::new(tmp.path().join("mirror"));
        Self {
            signer,
            ring,
            index: index_of(Vec::new()),
            managed: None,
            user_unpinned: None,
            fetcher,
            git: orrery_registry::SystemGit,
            hooks: orrery_registry::NoHooks,
            layout,
            tmp,
        }
    }

    /// Where fixture packages are authored.
    #[must_use]
    pub fn packages(&self) -> PathBuf {
        self.tmp.path().join("packages")
    }

    /// The mirror a [`orrery_registry::DirFetcher`] serves.
    #[must_use]
    pub fn mirror_root(&self) -> PathBuf {
        self.tmp.path().join("mirror")
    }

    /// Author a package, mirror it, and pin it in the index.
    pub fn publish(&mut self, id: &str, version: &str, requires: &str, indexed: &[&str]) -> PathBuf {
        let dir = write_package(&self.packages().join(version), id, version, requires);
        mirror(&self.mirror_root(), id, &dir);
        let entry = entry_for(&self.signer, &dir, id, version, indexed);
        self.index.extensions.push(entry);
        dir
    }

    /// Put a managed `[registry]` table in force.
    pub fn managed(&mut self, unpinned: &str) {
        let file = self.tmp.path().join("managed.toml");
        std::fs::write(
            &file,
            format!(
                "[registry]\nindex = \"{INDEX_URL}\"\nunpinned = \"{unpinned}\"\n"
            ),
        )
        .expect("a tempdir");
        self.managed = orrery_registry::ManagedRegistry::read(&file).expect("valid managed toml");
    }

    /// The installer.
    #[must_use]
    pub fn installer(&self) -> orrery_registry::Installer<'_> {
        orrery_registry::Installer {
            layout: self.layout.clone(),
            index: Some(&self.index),
            index_url: INDEX_URL.to_owned(),
            keyring: &self.ring,
            now: now(),
            managed: self.managed.as_ref(),
            user_unpinned: self.user_unpinned,
            fetcher: &self.fetcher,
            git: &self.git,
            hooks: &self.hooks,
            cache: self.tmp.path().join("cache"),
            quarantine: self.tmp.path().join("quarantine"),
        }
    }
}

impl Default for Fixture {
    fn default() -> Self {
        Self::new()
    }
}
