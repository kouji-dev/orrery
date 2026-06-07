//! Predefined natural-language prompts for AI-driven completion actions. katrix
//! types these into the agent's PTY; the agent runs the actual `git` with its own
//! tools (so its permission flow / hooks apply). Tool-agnostic.

/// The prompt for a completion `kind` (commit/push/rebase/merge), with the agent's
/// `branch` / `base` interpolated. `None` for an unknown kind.
pub fn action_prompt(kind: &str, branch: &str, base: &str) -> Option<String> {
    let p = match kind {
        "commit" => "Commit all current changes in this worktree with a clear, concise \
                     message describing what changed: run `git add -A` then `git commit`. \
                     Do not push."
            .to_string(),
        "push" => format!(
            "Push the current branch to origin: run `git push -u origin {branch}`. Do nothing else."
        ),
        "rebase" => format!(
            "Rebase this worktree onto `{base}`: run `git rebase {base}`, resolve any \
             conflicts, and complete the rebase. Do not push and do not merge."
        ),
        "merge" => format!(
            "Merge `{base}` INTO the current branch `{branch}` — bring `{base}`'s changes \
             into this branch. From within this worktree run `git merge {base}`, resolve any \
             conflicts, and complete the merge. Do NOT merge the other direction and do NOT \
             modify `{base}`. Do not push."
        ),
        _ => return None,
    };
    Some(p)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_interpolates_branch() {
        let p = action_prompt("push", "agent/fix_login", "main").unwrap();
        assert!(p.contains("git push -u origin agent/fix_login"), "got: {p}");
    }

    #[test]
    fn rebase_interpolates_base() {
        let p = action_prompt("rebase", "agent/x", "develop").unwrap();
        assert!(p.contains("git rebase develop"), "got: {p}");
    }

    #[test]
    fn merge_locks_direction_base_into_branch() {
        let p = action_prompt("merge", "agent/x", "main").unwrap();
        assert!(p.contains("Merge `main` INTO the current branch `agent/x`"), "got: {p}");
        assert!(p.contains("git merge main"), "got: {p}");
        assert!(p.contains("Do NOT merge the other direction"), "got: {p}");
    }

    #[test]
    fn unknown_kind_is_none() {
        assert_eq!(action_prompt("frobnicate", "a", "b"), None);
    }
}
