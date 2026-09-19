//! `orrery registry init|add|sign|verify` — the admin half of phase 8.
//!
//! # Why this exists
//!
//! Section 8 phase 8 is "an admin pins a version set and unpinned extensions
//! refuse to load". The refusing half was real: install, managed `unpinned =
//! "refuse"`, a load-time receipt, a skip in the ledger. The **pinning** half
//! was not. `cmd::install` says fetching a remote index is not implemented, and
//! nothing in this binary could create, sign or publish one — so an admin could
//! only pin a version set if somebody handed them a file made by a tool that
//! does not ship here. That is not a workflow, and the plan said it was.
//!
//! These four verbs are that tool:
//!
//! ```text
//! orrery registry init   --index <path> [--key <path>]   a key and an empty index
//! orrery registry add    --index <path> --id <id> --version <v> --source <s>
//! orrery registry sign   --index <path> --key <path>     the document signature
//! orrery registry verify --index <path> [--key <hex>]    check it back
//! ```
//!
//! # Offline, and it stays offline
//!
//! Nothing here opens a socket. `add` reads the package out of the **mirror
//! directory the installer fetches from** — [`crate::cmd::install::mirror_dir`],
//! one function, so an admin cannot pin bytes from a place the install would
//! never look. An air-gapped organisation mirrors its packages there, pins
//! them, signs the index and ships both.
//!
//! # The private key is a file, and it says so
//!
//! `init` writes the seed to `--key`, warns that it is a secret, and prints the
//! **public** half on stdout so it can be pasted into `managed.toml`. There is
//! no key store, no agent and no passphrase: this is a first cut of an admin
//! tool, and pretending otherwise would be worse than saying it plainly here
//! and in plan 15.
//!
//! Implementation plan: `harness/docs/plans/15-registry-supply-chain.md`.

use std::path::{Path, PathBuf};

use orrery_registry::author::{self, Signed};
use orrery_registry::index::{EntrySource, Index};
use orrery_registry::verify::signing::SigningKey;
use orrery_registry::{Keyring, PublicKey, Timestamp, Version};

use crate::args::{Cli, RegistryCommand};
use crate::cmd::install::{mirror_dir, now, plus_days, state_dir};
use crate::exit::{Exit, fail};

/// How long a fresh index is valid for, when nobody says otherwise.
///
/// A year: long enough that an organisation is not re-signing weekly, short
/// enough that an index nobody maintains eventually stops being accepted, which
/// is the point of the window.
const DEFAULT_VALID_DAYS: u64 = 365;

/// Dispatch a `registry` subcommand.
pub fn dispatch(cli: &Cli, command: &RegistryCommand) -> ! {
    match command {
        RegistryCommand::Init {
            index,
            key,
            key_id,
            expires,
        } => init(index, key.as_deref(), key_id, expires.as_deref()),
        RegistryCommand::Add {
            index,
            id,
            version,
            source,
        } => add(cli, index, id, version, source),
        RegistryCommand::Sign { index, key } => sign(index, key),
        RegistryCommand::Verify { index, key } => verify(index, key.as_deref()),
    }
}

/// `orrery registry init` — a signing key, and an empty index to fill.
fn init(index: &Path, key: Option<&Path>, key_id: &str, expires: Option<&str>) -> ! {
    if index.exists() {
        fail(
            Exit::Usage,
            format!(
                "{} already exists; `orrery registry add` extends an index rather than replacing it",
                index.display()
            ),
        );
    }
    let key_path = key.map_or_else(|| default_key_path(index), Path::to_path_buf);
    if key_path.exists() {
        fail(
            Exit::Usage,
            format!(
                "{} already exists; a signing key is never overwritten - every index it signed would stop verifying",
                key_path.display()
            ),
        );
    }

    let seed = orrery_registry::generate_seed().unwrap_or_else(|e| fail(Exit::Kernel, e));
    let signer = SigningKey::from_seed(key_id, &seed).unwrap_or_else(|e| fail(Exit::Kernel, e));
    let public = signer.public_hex();

    let issued = now();
    let expires = match expires {
        Some(text) => Timestamp::parse("expires", text)
            .unwrap_or_else(|e| fail(Exit::Usage, e))
            .as_str()
            .to_owned(),
        None => plus_days(DEFAULT_VALID_DAYS),
    };
    let doc = author::new_index(issued.as_str(), &expires)
        .unwrap_or_else(|e| fail(Exit::Usage, e));
    let text = doc.to_toml().unwrap_or_else(|e| fail(Exit::Kernel, e));
    write(index, &text);
    write(
        &key_path,
        &format!(
            "# The `{key_id}` signing key for {index}.\n\
             #\n\
             # THIS FILE IS A SECRET. Anything holding it can sign an index this\n\
             # organisation's machines will accept. It is not encrypted and there\n\
             # is no passphrase: keep it where you keep your other signing keys.\n\
             #\n\
             # The public half goes in managed.toml, under `[registry] key`.\n\
             id = \"{key_id}\"\n\
             seed = \"{seed}\"\n\
             public = \"{public}\"\n",
            index = index.display(),
            seed = hex_of(&seed),
        ),
    );

    eprintln!("orrery: wrote {} (valid until {expires})", index.display());
    eprintln!(
        "orrery: wrote {} - this is a secret; anything holding it can sign an index",
        key_path.display()
    );
    eprintln!(
        "orrery: distribute the public key through the managed layer:\n  [registry]\n  index = \"{}\"\n  key = \"{public}\"\n  unpinned = \"refuse\"",
        index.display()
    );
    // stdout is data: the public key, so `orrery registry init` composes.
    println!("{public}");
    Exit::Ok.exit()
}

