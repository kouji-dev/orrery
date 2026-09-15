//! Install pipeline building blocks: sha256 verification, zip extraction with
//! a zip-slip guard, manifest discovery inside an unpacked pack, and the
//! pure activation state machine (immediate vs. pending-restart). The
//! orchestration (download → tmp → verify → unzip → probe → activate) lives in
//! `ExtensionService::run_install`; everything here is side-effect-free or
//! confined to a directory the caller hands in, which is what makes it
//! testable with in-test zips.

use std::fs;
use std::io::{Read, Write};
use std::path::{Component, Path, PathBuf};

use sha2::{Digest, Sha256};

use super::manifest::Manifest;

/// Unpacked bytes cap per pack (a zip bomb stops here, not on the disk).
pub const MAX_UNPACKED_BYTES: u64 = 1024 * 1024 * 1024;
/// Entry count cap per pack.
pub const MAX_ENTRIES: usize = 10_000;

fn hex(bytes: &[u8]) -> String {
    let mut s = String::with_capacity(bytes.len() * 2);
    for b in bytes {
        s.push_str(&format!("{b:02x}"));
    }
    s
}

#[allow(dead_code)] // in-memory twin of `sha256_file`; tests + M3 server downloads
pub fn sha256_hex(bytes: &[u8]) -> String {
    hex(&Sha256::digest(bytes))
}

pub fn sha256_file(path: &Path) -> Result<String, String> {
    let mut f = fs::File::open(path).map_err(|e| format!("{}: {e}", path.display()))?;
    let mut h = Sha256::new();
    let mut buf = vec![0u8; 256 * 1024];
    loop {
        let n = f
            .read(&mut buf)
            .map_err(|e| format!("{}: {e}", path.display()))?;
        if n == 0 {
            break;
        }
        h.update(&buf[..n]);
    }
    Ok(hex(&h.finalize()))
}

/// Compare a file's sha256 with the registry's (hex, case-insensitive).
pub fn verify_sha256(path: &Path, expected: &str) -> Result<(), String> {
    let got = sha256_file(path)?;
    if !got.eq_ignore_ascii_case(expected.trim()) {
        return Err(format!(
            "sha256 mismatch for {}: expected {expected}, got {got}",
            path.display()
        ));
    }
    Ok(())
}

/// Extract `zip` under `dest`. Every entry name must resolve to a normal
/// relative path (no `..`, no root, no drive prefix, no symlink entries) or
/// the whole extraction is refused — a partially written `dest` is the
/// caller's to delete. Returns the extracted file paths.
pub fn unzip_guarded(zip: &Path, dest: &Path) -> Result<Vec<PathBuf>, String> {
    let file = fs::File::open(zip).map_err(|e| format!("{}: {e}", zip.display()))?;
    let mut archive =
        zip::ZipArchive::new(file).map_err(|e| format!("{}: not a zip: {e}", zip.display()))?;
    if archive.len() > MAX_ENTRIES {
        return Err(format!("{}: too many entries", zip.display()));
    }
    fs::create_dir_all(dest).map_err(|e| format!("{}: {e}", dest.display()))?;
    let mut written = Vec::new();
    let mut total: u64 = 0;
    for i in 0..archive.len() {
        let mut entry = archive
            .by_index(i)
            .map_err(|e| format!("{}: entry {i}: {e}", zip.display()))?;
        let raw = entry.name().to_string();
        let rel = entry
            .enclosed_name()
            .filter(|p| {
                p.components()
                    .all(|c| matches!(c, Component::Normal(_)))
            })
            .ok_or_else(|| format!("zip-slip: refusing entry {raw:?}"))?;
        if entry.is_symlink() {
            return Err(format!("refusing symlink entry {raw:?}"));
        }
        let out = dest.join(&rel);
        if entry.is_dir() {
            fs::create_dir_all(&out).map_err(|e| format!("{}: {e}", out.display()))?;
            continue;
        }
        total = total.saturating_add(entry.size());
        if total > MAX_UNPACKED_BYTES {
            return Err(format!("{}: unpacked size exceeds cap", zip.display()));
        }
        if let Some(parent) = out.parent() {
            fs::create_dir_all(parent).map_err(|e| format!("{}: {e}", parent.display()))?;
        }
        let mut f = fs::File::create(&out).map_err(|e| format!("{}: {e}", out.display()))?;
        let mut buf = vec![0u8; 256 * 1024];
        let mut left = entry.size();
        loop {
            let n = entry
                .read(&mut buf)
                .map_err(|e| format!("{}: {e}", out.display()))?;
            if n == 0 {
                break;
            }
            // A lying header (declared size < actual stream) must not escape the cap.
            if (n as u64) > left {
                return Err(format!("{}: entry {raw:?} larger than declared", zip.display()));
            }
            left -= n as u64;
            f.write_all(&buf[..n])
                .map_err(|e| format!("{}: {e}", out.display()))?;
        }
        drop(f);
        // Runtime packs (bin/node, bin/java) must stay executable after the
        // unzip — the zip carries the unix mode, std does not apply it.
        #[cfg(unix)]
        if let Some(mode) = entry.unix_mode() {
            use std::os::unix::fs::PermissionsExt;
            let _ = fs::set_permissions(&out, fs::Permissions::from_mode(mode & 0o777));
        }
        written.push(out);
    }
    Ok(written)
}

