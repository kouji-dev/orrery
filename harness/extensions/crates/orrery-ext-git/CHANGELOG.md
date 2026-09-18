# Changelog

## Unreleased

- Implemented, read-only: `status`, `log`, `show`, `diff`, `blame`, over
  gitoxide, reusing `ade/src-tauri/src/git/gix_backend.rs`'s knowledge.
- The manifest no longer declares `branch` and `commit`, and no longer asks for
  `write` or `spawn`: none of the three existed, and a manifest that lists a
  tool the extension does not have makes the ledger a lie.
- A broker `read` probe against the repository path runs before gitoxide opens
  anything, so a denial is a denial in the ledger rather than a capability
  gitoxide quietly went around.
