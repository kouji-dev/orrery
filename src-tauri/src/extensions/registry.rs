//! The extension registry: where `index.json` lives, which artifact fits this
//! machine, and the two gates (`minOrreryVersion`, grammar `abi`) that decide
//! whether a pack is offered at all. Network access sits behind [`Fetcher`] so
//! everything above it is testable without a socket.

use std::cmp::Ordering;
use std::collections::BTreeMap;
use std::time::Duration;

use super::loader;
use super::manifest::RegistryTarget;

/// Rolling `extensions` release in the releases repo — a stable URL like
/// `latest.json`, re-uploaded by every publish run.
pub const DEFAULT_REGISTRY_URL: &str =
    "https://github.com/kouji-dev/orrery-releases/releases/download/extensions/index.json";
/// Cached index age after which a list call re-fetches (unless offline).
pub const CACHE_TTL_SECS: i64 = 6 * 60 * 60;
/// Target key for platform-independent packs (server manifests).
pub const TARGET_ANY: &str = "any";
/// Index documents above this are refused (a stray HTML page, not a catalog).
pub const MAX_INDEX_BYTES: u64 = 8 * 1024 * 1024;
/// Pack artifacts above this are refused before download.
pub const MAX_ARTIFACT_BYTES: u64 = 512 * 1024 * 1024;

/// This machine's target key (`windows-x86_64`, `macos-aarch64`, …).
pub fn target_key() -> String {
    target_key_for(std::env::consts::OS, std::env::consts::ARCH)
}

pub fn target_key_for(os: &str, arch: &str) -> String {
    match (os, arch) {
        ("macos", "aarch64") => "macos-aarch64".into(),
        ("windows", "x86_64") => "windows-x86_64".into(),
        _ => format!("{os}-{arch}"),
    }
}

/// The artifact for `target`: an exact match first, else the `any` entry.
pub fn pick_target<'a>(
    targets: &'a BTreeMap<String, RegistryTarget>,
    target: &str,
) -> Option<(&'a str, &'a RegistryTarget)> {
    targets
        .get_key_value(target)
        .or_else(|| targets.get_key_value(TARGET_ANY))
        .map(|(k, v)| (k.as_str(), v))
}

// ---- versions ----

/// Pack versions are `<upstream semver-ish>[-<rev>]` where `-N` is the PACK
/// revision (a rebuild of the same upstream): `0.23.4-2` > `0.23.4-1` >
/// `0.23.4`. A non-numeric suffix is treated as a pre-release (`< 0.23.4`).
#[derive(Debug, PartialEq, Eq)]
struct Parsed {
    core: Vec<u64>,
    rev: Rev,
}

#[derive(Debug, PartialEq, Eq, PartialOrd, Ord)]
enum Rev {
    Pre(String),
    None,
    Pack(u64),
}

fn parse_version(s: &str) -> Parsed {
    let s = s.trim().trim_start_matches(['v', 'V']);
    let (core, suffix) = match s.split_once('-') {
        Some((c, r)) => (c, Some(r)),
        None => (s, None),
    };
    let core = core
        .split('.')
        .map(|p| p.trim().parse::<u64>().unwrap_or(0))
        .collect();
    let rev = match suffix {
        None | Some("") => Rev::None,
        Some(r) => r
            .parse::<u64>()
            .map(Rev::Pack)
            .unwrap_or_else(|_| Rev::Pre(r.to_string())),
    };
    Parsed { core, rev }
}

/// Order two pack/app versions (see [`Parsed`]). Missing core parts count as 0.
pub fn compare_versions(a: &str, b: &str) -> Ordering {
    let (pa, pb) = (parse_version(a), parse_version(b));
    let n = pa.core.len().max(pb.core.len());
    for i in 0..n {
        let (x, y) = (
            pa.core.get(i).copied().unwrap_or(0),
            pb.core.get(i).copied().unwrap_or(0),
        );
        match x.cmp(&y) {
            Ordering::Equal => continue,
            o => return o,
        }
    }
    pa.rev.cmp(&pb.rev)
}