/// Locate + parse `manifest.json` in an unpacked pack: at the root, or inside
/// a single top-level directory (zips built with a wrapping folder). Returns
/// the pack root, the manifest and its raw JSON (persisted verbatim so unknown
/// fields survive the round-trip).
pub fn read_manifest(dir: &Path) -> Result<(PathBuf, Manifest, String), String> {
    let root = if dir.join("manifest.json").is_file() {
        dir.to_path_buf()
    } else {
        let mut entries: Vec<PathBuf> = fs::read_dir(dir)
            .map_err(|e| format!("{}: {e}", dir.display()))?
            .filter_map(|e| e.ok().map(|e| e.path()))
            .collect();
        match entries.len() {
            1 if entries[0].is_dir() && entries[0].join("manifest.json").is_file() => {
                entries.remove(0)
            }
            _ => return Err(format!("{}: manifest.json not found", dir.display())),
        }
    };
    let raw = fs::read_to_string(root.join("manifest.json"))
        .map_err(|e| format!("{}: manifest.json: {e}", root.display()))?;
    let m = Manifest::parse(&raw)?;
    Ok((root, m, raw))
}

/// One `ext_installed` row.
#[derive(Debug, Clone, PartialEq)]
pub struct InstalledRow {
    pub id: String,
    pub kind: String,
    pub version: String,
    pub target: String,
    pub dir: String,
    /// Grammars: sha256 of the dylib as unpacked (re-checked at every load).
    /// Servers: sha256 of the pack zip.
    pub sha256: String,
    pub enabled: bool,
    pub manifest_json: String,
    pub installed_at: i64,
    /// A newer version unpacked next to a version whose dylib is mapped in
    /// this process — becomes `version` at the next startup.
    pub pending_version: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Activation {
    /// The new row is live now.
    Immediate,
    /// The old row stays live; `pending_version` points at the new dir.
    PendingRestart,
}

/// Decide how a freshly staged `new` version becomes the installed one.
/// `loaded_in_process` = this pack's dylib is currently mapped (any version):
/// its file is locked on Windows and its `Language` pointers are live, so the
/// old dir must stay and the swap waits for a restart.
pub fn apply_install(
    current: Option<InstalledRow>,
    new: InstalledRow,
    loaded_in_process: bool,
) -> (InstalledRow, Activation) {
    match current {
        Some(mut row) if loaded_in_process => {
            row.pending_version = Some(new.version);
            (row, Activation::PendingRestart)
        }
        _ => (new, Activation::Immediate),
    }
}

/// Startup: promote `pending_version` (if any) to the live version. The dir
/// is the sibling `<parent>/<pending>`; its manifest is re-read so the row
/// describes what is actually on disk. A pending dir that vanished (manual
/// deletion) just clears the flag and keeps the old version.
pub fn activate_pending(row: InstalledRow) -> InstalledRow {
    let Some(pending) = row.pending_version.clone() else {
        return row;
    };
    let mut out = row;
    out.pending_version = None;
    let new_dir = Path::new(&out.dir)
        .parent()
        .map(|p| p.join(&pending))
        .unwrap_or_else(|| PathBuf::from(&pending));
    match read_manifest(&new_dir) {
        Ok((root, m, raw)) if m.version() == pending => {
            out.version = pending;
            out.dir = root.to_string_lossy().into_owned();
            out.manifest_json = raw;
            if let Manifest::Grammar(g) = &m {
                if let Ok(sha) = sha256_file(&root.join(&g.library)) {
                    out.sha256 = sha;
                }
            }
        }
        Ok(_) => log::warn!(
            "extensions: pending {} {pending}: manifest version mismatch — keeping {}",
            out.id,
            out.version
        ),
        Err(e) => log::warn!(
            "extensions: pending {} {pending} unreadable ({e}) — keeping {}",
            out.id,
            out.version
        ),
    }
    out
}

#[cfg(test)]
pub(crate) mod testutil {
    use std::io::Write;
    use std::path::Path;

