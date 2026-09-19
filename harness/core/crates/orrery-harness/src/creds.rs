//! Where a login puts the token, so the next process finds it.
//!
//! # Why this exists at all
//!
//! `orrery auth login` had nowhere to write. The two stores a provider could be
//! given were [`MemoryCredStore`](orrery_ext_api::creds::MemoryCredStore),
//! which dies with the process, and
//! [`EnvCredStore`](orrery_ext_api::creds::EnvCredStore), which refuses to
//! write on purpose — "a login that wrote to the process environment would be
//! lost at exit and invisible to every other process, which is worse than
//! refusing". So the only working path to a real model was an undocumented
//! `ANTHROPIC_API_KEY`, and the whole device-code flow had no destination.
//!
//! # It is the `creds` grant, not a file the user manages
//!
//! The file is the **broker's**, under the state directory, in exactly the
//! format [`orrery_broker::FileCredStore`] reads and writes — one
//! `name=value` per line, permissions restricted where the platform has them.
//! A person does not edit it and nothing documents its shape as configuration;
//! `orrery auth login`, `orrery auth logout` and the provider are its only
//! readers and writers. When the broker grows an outgoing transport of its own
//! (plan 14), [`CredStore::get`] here is the one method that goes, and nothing
//! above it changes — which is the same seam
//! [`LayeredCredStore`](orrery_ext_api::creds::LayeredCredStore) documents.
//!
//! # What `get` costs, and why it is here anyway
//!
//! The broker's own `CredStore` has `apply`, `store` and `has` and
//! deliberately no `get`: it *uses* a credential and never returns one. That
//! only works while the broker owns the socket, and it does not yet — a
//! provider still builds its own request and therefore has to have the key.
//! This type is the honest version of that gap: one `get`, one call site, and
//! a `Debug` that prints names and never values.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use async_trait::async_trait;
use orrery_ext_api::creds::CredStore;
use orrery_ext_api::{BrokerError, BrokerResult};
use parking_lot::Mutex;

/// The file, under the state directory.
const FILE: &str = "credentials";

/// Credentials in the state directory, one grant per line.
pub struct FileGrants {
    path: PathBuf,
    lock: Mutex<()>,
}

impl std::fmt::Debug for FileGrants {
    /// Names, never values.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("FileGrants")
            .field("path", &self.path.display().to_string())
            .finish_non_exhaustive()
    }
}

impl FileGrants {
    /// The store for one state directory.
    ///
    /// Nothing is created until something is written: a binary that only ever
    /// *reads* a grant must not leave a credential file behind.
    #[must_use]
    pub fn in_state_dir(state_dir: &Path) -> Self {
        Self {
            path: state_dir.join(FILE),
            lock: Mutex::new(()),
        }
    }

    /// Where it is, so a message can name it.
    #[must_use]
    pub fn path(&self) -> &Path {
        &self.path
    }

    fn read(&self) -> BTreeMap<String, String> {
        let Ok(text) = std::fs::read_to_string(&self.path) else {
            return BTreeMap::new();
        };
        text.lines()
            .filter_map(|line| line.split_once('='))
            .map(|(k, v)| (k.trim().to_owned(), v.trim().to_owned()))
            .collect()
    }

    fn write(&self, held: &BTreeMap<String, String>) -> BrokerResult<()> {
        if let Some(parent) = self.path.parent()
            && !parent.as_os_str().is_empty()
        {
            std::fs::create_dir_all(parent).map_err(|e| io(&self.path, &e))?;
        }
        let body: String = held.iter().map(|(k, v)| format!("{k}={v}\n")).collect();
        std::fs::write(&self.path, body).map_err(|e| io(&self.path, &e))?;
        restrict(&self.path)
    }
}

#[async_trait]
impl CredStore for FileGrants {
    async fn get(&self, grant: &str) -> BrokerResult<Option<String>> {
        let _guard = self.lock.lock();
        Ok(self.read().get(grant).cloned())
    }

    async fn put(&self, grant: &str, secret: &str) -> BrokerResult<()> {
        let _guard = self.lock.lock();
        let mut held = self.read();
        held.insert(grant.to_owned(), secret.to_owned());
        self.write(&held)
    }

    async fn clear(&self, grant: &str) -> BrokerResult<()> {
        let _guard = self.lock.lock();
        let mut held = self.read();
        // Clearing something that was never there is not a failure: `logout`
        // twice is `logout`.
        if held.remove(grant).is_none() {
            return Ok(());
        }
        self.write(&held)
    }

    async fn has(&self, grant: &str) -> BrokerResult<bool> {
        let _guard = self.lock.lock();
        Ok(self.read().contains_key(grant))
    }
}

fn io(path: &Path, e: &std::io::Error) -> BrokerError {
    BrokerError::Io {
        message: format!("{}: {e}", path.display()),
    }
}

#[cfg(unix)]
fn restrict(path: &Path) -> BrokerResult<()> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| io(path, &e))
}

#[cfg(not(unix))]
fn restrict(_path: &Path) -> BrokerResult<()> {
    // Windows has no mode bits; the file inherits the user profile's ACL. The
    // same note `orrery_broker::FileCredStore` carries, and the same answer: a
    // real keychain backend is what `CredStore` exists to allow.
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[tokio::test]
    async fn a_token_survives_the_process_that_wrote_it() {
        let dir = tempfile::tempdir().expect("a state dir");
        let store = FileGrants::in_state_dir(dir.path());
        assert!(!store.has("anthropic").await.expect("a read"));
        assert!(
            !store.path().exists(),
            "reading must not create a credential file"
        );

        store.put("anthropic", "tok-1").await.expect("a write");
        // A second store over the same directory is the next process.
        let next = FileGrants::in_state_dir(dir.path());
        assert_eq!(
            next.get("anthropic").await.expect("a read").as_deref(),
            Some("tok-1")
        );
        assert!(next.has("anthropic").await.expect("a read"));

        next.put("anthropic", "tok-2").await.expect("a rotation");
        assert_eq!(
            store.get("anthropic").await.expect("a read").as_deref(),
            Some("tok-2"),
            "rotation replaces the value under the name"
        );

        next.clear("anthropic").await.expect("a logout");
        assert!(!store.has("anthropic").await.expect("a read"));
        next.clear("anthropic").await.expect("logging out twice is not an error");
    }

    #[tokio::test]
    async fn one_grant_does_not_disturb_another() {
        let dir = tempfile::tempdir().expect("a state dir");
        let store = FileGrants::in_state_dir(dir.path());
        store.put("anthropic", "access").await.expect("a write");
        store
            .put("anthropic.refresh", "refresh")
            .await
            .expect("a write");
        store.clear("anthropic").await.expect("a clear");
        assert_eq!(
            store
                .get("anthropic.refresh")
                .await
                .expect("a read")
                .as_deref(),
            Some("refresh")
        );
    }

    #[test]
    fn debug_never_prints_a_value() {
        let dir = tempfile::tempdir().expect("a state dir");
        let store = FileGrants::in_state_dir(dir.path());
        let shown = format!("{store:?}");
        assert!(!shown.contains("sk-"), "{shown}");
    }
}