/// DEBUG builds only: the registry a local `pnpm ext:build:all` produced
/// (`<repo>/dist-ext/index.json`, next to `src-tauri/`) — so `tauri dev`
/// lists and installs the packs being developed without any setting. None
/// when the file is absent or in release builds.
pub fn local_dev_registry_url() -> Option<String> {
    if !dev_conveniences() {
        return None;
    }
    let p = std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("..").join("dist-ext").join("index.json");
    let abs = p.canonicalize().ok()?;
    Some(file_url_for(&abs))
}

/// A canonical path as a `file:///` URL: backslashes become slashes, the
/// verbatim prefix canonicalize adds on Windows is dropped, and posix paths
/// keep their leading slash.
pub fn file_url_for(path: &std::path::Path) -> String {
    let s = path.to_string_lossy().replace('\\', "/");
    let s = s.strip_prefix("//?/").unwrap_or(&s);
    if s.starts_with('/') {
        format!("file://{s}")
    } else {
        format!("file:///{s}")
    }
}

/// The gate the host applies: `orrery_version_ok`, except that `tauri dev`
/// accepts every pack — it runs as the unreleased version (0.23.1 today) while
/// freshly built packs already declare the release they ship with (0.24.0),
/// and dev is exactly where those packs get exercised.
pub fn app_version_ok(min: Option<&str>, current: &str) -> bool {
    dev_conveniences() || orrery_version_ok(min, current)
}

/// `tauri dev` only: debug build, but NOT the test harness (which also runs
/// with debug assertions and must see release semantics).
fn dev_conveniences() -> bool {
    cfg!(debug_assertions) && !cfg!(test)
}

/// `minOrreryVersion` gate. Compares release triples only, so a `0.24.0-beta.1`
/// build still accepts packs that need `0.24.0` (the pre-release IS that
/// version under test). Unparseable inputs fall back to [`compare_versions`].
pub fn orrery_version_ok(min: Option<&str>, current: &str) -> bool {
    let Some(min) = min.map(str::trim).filter(|m| !m.is_empty()) else {
        return true;
    };
    match (
        semver::Version::parse(current.trim_start_matches('v')),
        semver::Version::parse(min.trim_start_matches('v')),
    ) {
        (Ok(cur), Ok(min)) => {
            semver::Version::new(cur.major, cur.minor, cur.patch)
                >= semver::Version::new(min.major, min.minor, min.patch)
        }
        _ => compare_versions(current, min) != Ordering::Less,
    }
}

/// Grammar ABI gate. `None` = not a grammar (servers carry no ABI).
pub fn abi_ok(abi: Option<u32>) -> bool {
    abi.is_none_or(|a| loader::abi_compatible(a as usize))
}

/// `https://` always; `file://` only in debug builds (local `pnpm ext:build`
/// output). Anything else is refused before a single byte moves.
pub fn validate_registry_url(url: &str) -> Result<(), String> {
    if url.starts_with("https://") {
        return Ok(());
    }
    if url.starts_with("file://") {
        if cfg!(debug_assertions) {
            return Ok(());
        }
        return Err("extensions: file:// registry URLs are only allowed in debug builds".into());
    }
    Err(format!("extensions: registry URL must be https:// (got {url:?})"))
}

// ---- fetching ----

/// `sink(chunk, total)` — receives a body in order; `total` is the content
/// length when the server sent one.
pub type Sink<'a> = dyn FnMut(&[u8], Option<u64>) -> Result<(), String> + 'a;
type SendSink<'a> = dyn FnMut(&[u8], Option<u64>) -> Result<(), String> + Send + 'a;