    /// Build a zip at `path` from `(entry name, bytes)` pairs. Entry names are
    /// written verbatim (so `../x` really lands in the archive).
    pub fn write_zip(path: &Path, entries: &[(&str, &[u8])]) {
        let f = std::fs::File::create(path).unwrap();
        let mut w = zip::ZipWriter::new(f);
        let opts = zip::write::SimpleFileOptions::default()
            .compression_method(zip::CompressionMethod::Deflated);
        for (name, bytes) in entries {
            if name.ends_with('/') {
                w.add_directory(*name, opts).unwrap();
            } else {
                w.start_file(*name, opts).unwrap();
                w.write_all(bytes).unwrap();
            }
        }
        w.finish().unwrap();
    }
}

#[cfg(test)]
mod tests {
    use super::testutil::write_zip;
    use super::*;
    use crate::extensions::manifest::fixtures;

    #[test]
    fn sha256_hex_matches_known_vector() {
        assert_eq!(
            sha256_hex(b"abc"),
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad"
        );
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        fs::write(&p, b"abc").unwrap();
        assert_eq!(sha256_file(&p).unwrap(), sha256_hex(b"abc"));
    }

    #[test]
    fn verify_sha256_pass_and_fail() {
        let dir = tempfile::tempdir().unwrap();
        let p = dir.path().join("f");
        fs::write(&p, b"payload").unwrap();
        let ok = sha256_hex(b"payload");
        verify_sha256(&p, &ok).unwrap();
        verify_sha256(&p, &ok.to_uppercase()).unwrap();
        let err = verify_sha256(&p, &"0".repeat(64)).unwrap_err();
        assert!(err.contains("mismatch"), "{err}");
        assert!(verify_sha256(&dir.path().join("nope"), &ok).is_err());
    }

    #[test]
    fn unzip_extracts_nested_entries() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("p.zip");
        write_zip(
            &zip,
            &[
                ("manifest.json", fixtures::GRAMMAR.as_bytes()),
                ("queries/", b""),
                ("queries/tags.scm", b"(class_declaration) @definition.class"),
                ("tree_sitter_java.dll", b"not really"),
            ],
        );
        let out = dir.path().join("out");
        let files = unzip_guarded(&zip, &out).unwrap();
        assert_eq!(files.len(), 3);
        assert_eq!(
            fs::read_to_string(out.join("queries/tags.scm")).unwrap(),
            "(class_declaration) @definition.class"
        );
        let (root, m, raw) = read_manifest(&out).unwrap();
        assert_eq!(root, out);
        assert_eq!(m.id(), "grammar.java");
        assert!(raw.contains("futureKey"), "raw json kept verbatim");
    }

    #[test]
    fn unzip_rejects_zip_slip_and_absolute_entries() {
        for bad in ["../evil.txt", "a/../../evil.txt", "/abs.txt", "C:/abs.txt"] {
            let dir = tempfile::tempdir().unwrap();
            let zip = dir.path().join("p.zip");
            write_zip(&zip, &[("ok.txt", b"fine"), (bad, b"pwned")]);
            let out = dir.path().join("out");
            let err = unzip_guarded(&zip, &out).unwrap_err();
            assert!(
                err.contains("zip-slip") || err.contains("refusing"),
                "{bad}: {err}"
            );
            assert!(!dir.path().join("evil.txt").exists());
        }
    }

    #[test]
    fn unzip_refuses_garbage() {
        let dir = tempfile::tempdir().unwrap();
        let zip = dir.path().join("p.zip");
        fs::write(&zip, b"definitely not a zip").unwrap();
        assert!(unzip_guarded(&zip, &dir.path().join("out")).is_err());
    }

