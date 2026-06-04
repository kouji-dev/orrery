# Add Project Dialog

**Context.** The add‑project dialog registers a new git repository. It captures a working
directory, detects whether a `.git` already exists (offering `git init` otherwise), and lets the
user pick an icon and accent color.

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

## Submit

- [x] Add button disabled until a directory is entered
- [x] New project appended to the sidebar
- [x] Toast differentiates "initialized git + added" vs. "added project"