/// Streams a URL into a [`Sink`].
pub trait Fetcher: Send + Sync {
    fn get(&self, url: &str, sink: &mut Sink<'_>) -> Result<(), String>;
}

/// Whole body in memory, refused past `max` bytes.
pub fn fetch_bytes(f: &dyn Fetcher, url: &str, max: u64) -> Result<Vec<u8>, String> {
    let mut buf = Vec::new();
    f.get(url, &mut |chunk, total| {
        if total.is_some_and(|t| t > max) || (buf.len() as u64 + chunk.len() as u64) > max {
            return Err(format!("extensions: {url} exceeds {max} bytes"));
        }
        buf.extend_from_slice(chunk);
        Ok(())
    })?;
    Ok(buf)
}

/// The production fetcher: reqwest over rustls (same stack as the updater),
/// plus `file://` in debug builds. Runs the async client on a scoped thread
/// via the app runtime so it is safe to call from any thread, including
/// tauri's blocking-command pool.
pub struct HttpFetcher;

impl HttpFetcher {
    fn get_file(url: &str, sink: &mut Sink<'_>) -> Result<(), String> {
        let path = tauri::Url::parse(url)
            .ok()
            .and_then(|u| u.to_file_path().ok())
            .ok_or_else(|| format!("extensions: bad file url {url:?}"))?;
        let bytes = std::fs::read(&path).map_err(|e| format!("{}: {e}", path.display()))?;
        sink(&bytes, Some(bytes.len() as u64))
    }

