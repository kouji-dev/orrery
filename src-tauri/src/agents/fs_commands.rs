//! Agent-scoped worktree file operations (B1.1): write / create / rename /
//! delete. All paths are worktree-relative; `safe_join` rejects absolute
//! paths, drive prefixes, and `..` so a command can never escape the agent's
//! worktree. Writes never touch the git index (unlike `conflict_resolve`) —
//! the FS watcher picks the change up and pushes state to the frontend.

use std::path::{Component, Path, PathBuf};

use tauri::State;
use uuid::Uuid;

use crate::core::errors::{AppError, AppResult};

use super::service::AgentService;

/// Join `rel` under `workdir`, refusing anything that could resolve outside it.
fn safe_join(workdir: &Path, rel: &str) -> AppResult<PathBuf> {
    let rel = rel.trim();
    if rel.is_empty() {
        return Err(AppError::Other("empty path".into()));
    }
    let p = Path::new(rel);
    if p.is_absolute() {
        return Err(AppError::Other(format!("absolute path rejected: {rel}")));
    }
    for c in p.components() {
        match c {
            Component::Normal(_) | Component::CurDir => {}
            _ => return Err(AppError::Other(format!("unsafe path component in '{rel}'"))),
        }
    }
    Ok(workdir.join(p))
}

// ---- pure fs ops (unit-tested against a plain tempdir, no service needed) ----

pub(crate) fn write_file(workdir: &Path, rel: &str, content: &str) -> AppResult<()> {
    let abs = safe_join(workdir, rel)?;
    if abs.is_dir() {
        return Err(AppError::Other(format!("'{rel}' is a directory")));
    }
    if let Some(parent) = abs.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&abs, content).map_err(|e| AppError::Other(format!("write '{rel}': {e}")))
}

pub(crate) fn create_file(workdir: &Path, rel: &str) -> AppResult<()> {
    let abs = safe_join(workdir, rel)?;
    if abs.exists() {
        return Err(AppError::Other(format!("'{rel}' already exists")));
    }
    if let Some(parent) = abs.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::write(&abs, "").map_err(|e| AppError::Other(format!("create '{rel}': {e}")))
}

pub(crate) fn create_dir(workdir: &Path, rel: &str) -> AppResult<()> {
    let abs = safe_join(workdir, rel)?;
    if abs.exists() {
        return Err(AppError::Other(format!("'{rel}' already exists")));
    }
    std::fs::create_dir_all(&abs).map_err(|e| AppError::Other(format!("mkdir '{rel}': {e}")))
}

pub(crate) fn rename_path(workdir: &Path, from: &str, to: &str) -> AppResult<()> {
    let src = safe_join(workdir, from)?;
    let dst = safe_join(workdir, to)?;
    if !src.exists() {
        return Err(AppError::Other(format!("'{from}' does not exist")));
    }
    // Renaming only a change of case (README.md → readme.md) hits the same
    // path on Windows' case-insensitive FS — that's the one legal "dst exists".
    if dst.exists() && !paths_equal_ci(&src, &dst) {
        return Err(AppError::Other(format!("'{to}' already exists")));
    }
    if let Some(parent) = dst.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    std::fs::rename(&src, &dst)
        .map_err(|e| AppError::Other(format!("rename '{from}' → '{to}': {e}")))
}

pub(crate) fn delete_path(workdir: &Path, rel: &str) -> AppResult<()> {
    let abs = safe_join(workdir, rel)?;
    if abs.is_dir() {
        std::fs::remove_dir_all(&abs).map_err(|e| AppError::Other(format!("delete '{rel}': {e}")))
    } else if abs.exists() {
        std::fs::remove_file(&abs).map_err(|e| AppError::Other(format!("delete '{rel}': {e}")))
    } else {
        Err(AppError::Other(format!("'{rel}' does not exist")))
    }
}

fn paths_equal_ci(a: &Path, b: &Path) -> bool {
    a.to_string_lossy().eq_ignore_ascii_case(&b.to_string_lossy())
}

// ---- binary read (B1.4 image/PDF preview) ----

/** Refuse to shovel arbitrarily large blobs through the IPC as base64. */
const MAX_BINARY_BYTES: u64 = 25 * 1024 * 1024;

#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct BinaryFile {
    pub base64: String,
    pub mime: String,
    pub len: u64,
}

fn mime_of(rel: &str) -> &'static str {
    let ext = rel.rsplit('.').next().unwrap_or("").to_ascii_lowercase();
    match ext.as_str() {
        "png" => "image/png",
        "jpg" | "jpeg" => "image/jpeg",
        "gif" => "image/gif",
        "webp" => "image/webp",
        "bmp" => "image/bmp",
        "ico" => "image/x-icon",
        "avif" => "image/avif",
        "svg" => "image/svg+xml",
        "pdf" => "application/pdf",
        _ => "application/octet-stream",
    }
}