    #[test]
    fn read_manifest_accepts_single_wrapping_dir_only() {
        let dir = tempfile::tempdir().unwrap();
        let inner = dir.path().join("grammar.java-0.23.4-1");
        fs::create_dir_all(&inner).unwrap();
        fs::write(inner.join("manifest.json"), fixtures::GRAMMAR).unwrap();
        let (root, _, _) = read_manifest(dir.path()).unwrap();
        assert_eq!(root, inner);
        // two top-level dirs → ambiguous → error
        fs::create_dir_all(dir.path().join("other")).unwrap();
        assert!(read_manifest(dir.path()).is_err());
    }

    fn row(version: &str) -> InstalledRow {
        InstalledRow {
            id: "grammar.java".into(),
            kind: "grammar".into(),
            version: version.into(),
            target: "windows-x86_64".into(),
            dir: format!("C:/x/extensions/grammar.java/{version}"),
            sha256: "0".repeat(64),
            enabled: true,
            manifest_json: "{}".into(),
            installed_at: 1,
            pending_version: None,
        }
    }

    #[test]
    fn fresh_install_and_unloaded_upgrade_activate_immediately() {
        let (r, a) = apply_install(None, row("0.23.4-1"), false);
        assert_eq!(a, Activation::Immediate);
        assert_eq!(r.version, "0.23.4-1");
        let (r, a) = apply_install(Some(row("0.23.4-1")), row("0.23.4-2"), false);
        assert_eq!(a, Activation::Immediate, "nothing mapped → swap now");
        assert_eq!(r.version, "0.23.4-2");
        assert_eq!(r.pending_version, None);
        // no current row but "loaded" (stale registry entry) → still immediate
        let (_, a) = apply_install(None, row("0.23.4-2"), true);
        assert_eq!(a, Activation::Immediate);
    }

    #[test]
    fn upgrade_of_a_mapped_grammar_goes_pending() {
        let (r, a) = apply_install(Some(row("0.23.4-1")), row("0.23.4-2"), true);
        assert_eq!(a, Activation::PendingRestart);
        assert_eq!(r.version, "0.23.4-1", "old stays live");
        assert_eq!(r.pending_version.as_deref(), Some("0.23.4-2"));
        assert!(r.dir.ends_with("0.23.4-1"));
    }

    #[test]
    fn activate_pending_promotes_from_disk() {
        let tmp = tempfile::tempdir().unwrap();
        let base = tmp.path().join("grammar.java");
        let old = base.join("0.23.4-1");
        let new = base.join("0.23.4-2");
        fs::create_dir_all(&old).unwrap();
        fs::create_dir_all(&new).unwrap();
        let manifest = fixtures::GRAMMAR.replace("0.23.4-1", "0.23.4-2");
        fs::write(new.join("manifest.json"), &manifest).unwrap();
        fs::write(new.join("tree_sitter_java.dll"), b"dll").unwrap();
        let mut r = row("0.23.4-1");
        r.dir = old.to_string_lossy().into_owned();
        r.pending_version = Some("0.23.4-2".into());
        let out = activate_pending(r);
        assert_eq!(out.version, "0.23.4-2");
        assert_eq!(out.pending_version, None);
        assert_eq!(Path::new(&out.dir), new);
        assert_eq!(out.sha256, sha256_hex(b"dll"));
        assert!(out.manifest_json.contains("0.23.4-2"));
        // idempotent: no pending → unchanged
        assert_eq!(activate_pending(out.clone()), out);
    }

    #[test]
    fn activate_pending_keeps_old_when_new_dir_is_missing_or_wrong() {
        let tmp = tempfile::tempdir().unwrap();
        let old = tmp.path().join("grammar.java").join("0.23.4-1");
        fs::create_dir_all(&old).unwrap();
        let mut r = row("0.23.4-1");
        r.dir = old.to_string_lossy().into_owned();
        r.pending_version = Some("0.23.4-2".into());
        let out = activate_pending(r.clone());
        assert_eq!(out.version, "0.23.4-1");
        assert_eq!(out.pending_version, None, "flag cleared even on failure");
        // dir exists but manifest says another version → keep old
        let new = tmp.path().join("grammar.java").join("0.23.4-2");
        fs::create_dir_all(&new).unwrap();
        fs::write(new.join("manifest.json"), fixtures::GRAMMAR).unwrap();
        let out = activate_pending(r);
        assert_eq!(out.version, "0.23.4-1");
    }
}
