use std::path::Path;

use git2::Repository;

/// One node in the worktree file tree.
/// `children: None` = a directory whose contents haven't been loaded yet (lazy stub).
#[derive(serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FileNode {
    pub name: String,
    pub path: String, // relative to the worktree, forward-slash separated
    pub is_dir: bool,
    pub ignored: bool,
    pub children: Option<Vec<FileNode>>,
}

const NODE_LIMIT: usize = 10_000;

/// Full tree of a worktree: tracked/source dirs are recursed, gitignored dirs are
/// listed but left as lazy stubs (`children: None`) so we never walk node_modules.
pub fn tree(worktree: &Path) -> Vec<FileNode> {
    let Ok(repo) = Repository::open(worktree) else {
        return Vec::new();
    };
    let workdir = repo.workdir().unwrap_or(worktree).to_path_buf();
    let mut count = 0;
    scan(&repo, &workdir, "", true, &mut count)
}

/// One level of a directory (lazy expand) — every subdir comes back as a stub.
pub fn list_dir(worktree: &Path, rel: &str) -> Vec<FileNode> {
    let Ok(repo) = Repository::open(worktree) else {
        return Vec::new();
    };
    let workdir = repo.workdir().unwrap_or(worktree).to_path_buf();
    let mut count = 0;
    scan(&repo, &workdir, rel, false, &mut count)
}

fn scan(
    repo: &Repository,
    workdir: &Path,
    rel: &str,
    recurse: bool,
    count: &mut usize,
) -> Vec<FileNode> {
    let abs = if rel.is_empty() {
        workdir.to_path_buf()
    } else {
        workdir.join(rel)
    };
    let Ok(entries) = std::fs::read_dir(&abs) else {
        return Vec::new();
    };

    let mut nodes = Vec::new();
    for entry in entries.flatten() {
        if *count >= NODE_LIMIT {
            break;
        }
        let name = entry.file_name().to_string_lossy().to_string();
        if name == ".git" {
            continue;
        }
        let child_rel = if rel.is_empty() {
            name.clone()
        } else {
            format!("{rel}/{name}")
        };
        let is_dir = entry.file_type().map(|t| t.is_dir()).unwrap_or(false);
        // git's own ignore logic (nested .gitignore, .git/info/exclude, global)
        let ignored = repo.is_path_ignored(Path::new(&child_rel)).unwrap_or(false);
        *count += 1;

        // recurse only into tracked dirs; ignored dirs stay lazy stubs
        let children = if is_dir && recurse && !ignored {
            Some(scan(repo, workdir, &child_rel, true, count))
        } else {
            None
        };
        nodes.push(FileNode {
            name,
            path: child_rel,
            is_dir,
            ignored,
            children,
        });
    }

    nodes.sort_by(|a, b| match (a.is_dir, b.is_dir) {
        (true, false) => std::cmp::Ordering::Less,
        (false, true) => std::cmp::Ordering::Greater,
        _ => a.name.to_lowercase().cmp(&b.name.to_lowercase()),
    });
    nodes
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn tree_recurses_source_but_stubs_ignored_dirs() {
        let dir = tempfile::tempdir().unwrap();
        Repository::init(dir.path()).unwrap();
        std::fs::create_dir_all(dir.path().join("src")).unwrap();
        std::fs::write(dir.path().join("src/main.rs"), "x").unwrap();
        std::fs::create_dir_all(dir.path().join("node_modules/foo")).unwrap();
        std::fs::write(dir.path().join("node_modules/foo/index.js"), "x").unwrap();
        std::fs::write(dir.path().join(".gitignore"), "node_modules/\n").unwrap();

        let t = tree(dir.path());

        let nm = t
            .iter()
            .find(|n| n.name == "node_modules")
            .expect("ignored dir is listed");
        assert!(nm.is_dir && nm.ignored, "node_modules marked ignored");
        assert!(
            nm.children.is_none(),
            "ignored dir not auto-scanned (lazy stub)"
        );

        let src = t.iter().find(|n| n.name == "src").expect("src listed");
        assert!(!src.ignored);
        assert!(
            src.children
                .as_ref()
                .map(|c| c.iter().any(|x| x.name == "main.rs"))
                .unwrap_or(false),
            "src is recursed"
        );

        // lazy-expand the ignored dir on demand
        let kids = list_dir(dir.path(), "node_modules");
        assert!(
            kids.iter().any(|n| n.name == "foo" && n.is_dir),
            "lazy listing works"
        );
    }

    #[test]
    fn tree_excludes_dot_git() {
        let dir = tempfile::tempdir().unwrap();
        Repository::init(dir.path()).unwrap();
        std::fs::write(dir.path().join("a.txt"), "x").unwrap();
        let t = tree(dir.path());
        assert!(t.iter().all(|n| n.name != ".git"));
        assert!(t.iter().any(|n| n.name == "a.txt"));
    }
}
