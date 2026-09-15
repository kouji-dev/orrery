//! `file://` URI ↔ path, hand-rolled: `lsp-types` 0.97 carries a plain
//! `Uri` (no file-path helpers) and Windows drive letters need the
//! `file:///C:/...` shape every server understands.

use std::path::{Path, PathBuf};

fn is_unreserved(b: u8) -> bool {
    b.is_ascii_alphanumeric() || matches!(b, b'-' | b'.' | b'_' | b'~')
}

/// Percent-encode a path segment string, keeping `/` and `:` (drive) intact.
fn encode(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for b in s.bytes() {
        if is_unreserved(b) || b == b'/' || b == b':' {
            out.push(b as char);
        } else {
            out.push_str(&format!("%{b:02X}"));
        }
    }
    out
}

fn decode(s: &str) -> String {
    let bytes = s.as_bytes();
    let mut out = Vec::with_capacity(bytes.len());
    let mut i = 0;
    while i < bytes.len() {
        if bytes[i] == b'%' && i + 2 < bytes.len() {
            if let (Some(h), Some(l)) = (hex(bytes.get(i + 1)), hex(bytes.get(i + 2))) {
                out.push(h << 4 | l);
                i += 3;
                continue;
            }
        }
        out.push(bytes[i]);
        i += 1;
    }
    String::from_utf8_lossy(&out).into_owned()
}

fn hex(b: Option<&u8>) -> Option<u8> {
    (*b? as char).to_digit(16).map(|d| d as u8)
}

/// Absolute path → `file:///C:/dir/a%20b.java` / `file:///home/x/a.rs`.
pub fn path_to_uri(p: &Path) -> String {
    let s = p.to_string_lossy().replace('\\', "/");
    let s = s.strip_prefix("//?/").unwrap_or(&s); // verbatim prefix
    if s.starts_with('/') {
        format!("file://{}", encode(s))
    } else {
        format!("file:///{}", encode(s))
    }
}

/// `file://` URI → absolute path (`None` for any other scheme).
pub fn uri_to_path(uri: &str) -> Option<PathBuf> {
    let rest = uri.strip_prefix("file://")?;
    // strip an authority (`file://localhost/...`) — rare, tolerated
    let rest = if let Some(r) = rest.strip_prefix('/') {
        format!("/{r}")
    } else if let Some(idx) = rest.find('/') {
        rest[idx..].to_string()
    } else {
        return None;
    };
    let decoded = decode(&rest);
    // `/C:/x` → `C:/x`
    let bytes = decoded.as_bytes();
    let path = if bytes.len() >= 3
        && bytes[0] == b'/'
        && bytes[1].is_ascii_alphabetic()
        && bytes[2] == b':'
    {
        decoded[1..].to_string()
    } else {
        decoded
    };
    Some(PathBuf::from(if cfg!(windows) { path.replace('/', "\\") } else { path }))
}

/// Root-relative forward-slash path of `abs` under `root` (`None` outside).
pub fn relative_to(root: &Path, abs: &Path) -> Option<String> {
    let norm = |p: &Path| {
        let s = p.to_string_lossy().replace('\\', "/");
        s.trim_end_matches('/').to_string()
    };
    let (r, a) = (norm(root), norm(abs));
    let (r_cmp, a_cmp) = if cfg!(windows) {
        (r.to_ascii_lowercase(), a.to_ascii_lowercase())
    } else {
        (r.clone(), a.clone())
    };
    if a_cmp == r_cmp {
        return Some(String::new());
    }
    let rest = a_cmp.strip_prefix(&r_cmp)?;
    if !rest.starts_with('/') {
        return None;
    }
    Some(a[r.len() + 1..].to_string())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn windows_and_posix_round_trip() {
        let uri = path_to_uri(Path::new("C:\\Users\\me\\proj\\src\\A b.java"));
        assert_eq!(uri, "file:///C:/Users/me/proj/src/A%20b.java");
        let back = uri_to_path(&uri).unwrap();
        assert_eq!(back.to_string_lossy().replace('\\', "/"), "C:/Users/me/proj/src/A b.java");
        let uri = path_to_uri(Path::new("/home/x/a#1.rs"));
        assert_eq!(uri, "file:///home/x/a%231.rs");
        assert_eq!(
            uri_to_path(&uri).unwrap().to_string_lossy().replace('\\', "/"),
            "/home/x/a#1.rs"
        );
        assert!(uri_to_path("jdt://contents/rt.jar/java.lang/String.class").is_none());
        // lower-case drive letters and `file://localhost/` are accepted
        assert!(uri_to_path("file:///c%3A/x/y.rs").unwrap().to_string_lossy().starts_with("c:"));
        assert!(uri_to_path("file://localhost/etc/hosts").is_some());
    }

    #[test]
    fn relative_to_root() {
        let root = Path::new("C:/w/proj");
        assert_eq!(relative_to(root, Path::new("C:\\w\\proj\\src\\a.rs")).as_deref(), Some("src/a.rs"));
        assert_eq!(relative_to(root, Path::new("C:/w/proj")).as_deref(), Some(""));
        assert_eq!(relative_to(root, Path::new("C:/w/proj2/a.rs")), None);
        assert_eq!(relative_to(root, Path::new("D:/other/a.rs")), None);
    }
}
