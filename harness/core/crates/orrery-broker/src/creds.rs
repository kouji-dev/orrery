//! Credentials, which are **used** and never **read**.
//!
//! The shape is the point. A provider hands the broker a request to *use* a
//! credential — sign this header — not a request to *read* one. There is no
//! method anywhere in this module that returns a secret, so rotation rewrites
//! the name's value and nothing holding a reference changes.
//!
//! ```
//! use orrery_broker::creds::{CredUse, MemoryCredStore, RequestSlot};
//! # use orrery_broker::creds::CredStore;
//! let store = MemoryCredStore::new();
//! store.store("ANTHROPIC_API_KEY", "sk-live-1").unwrap();
//!
//! // A slot is what a request looks like before it has been authorised.
//! let slot = RequestSlot::new("https://api.anthropic.com/v1/messages");
//! let use_it = CredUse::Header {
//!     slot: slot.clone(),
//!     header: "x-api-key".to_owned(),
//!     scheme: None,
//! };
//! // `use_it` goes to `Broker::creds`, which resolves the NAME at the point of
//! // use. The caller can see that the header is set, and never what it is set to.
//! # store.apply("ANTHROPIC_API_KEY", &use_it).unwrap();
//! assert!(slot.has_header("x-api-key"));
//! ```
//!
//! There is no `slot.header_value("x-api-key")`, and there is no
//! `store.resolve(name)`: both would be the thing this module exists to prevent.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;

use orrery_audit::Digest;
use parking_lot::Mutex;

use crate::error::BrokerError;

/// A request on its way out, before and after it has been authorised.
///
/// Header **names** are readable. Header **values** are not: the only way to put
/// one in is [`CredStore::apply`], and there is no way to take one out except by
/// sending the request, which only the broker's transport does.
#[derive(Clone, Default)]
pub struct RequestSlot(Arc<Mutex<Prepared>>);

impl std::fmt::Debug for RequestSlot {
    /// Header **names**, never header values. A derived `Debug` would put the
    /// credential in every log line that formatted a request.
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        let p = self.0.lock();
        f.debug_struct("RequestSlot")
            .field("method", &p.method)
            .field("url", &p.url)
            .field("headers", &p.headers.keys().collect::<Vec<_>>())
            .field("body_len", &p.body.len())
            .finish()
    }
}

#[derive(Debug, Default)]
struct Prepared {
    url: String,
    method: String,
    headers: BTreeMap<String, String>,
    body: Vec<u8>,
}

impl RequestSlot {
    /// A GET to a url, with no headers yet.
    #[must_use]
    pub fn new(url: impl Into<String>) -> Self {
        Self(Arc::new(Mutex::new(Prepared {
            url: url.into(),
            method: "GET".to_owned(),
            ..Prepared::default()
        })))
    }

    /// Where it is going.
    #[must_use]
    pub fn url(&self) -> String {
        self.0.lock().url.clone()
    }

    /// Which method.
    #[must_use]
    pub fn method(&self) -> String {
        self.0.lock().method.clone()
    }

    /// Set the method.
    pub fn set_method(&self, method: impl Into<String>) {
        self.0.lock().method = method.into();
    }

    /// Set the body.
    pub fn set_body(&self, body: impl Into<Vec<u8>>) {
        self.0.lock().body = body.into();
    }

    /// Set a header that is not a secret.
    pub fn set_header(&self, name: impl Into<String>, value: impl Into<String>) {
        self.0.lock().headers.insert(name.into(), value.into());
    }

    /// Which headers are set. Names only.
    #[must_use]
    pub fn header_names(&self) -> Vec<String> {
        self.0.lock().headers.keys().cloned().collect()
    }

    /// Whether a header is set.
    #[must_use]
    pub fn has_header(&self, name: &str) -> bool {
        self.0.lock().headers.contains_key(name)
    }

    /// The digest of a header's value, for a test or an audit record that has
    /// to say "this changed" without saying what it is.
    #[must_use]
    pub fn header_digest(&self, name: &str) -> Option<Digest> {
        self.0
            .lock()
            .headers
            .get(name)
            .map(|v| Digest::of_bytes(v.as_bytes()))
    }

    /// Hand the whole thing to a transport. Crate-private: this is the only way
    /// a header value leaves the slot, and it leaves towards the wire.
    pub(crate) fn take(&self) -> (String, String, BTreeMap<String, String>, Vec<u8>) {
        let p = self.0.lock();
        (
            p.method.clone(),
            p.url.clone(),
            p.headers.clone(),
            p.body.clone(),
        )
    }
}

/// What to do with a credential, at the point of use.
#[non_exhaustive]
#[derive(Clone, Debug)]
#[allow(clippy::exhaustive_enums)]
pub enum CredUse {
    /// Put it in a header of a request that is about to be sent.
    Header {
        /// The request it authorises.
        slot: RequestSlot,
        /// The header name — `authorization`, `x-api-key`.
        header: String,
        /// A scheme to prefix, such as `Bearer`. `None` writes the value alone.
        scheme: Option<String>,
    },
}