/// `orrery registry add` — pin one version.
fn add(cli: &Cli, index: &Path, id: &str, version: &str, source: &str) -> ! {
    let mut doc = read_index(index);
    let version: Version = version
        .parse()
        .unwrap_or_else(|e| fail(Exit::Usage, format!("`{version}` is not a version: {e}")));
    let source: EntrySource = source
        .parse()
        .unwrap_or_else(|e| fail(Exit::Usage, e));

    if doc.entry(id, &version).is_some() {
        fail(
            Exit::Usage,
            format!(
                "{id}@{version} is already pinned in {}; a pin is not edited in place - remove it or publish a new version",
                index.display()
            ),
        );
    }

    // The bytes come from the mirror the **installer** fetches from, staged the
    // way the installer stages them. Pinning a hash no install would compute is
    // therefore not expressible here.
    let mirror = mirror_dir(&state_dir(cli));
    let fetcher = orrery_registry::DirFetcher::new(&mirror);
    let staging = staging_dir(index, id, &version);
    if staging.exists() {
        let _ = std::fs::remove_dir_all(&staging);
    }
    let entry = author::entry_from_fetch(id, version.clone(), source, &fetcher, &staging)
        .unwrap_or_else(|e| {
            fail(
                Exit::Usage,
                format!("{e}\n  the mirror this build reads is {}", mirror.display()),
            )
        });

    println!(
        "{id}  {version}  {}  sha256:{}",
        entry.source, entry.sha256
    );
    println!(
        "    requires: {}",
        if entry.requires.is_empty() {
            "nothing".to_owned()
        } else {
            entry.requires.join(", ")
        }
    );

    doc.extensions.push(entry);
    let text = doc.to_toml().unwrap_or_else(|e| fail(Exit::Kernel, e));
    write(index, &text);

    // The document changed, so any signature beside it is now a signature over
    // something else. Leaving it there is the one state `author` exists to
    // prevent.
    let sig = author::signature_path(index);
    if sig.exists() {
        if let Err(e) = std::fs::remove_file(&sig) {
            fail(Exit::Kernel, format!("{}: {e}", sig.display()));
        }
        eprintln!(
            "orrery: removed {} - the index changed and the old signature no longer covers it",
            sig.display()
        );
    }
    eprintln!("orrery: pinned; sign it with `orrery registry sign`");
    Exit::Ok.exit()
}

/// `orrery registry sign` — every entry, then the document.
fn sign(index: &Path, key: &Path) -> ! {
    let mut doc = read_index(index);
    let signer = read_key(key);
    let signed: Signed =
        author::sign_index(&mut doc, &signer).unwrap_or_else(|e| fail(Exit::Kernel, e));
    // The text that is written is the text that was signed. Nothing here
    // renders the document a second time.
    let sig = author::write_signed(index, &signed).unwrap_or_else(|e| fail(Exit::Kernel, e));

    eprintln!(
        "orrery: signed {} pin(s) and the document itself",
        doc.extensions.len()
    );
    eprintln!("orrery: wrote {}", sig.display());
    println!("{}", index.display());
    Exit::Ok.exit()
}

/// `orrery registry verify` — check an index back, the way an install will.
fn verify(index: &Path, key: Option<&str>) -> ! {
    let text = read_text(index);
    let doc = Index::parse(&text, index.display().to_string())
        .unwrap_or_else(|e| fail(Exit::Usage, e));
    let keyring = keyring_for(key);
    let now = now();
    let file = index.display().to_string();

    let sig_path = author::signature_path(index);
    let signature = std::fs::read_to_string(&sig_path).unwrap_or_else(|e| {
        fail(
            Exit::Usage,
            format!(
                "{}: {e}; an index with no signature is not a signed index - `orrery registry sign` makes one",
                sig_path.display()
            ),
        )
    });
    let by = orrery_registry::verify::verify_index(
        &doc,
        &text,
        signature.trim(),
        &keyring,
        &now,
        &file,
        &file,
    )
    .unwrap_or_else(|e| fail(Exit::Denied, e));
    println!("index    {file}  signed by {by}  valid until {}", doc.expires);

    for entry in &doc.extensions {
        let by = orrery_registry::verify::verify_entry(entry, &keyring, &now)
            .unwrap_or_else(|e| fail(Exit::Denied, e));
        println!(
            "pin      {}  {}  {}  signed by {by}",
            entry.id, entry.version, entry.source
        );
    }
    eprintln!(
        "orrery: {} pin(s) verified at {now}",
        doc.extensions.len()
    );
    Exit::Ok.exit()
}