    async fn get_https(url: &str, sink: &mut SendSink<'_>) -> Result<(), String> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("orrery/", env!("CARGO_PKG_VERSION")))
            .connect_timeout(Duration::from_secs(15))
            .read_timeout(Duration::from_secs(60))
            .build()
            .map_err(|e| format!("extensions: http client: {e}"))?;
        let mut resp = client
            .get(url)
            .send()
            .await
            .map_err(|e| format!("extensions: {url}: {e}"))?;
        if !resp.status().is_success() {
            return Err(format!("extensions: {url}: HTTP {}", resp.status()));
        }
        let total = resp.content_length();
        while let Some(chunk) = resp
            .chunk()
            .await
            .map_err(|e| format!("extensions: {url}: {e}"))?
        {
            sink(&chunk, total)?;
        }
        Ok(())
    }
}

impl Fetcher for HttpFetcher {
    fn get(&self, url: &str, sink: &mut Sink<'_>) -> Result<(), String> {
        if url.starts_with("file://") {
            if !cfg!(debug_assertions) {
                return Err("extensions: file:// only in debug builds".into());
            }
            return Self::get_file(url, sink);
        }
        if !url.starts_with("https://") {
            return Err(format!("extensions: refusing non-https url {url:?}"));
        }
        // Buffer chunks through a channel so the sink (which may touch the
        // caller's non-Send state) stays on this thread.
        let (tx, rx) = std::sync::mpsc::channel::<(Vec<u8>, Option<u64>)>();
        let url_owned = url.to_string();
        std::thread::scope(|s| {
            let worker = s.spawn(move || {
                let mut push = |chunk: &[u8], total: Option<u64>| {
                    tx.send((chunk.to_vec(), total))
                        .map_err(|_| "extensions: download aborted".to_string())
                };
                tauri::async_runtime::block_on(Self::get_https(&url_owned, &mut push))
            });
            let mut sink_err = None;
            for (chunk, total) in rx {
                if sink_err.is_none() {
                    if let Err(e) = sink(&chunk, total) {
                        sink_err = Some(e);
                        // keep draining so the worker never blocks on send
                    }
                }
            }
            let net = worker
                .join()
                .unwrap_or_else(|_| Err("extensions: download thread panicked".into()));
            match (sink_err, net) {
                (Some(e), _) => Err(e),
                (None, r) => r,
            }
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn targets(keys: &[&str]) -> BTreeMap<String, RegistryTarget> {
        keys.iter()
            .map(|k| {
                (
                    k.to_string(),
                    RegistryTarget {
                        url: format!("https://x/{k}.zip"),
                        sha256: "0".repeat(64),
                        size: 1,
                    },
                )
            })
            .collect()
    }

    #[test]
    fn target_key_maps_the_two_shipped_platforms() {
        assert_eq!(target_key_for("windows", "x86_64"), "windows-x86_64");
        assert_eq!(target_key_for("macos", "aarch64"), "macos-aarch64");
        assert_eq!(target_key_for("linux", "x86_64"), "linux-x86_64");
        assert!(!target_key().is_empty());
    }

    #[test]
    fn pick_target_prefers_exact_then_any() {
        let t = targets(&["windows-x86_64", "any"]);
        assert_eq!(pick_target(&t, "windows-x86_64").unwrap().0, "windows-x86_64");
        assert_eq!(pick_target(&t, "macos-aarch64").unwrap().0, "any");
        let only_win = targets(&["windows-x86_64"]);
        assert!(pick_target(&only_win, "macos-aarch64").is_none());
        assert!(pick_target(&BTreeMap::new(), "windows-x86_64").is_none());
    }

    #[test]
    fn version_compare_handles_pack_revisions() {
        use Ordering::*;
        assert_eq!(compare_versions("0.23.4-2", "0.23.4-1"), Greater);
        assert_eq!(compare_versions("0.23.4-1", "0.23.4"), Greater, "rev beats bare");
        assert_eq!(compare_versions("0.23.4", "0.23.4-beta"), Greater, "pre-release is lower");
        assert_eq!(compare_versions("0.23.10", "0.23.9"), Greater, "numeric, not lexical");
        assert_eq!(compare_versions("1.0", "1.0.0"), Equal);
        assert_eq!(compare_versions("v1.40.0-1", "1.40.0-1"), Equal);
        assert_eq!(compare_versions("0.9.0", "0.23.0"), Less);
        assert_eq!(compare_versions("1.40.0-10", "1.40.0-9"), Greater);
    }

    #[test]
    fn min_orrery_version_gate() {
        assert!(orrery_version_ok(None, "0.23.1"));
        assert!(orrery_version_ok(Some(""), "0.23.1"));
        assert!(orrery_version_ok(Some("0.24.0"), "0.24.0"));
        assert!(orrery_version_ok(Some("0.24.0"), "0.25.3"));
        assert!(!orrery_version_ok(Some("0.24.0"), "0.23.1"));
        assert!(
            orrery_version_ok(Some("0.24.0"), "0.24.0-beta.1"),
            "a pre-release of the required version counts"
        );
        assert!(!orrery_version_ok(Some("0.24.0"), "0.23.9-beta.1"));
        assert!(orrery_version_ok(Some("0.24"), "0.24.1"), "loose fallback");
    }

    #[test]
    fn abi_gate_uses_the_linked_runtime_range() {
        assert!(abi_ok(None), "servers have no abi");
        assert!(abi_ok(Some(tree_sitter::LANGUAGE_VERSION as u32)));
        assert!(abi_ok(Some(
            tree_sitter::MIN_COMPATIBLE_LANGUAGE_VERSION as u32
        )));
        assert!(!abi_ok(Some(tree_sitter::LANGUAGE_VERSION as u32 + 1)));
        assert!(!abi_ok(Some(1)));
    }

    #[test]
    fn registry_url_scheme_rules() {
        assert!(validate_registry_url(DEFAULT_REGISTRY_URL).is_ok());
        assert_eq!(file_url_for(std::path::Path::new(r"\\?\C:\r\dist-ext\index.json")), "file:///C:/r/dist-ext/index.json");
        assert_eq!(file_url_for(std::path::Path::new("/home/u/r/dist-ext/index.json")), "file:///home/u/r/dist-ext/index.json");
        assert!(validate_registry_url("http://x/index.json").is_err());
        assert!(validate_registry_url("ftp://x").is_err());
        assert_eq!(
            validate_registry_url("file:///C:/x/index.json").is_ok(),
            cfg!(debug_assertions)
        );
    }

    struct MemFetcher(Vec<u8>);
    impl Fetcher for MemFetcher {
        fn get(&self, _url: &str, sink: &mut Sink<'_>) -> Result<(), String> {
            for c in self.0.chunks(3) {
                sink(c, Some(self.0.len() as u64))?;
            }
            Ok(())
        }
    }

    #[test]
    fn fetch_bytes_reassembles_and_caps() {
        let f = MemFetcher(b"hello world".to_vec());
        assert_eq!(fetch_bytes(&f, "https://x", 1024).unwrap(), b"hello world");
        assert!(fetch_bytes(&f, "https://x", 5).is_err(), "over the cap");
    }

    #[test]
    fn http_fetcher_refuses_plain_http_without_touching_the_network() {
        let err = HttpFetcher
            .get("http://127.0.0.1:9/x", &mut |_, _| Ok(()))
            .unwrap_err();
        assert!(err.contains("non-https"), "{err}");
    }
}