pub(crate) fn read_binary(workdir: &Path, rel: &str) -> AppResult<BinaryFile> {
    let abs = safe_join(workdir, rel)?;
    let meta = std::fs::metadata(&abs).map_err(|e| AppError::Other(format!("stat '{rel}': {e}")))?;
    if !meta.is_file() {
        return Err(AppError::Other(format!("'{rel}' is not a file")));
    }
    if meta.len() > MAX_BINARY_BYTES {
        return Err(AppError::Other(format!(
            "'{rel}' is too large to preview ({} MB)",
            meta.len() / (1024 * 1024)
        )));
    }
    let bytes = std::fs::read(&abs).map_err(|e| AppError::Other(format!("read '{rel}': {e}")))?;
    use base64::Engine as _;
    Ok(BinaryFile {
        base64: base64::engine::general_purpose::STANDARD.encode(&bytes),
        mime: mime_of(rel).to_string(),
        len: meta.len(),
    })
}

// ---- commands (resolve the agent's worktree, then run on the blocking pool) ----

fn worktree_of(svc: &AgentService, id: Uuid) -> AppResult<PathBuf> {
    Ok(PathBuf::from(svc.get(id)?.worktree))
}

#[tauri::command]
pub async fn file_write(
    svc: State<'_, AgentService>,
    id: Uuid,
    path: String,
    content: String,
) -> AppResult<()> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("file_write", || {
            write_file(&worktree_of(&svc, id)?, &path, &content)
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn file_create(
    svc: State<'_, AgentService>,
    id: Uuid,
    path: String,
) -> AppResult<()> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("file_create", || create_file(&worktree_of(&svc, id)?, &path))
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn dir_create(svc: State<'_, AgentService>, id: Uuid, path: String) -> AppResult<()> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("dir_create", || create_dir(&worktree_of(&svc, id)?, &path))
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn file_rename(
    svc: State<'_, AgentService>,
    id: Uuid,
    from: String,
    to: String,
) -> AppResult<()> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("file_rename", || {
            rename_path(&worktree_of(&svc, id)?, &from, &to)
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn file_read_binary(
    svc: State<'_, AgentService>,
    id: Uuid,
    path: String,
) -> AppResult<BinaryFile> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("file_read_binary", || {
            read_binary(&worktree_of(&svc, id)?, &path)
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[tauri::command]
pub async fn file_delete(svc: State<'_, AgentService>, id: Uuid, path: String) -> AppResult<()> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("file_delete", || delete_path(&worktree_of(&svc, id)?, &path))
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

// ---- hand off to the OS (open with the default app / reveal in the file manager) ----

/// Resolve a worktree-relative path to the absolute path the OS needs, refusing
/// escapes (`safe_join`) and paths that no longer exist — handing a stale path to
/// the shell would either do nothing or, worse, open the wrong thing.
fn os_target(workdir: &Path, rel: &str) -> AppResult<String> {
    let abs = safe_join(workdir, rel)?;
    if !abs.exists() {
        return Err(AppError::Other(format!("'{rel}' does not exist")));
    }
    Ok(abs.to_string_lossy().into_owned())
}

/// Open a worktree file (or folder) in whatever app the OS associates with it —
/// an `.html` in the default browser, a `.png` in the image viewer, and so on.
/// Directories open in the file manager.
#[tauri::command]
pub async fn file_open_external(
    app: tauri::AppHandle,
    svc: State<'_, AgentService>,
    id: Uuid,
    path: String,
) -> AppResult<()> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("file_open_external", || {
            let target = os_target(&worktree_of(&svc, id)?, &path)?;
            crate::core::commands::open_path(app, target)
                .map_err(|e| AppError::Other(format!("open '{path}': {e}")))
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

/// Show a worktree file in the OS file manager with the item itself selected
/// (Explorer on Windows, Finder on macOS, the desktop's manager on Linux) —
/// unlike `file_open_external`, this never launches the file's own handler.
#[tauri::command]
pub async fn file_reveal(
    app: tauri::AppHandle,
    svc: State<'_, AgentService>,
    id: Uuid,
    path: String,
) -> AppResult<()> {
    let svc = svc.inner().clone();
    tauri::async_runtime::spawn_blocking(move || {
        crate::perf::timed("file_reveal", || {
            let target = os_target(&worktree_of(&svc, id)?, &path)?;
            crate::core::commands::reveal_path(app, target)
                .map_err(|e| AppError::Other(format!("reveal '{path}': {e}")))
        })
    })
    .await
    .map_err(|e| AppError::Other(format!("join: {e}")))?
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dir() -> tempfile::TempDir {
        tempfile::tempdir().expect("tempdir")
    }

    // The OS hand-off resolves under the worktree and refuses both escapes and
    // paths that are no longer there.
    #[test]
    fn os_target_resolves_existing_paths_and_refuses_the_rest() {
        let d = dir();
        create_file(d.path(), "docs/page.html").unwrap();
        let abs = os_target(d.path(), "docs/page.html").unwrap();
        assert_eq!(std::path::Path::new(&abs), d.path().join("docs/page.html"));
        // a directory is a valid target too (opens the file manager)
        assert!(os_target(d.path(), "docs").is_ok());
        assert!(os_target(d.path(), "docs/gone.html").is_err(), "missing path");
        assert!(os_target(d.path(), "../outside").is_err(), "escape");
    }

    #[test]
    fn safe_join_rejects_escapes() {
        let d = dir();
        for bad in ["", "  ", "../x", "a/../../x", "..", "/etc/passwd", "C:\\Windows\\x"] {
            assert!(safe_join(d.path(), bad).is_err(), "should reject {bad:?}");
        }
        assert!(safe_join(d.path(), "src/lib.rs").is_ok());
        assert!(safe_join(d.path(), "./src/lib.rs").is_ok());
    }

    #[test]
    fn write_creates_parents_and_overwrites() {
        let d = dir();
        write_file(d.path(), "a/b/f.txt", "one").unwrap();
        assert_eq!(std::fs::read_to_string(d.path().join("a/b/f.txt")).unwrap(), "one");
        write_file(d.path(), "a/b/f.txt", "two").unwrap();
        assert_eq!(std::fs::read_to_string(d.path().join("a/b/f.txt")).unwrap(), "two");
    }

    #[test]
    fn write_refuses_directory_target() {
        let d = dir();
        std::fs::create_dir(d.path().join("sub")).unwrap();
        assert!(write_file(d.path(), "sub", "x").is_err());
    }

    #[test]
    fn create_refuses_existing() {
        let d = dir();
        create_file(d.path(), "f.txt").unwrap();
        assert_eq!(std::fs::read_to_string(d.path().join("f.txt")).unwrap(), "");
        assert!(create_file(d.path(), "f.txt").is_err());
        create_dir(d.path(), "sub/deep").unwrap();
        assert!(create_dir(d.path(), "sub/deep").is_err());
    }

    #[test]
    fn rename_moves_and_guards_collisions() {
        let d = dir();
        write_file(d.path(), "old.txt", "x").unwrap();
        rename_path(d.path(), "old.txt", "nested/new.txt").unwrap();
        assert!(!d.path().join("old.txt").exists());
        assert_eq!(
            std::fs::read_to_string(d.path().join("nested/new.txt")).unwrap(),
            "x"
        );
        write_file(d.path(), "other.txt", "y").unwrap();
        assert!(rename_path(d.path(), "nested/new.txt", "other.txt").is_err());
        assert!(rename_path(d.path(), "missing.txt", "z.txt").is_err());
    }

    #[test]
    fn rename_allows_case_only_change() {
        let d = dir();
        write_file(d.path(), "readme.md", "x").unwrap();
        rename_path(d.path(), "readme.md", "README.md").unwrap();
        let names: Vec<_> = std::fs::read_dir(d.path())
            .unwrap()
            .map(|e| e.unwrap().file_name().to_string_lossy().into_owned())
            .collect();
        assert!(names.contains(&"README.md".to_string()), "{names:?}");
    }

    #[test]
    fn read_binary_encodes_maps_mime_and_guards() {
        let d = dir();
        std::fs::write(d.path().join("pic.png"), [0x89u8, 0x50, 0x4e, 0x47]).unwrap();
        let f = read_binary(d.path(), "pic.png").unwrap();
        assert_eq!(f.mime, "image/png");
        assert_eq!(f.len, 4);
        assert_eq!(f.base64, "iVBORw==");
        assert_eq!(mime_of("doc.PDF"), "application/pdf");
        assert_eq!(mime_of("x.unknown"), "application/octet-stream");
        assert!(read_binary(d.path(), "../pic.png").is_err());
        assert!(read_binary(d.path(), "missing.png").is_err());
        std::fs::create_dir(d.path().join("sub")).unwrap();
        assert!(read_binary(d.path(), "sub").is_err());
    }

    #[test]
    fn delete_handles_files_and_dirs() {
        let d = dir();
        write_file(d.path(), "f.txt", "x").unwrap();
        delete_path(d.path(), "f.txt").unwrap();
        assert!(!d.path().join("f.txt").exists());
        write_file(d.path(), "sub/inner.txt", "x").unwrap();
        delete_path(d.path(), "sub").unwrap();
        assert!(!d.path().join("sub").exists());
        assert!(delete_path(d.path(), "missing").is_err());
    }
}