/// The keyring to check against: the hex a person passed, or the managed layer.
///
/// The managed path is the same [`crate::cmd::install::keyring`] an install
/// uses, so `verify` cannot say yes to an index the install would reject for
/// being signed by the wrong key.
fn keyring_for(key: Option<&str>) -> Keyring {
    let Some(hex) = key else {
        let keyring = crate::cmd::install::managed_keyring();
        if keyring.keys().is_empty() {
            fail(
                Exit::Usage,
                "no key to check against: pass `--key <hex>`, or set `[registry] key` in the managed layer",
            );
        }
        return keyring;
    };
    match PublicKey::new("managed", hex, "1970-01-01T00:00:00Z", "9999-12-31T23:59:59Z") {
        Ok(key) => Keyring::new(vec![key]),
        Err(e) => fail(Exit::Usage, e),
    }
}

/// Where an index's key goes when `--key` does not say.
fn default_key_path(index: &Path) -> PathBuf {
    index.with_file_name("signing-key.toml")
}

/// Where `add` stages the bytes it hashed, so an admin can look at them.
fn staging_dir(index: &Path, id: &str, version: &Version) -> PathBuf {
    index
        .with_file_name("staging")
        .join(format!("{id}-{version}"))
}

fn read_index(path: &Path) -> Index {
    let text = read_text(path);
    Index::parse(&text, path.display().to_string()).unwrap_or_else(|e| fail(Exit::Usage, e))
}

fn read_text(path: &Path) -> String {
    std::fs::read_to_string(path).unwrap_or_else(|e| {
        fail(
            Exit::Usage,
            format!("{}: {e}; `orrery registry init` makes one", path.display()),
        )
    })
}

/// A signing key, from the file `init` wrote.
fn read_key(path: &Path) -> SigningKey {
    let text = std::fs::read_to_string(path)
        .unwrap_or_else(|e| fail(Exit::Usage, format!("{}: {e}", path.display())));
    let doc: toml::Value = text
        .parse()
        .unwrap_or_else(|e| fail(Exit::Usage, format!("{}: {e}", path.display())));
    let id = doc
        .get("id")
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| fail(Exit::Usage, format!("{}: no `id`", path.display())));
    let seed = doc
        .get("seed")
        .and_then(toml::Value::as_str)
        .unwrap_or_else(|| fail(Exit::Usage, format!("{}: no `seed`", path.display())));
    let bytes = bytes_of(seed)
        .unwrap_or_else(|| fail(Exit::Usage, format!("{}: `seed` is not 32 bytes of hex", path.display())));
    SigningKey::from_seed(id, &bytes).unwrap_or_else(|e| fail(Exit::Usage, e))
}

fn write(path: &Path, text: &str) {
    if let Some(parent) = path.parent() {
        if let Err(e) = std::fs::create_dir_all(parent) {
            fail(Exit::Kernel, format!("{}: {e}", parent.display()));
        }
    }
    if let Err(e) = std::fs::write(path, text) {
        fail(Exit::Kernel, format!("{}: {e}", path.display()));
    }
}

/// Lowercase hex, written out rather than taken from a crate: this binary needs
/// it in one direction, for one 32-byte value.
fn hex_of(bytes: &[u8]) -> String {
    let mut out = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        out.push_str(&format!("{byte:02x}"));
    }
    out
}

/// The other direction, for the seed a key file holds.
fn bytes_of(text: &str) -> Option<[u8; 32]> {
    let text = text.trim();
    if text.len() != 64 {
        return None;
    }
    let mut out = [0u8; 32];
    for (slot, pair) in out.iter_mut().zip(text.as_bytes().chunks_exact(2)) {
        let pair = std::str::from_utf8(pair).ok()?;
        *slot = u8::from_str_radix(pair, 16).ok()?;
    }
    Some(out)
}

#[cfg(test)]
mod tests {
    use super::{bytes_of, hex_of};

    /// Hex round-trips both ways, which is all this file asks of it.
    #[test]
    fn hex_round_trips() {
        let seed = [0x0au8; 32];
        let text = hex_of(&seed);
        assert_eq!(text.len(), 64);
        assert_eq!(bytes_of(&text), Some(seed));
        assert_eq!(bytes_of("not hex"), None);
        assert_eq!(bytes_of(&"z".repeat(64)), None);
    }
}