/// Somewhere credentials live.
///
/// Note what the trait does **not** have: a `resolve` or a `get`. A store can be
/// asked to *apply* a credential and never to hand one over.
pub trait CredStore: Send + Sync + std::fmt::Debug {
    /// Resolve `name` and use it, without returning it.
    ///
    /// # Errors
    ///
    /// [`BrokerError::NoCredential`] when nothing is stored under that name.
    fn apply(&self, name: &str, use_it: &CredUse) -> Result<(), BrokerError>;

    /// Write a value under a name, creating or replacing it.
    ///
    /// Rotation is exactly this: nothing holding the *name* changes.
    ///
    /// # Errors
    ///
    /// When the backing store cannot be written.
    fn store(&self, name: &str, value: &str) -> Result<(), BrokerError>;

    /// Whether a name is known. Not whether its value is anything in particular.
    fn has(&self, name: &str) -> bool;
}

fn apply_to(use_it: &CredUse, value: &str) {
    match use_it {
        CredUse::Header {
            slot,
            header,
            scheme,
        } => {
            let composed = match scheme {
                Some(scheme) => format!("{scheme} {value}"),
                None => value.to_owned(),
            };
            slot.0.lock().headers.insert(header.clone(), composed);
        }
    }
}

/// Credentials held in memory. For tests, and for a session that was handed its
/// secrets rather than told where to find them.
#[derive(Debug, Default)]
pub struct MemoryCredStore(Mutex<BTreeMap<String, String>>);

impl MemoryCredStore {
    /// An empty store.
    #[must_use]
    pub fn new() -> Self {
        Self::default()
    }
}

impl CredStore for MemoryCredStore {
    fn apply(&self, name: &str, use_it: &CredUse) -> Result<(), BrokerError> {
        let held = self.0.lock();
        let Some(value) = held.get(name) else {
            return Err(BrokerError::NoCredential(name.to_owned()));
        };
        apply_to(use_it, value);
        Ok(())
    }

    fn store(&self, name: &str, value: &str) -> Result<(), BrokerError> {
        self.0.lock().insert(name.to_owned(), value.to_owned());
        Ok(())
    }

    fn has(&self, name: &str) -> bool {
        self.0.lock().contains_key(name)
    }
}

/// Credentials in a file, one `name=value` per line, mode `0600` where the
/// platform has modes.
///
/// The fallback for a platform with no OS keychain, and the shape a keychain
/// implementation slots into unchanged: `apply` resolves at the point of use
/// either way.
#[derive(Debug)]
pub struct FileCredStore {
    path: PathBuf,
    lock: Mutex<()>,
}

impl FileCredStore {
    /// Open (or create) a credential file.
    ///
    /// # Errors
    ///
    /// When the file cannot be created with the right permissions.
    pub fn open(path: impl AsRef<Path>) -> Result<Self, BrokerError> {
        let path = path.as_ref().to_path_buf();
        if let Some(parent) = path.parent() {
            if !parent.as_os_str().is_empty() {
                std::fs::create_dir_all(parent).map_err(|e| BrokerError::io(parent, e))?;
            }
        }
        if !path.exists() {
            std::fs::write(&path, "").map_err(|e| BrokerError::io(&path, e))?;
        }
        restrict(&path)?;
        Ok(Self {
            path,
            lock: Mutex::new(()),
        })
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
}

impl CredStore for FileCredStore {
    fn apply(&self, name: &str, use_it: &CredUse) -> Result<(), BrokerError> {
        let _guard = self.lock.lock();
        let held = self.read();
        let Some(value) = held.get(name) else {
            return Err(BrokerError::NoCredential(name.to_owned()));
        };
        apply_to(use_it, value);
        Ok(())
    }

    fn store(&self, name: &str, value: &str) -> Result<(), BrokerError> {
        let _guard = self.lock.lock();
        let mut held = self.read();
        held.insert(name.to_owned(), value.to_owned());
        let body: String = held
            .iter()
            .map(|(k, v)| format!("{k}={v}\n"))
            .collect::<Vec<_>>()
            .concat();
        std::fs::write(&self.path, body).map_err(|e| BrokerError::io(&self.path, e))?;
        restrict(&self.path)
    }

    fn has(&self, name: &str) -> bool {
        let _guard = self.lock.lock();
        self.read().contains_key(name)
    }
}

#[cfg(unix)]
fn restrict(path: &Path) -> Result<(), BrokerError> {
    use std::os::unix::fs::PermissionsExt as _;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|e| BrokerError::io(path, e))
}

#[cfg(not(unix))]
fn restrict(_path: &Path) -> Result<(), BrokerError> {
    // Windows has no mode bits. The file inherits the user profile's ACL, which
    // is the same protection the OS keychain path would rely on; a real keychain
    // backend is the right answer here and is what `CredStore` exists for.
    Ok(())
}
