//! A path is normalised before it is asked about, never after.
//!
//! # Why the tool does this and not only the broker
//!
//! A grant is a glob, and `**` spans separators, so `read = ["$WORKSPACE/**"]`
//! matches the *string* `$WORKSPACE/../../etc/passwd` on its face. The broker
//! the harness wires normalises first and refuses it (`orrery-policy`'s
//! `normalise_path`), but a tool that hands out an unnormalised path has made
//! its own safety somebody else's problem: the mock broker a community author
//! tests against does not normalise, and neither need any other facade an
//! embedder supplies.
//!
//! So the ask itself is normalised. `/ws/../outside` is asked about as
//! `/outside` — which no `/ws/**` grant covers, whoever is answering — and
//! `/ws/sub/../a.txt` is asked about as `/ws/a.txt`, which is the file it
//! always was. This is lexical only: nothing here touches the filesystem, so
//! it works for a file that does not exist yet, which is exactly the case
//! `write` is.

use std::path::{Component, Path, PathBuf};

/// The same path with `.` removed and `..` resolved against what precedes it.
///
/// A leading `..` that nothing precedes stays: `../outside` names something
/// above wherever the call is rooted, and pretending otherwise would turn an
/// escape into an innocent-looking relative name.
pub(crate) fn normalise(path: impl AsRef<Path>) -> PathBuf {
    let mut out: Vec<Component<'_>> = Vec::new();
    for component in path.as_ref().components() {
        match component {
            Component::CurDir => {}
            Component::ParentDir => match out.last() {
                // Above the root is still the root, on every platform.
                Some(Component::RootDir | Component::Prefix(_)) => {}
                Some(Component::Normal(_)) => {
                    out.pop();
                }
                // Nothing to climb out of yet, so the climb is part of the name.
                _ => out.push(component),
            },
            other => out.push(other),
        }
    }
    if out.is_empty() {
        return PathBuf::from(".");
    }
    out.iter().collect()
}

#[cfg(test)]
mod tests {
    use super::normalise;

    #[test]
    fn dot_dot_is_resolved_against_what_precedes_it() {
        assert_eq!(normalise("/ws/../outside"), std::path::Path::new("/outside"));
        assert_eq!(normalise("/ws/sub/../a.txt"), std::path::Path::new("/ws/a.txt"));
        assert_eq!(normalise("/ws/./a.txt"), std::path::Path::new("/ws/a.txt"));
    }

    #[test]
    fn a_climb_nothing_precedes_survives() {
        assert_eq!(normalise("../outside"), std::path::Path::new("../outside"));
    }

    #[test]
    fn the_root_has_no_parent() {
        assert_eq!(normalise("/../../etc"), std::path::Path::new("/etc"));
    }

    #[test]
    fn nothing_at_all_is_here() {
        assert_eq!(normalise("a/.."), std::path::Path::new("."));
    }
}
