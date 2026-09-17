# Add Project Dialog

**Context.** The add‑project dialog registers a new git repository. It captures a working
directory, detects whether a `.git` already exists (offering `git init` otherwise), and lets the
user pick an icon and accent color. A second source — "From Git URL" — clones a remote
(e.g. a GitHub link) instead of registering a local folder.

Source: `modals/add-project-modal.component.ts`, `orchestra.store.ts` (`addProject`).

## Layout

- [x] Modal overlay with blurred backdrop; click‑outside / Cancel closes
- [x] Header preview tile reflects the chosen icon + color live
- [x] Working‑directory field auto‑focused on open

## Working directory

- [x] Free‑text path input (`~/code/my-repo`)
- [x] Browse… folder picker (`webkitdirectory`) that fills the path from the chosen folder
- [x] Project name derived live from the directory’s last path segment

## Git detection

- [x] Heuristic detects an existing repository vs. a fresh directory
- [x] "Existing git repository detected" state (success styling, git icon)
- [x] "Run git init (no .git found)" state with an explanatory subline
- [x] Checkbox toggles git‑init; default follows detection and updates as the path changes

## Icon & color

- [x] Icon picker (8 presets) with selected highlight
- [x] Color picker (7 presets) as swatches with selected ring/glow
- [x] Selected icon tinted with the selected color

## From Git URL (clone source)

- [x] Source toggle at the top of the dialog: "Local folder" / "From Git URL"
- [x] Repository URL field (https or ssh); project name derived from the repo name
- [x] "Clone into" destination folder with Browse…
- [x] Path mode — "Use path as root": clone lands in `<path>/<repo-name>`
- [x] Path mode — "Use path as the project": repo content lands directly in `<path>` (the `git clone <url> .` shape); destination must be absent or empty
- [x] Shallow clone toggle (default on): `--depth 1` fetches only the default branch at its tip
- [x] Live "clones to →" destination preview
- [x] Same `project_create` command carries optional `sourceUrl` / `sourceMode` / `depth` — the frontend never knows whether the backend cloned or registered locally
- [x] Clone shells out to the git CLI so the OS credential helper can auth private remotes

## Submit

- [x] Add button disabled until a directory is entered
- [x] New project appended to the sidebar
- [x] Toast differentiates "initialized git + added" vs. "added project"
