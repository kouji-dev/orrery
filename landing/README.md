# Orrery landing site

Zero-dependency static site implementing the **Orrery Home** + **Changelog**
designs from the Claude Design handoff. No build step — what's in this folder
is what ships.

| File | Purpose |
|---|---|
| `index.html` | Home / landing page. Self-contains the design CSS; loads the two scripts below. |
| `home.js` | Interactions — nav, mobile menu, scroll reveals, the epicycle rosette logo, and mounting/scaling the console mock. |
| `console-mock.js` | Data-driven, faithful static render of the app's "Orchestrator" view (the product showcase). |
| `agents.js` | `<orrery-agent>` — native custom element rendering an AI-tool brand mark + label (registry-driven; used in the hero orbit and tool strip). New agents = one sprite `<symbol>` in `index.html` + one registry entry. |
| `dl-button.js` | `<orrery-download>` — native custom element (light DOM) for the split download button + installer dropdown, reused in the nav (`size="sm"`), hero and bottom CTA band. |
| `version.js` | Resolves the latest release via one GitHub API call, updates `.js-ver` labels and feeds every `<orrery-download>` its direct installer URLs for the visitor's OS (mac → `.dmg`, otherwise `-setup.exe`), falling back to the releases page when offline/rate-limited. |
| `changelog.html` | Self-contained changelog page, populated from real Orrery releases (`git log` on `origin/main`). |

Brand: Space Grotesk + JetBrains Mono, on the app's own purple `#a855f7` /
cyan `#22d3ee` tokens. The mark is the epicycle (rosette) shared with the app splash.

## Local preview

```bash
# from the repo root
npx serve landing      # or: python -m http.server -d landing 8080
```

## Updating the changelog

`changelog.html` holds a `RELEASES` array baked from `git log`. Regenerate it at
release time from the real tags/commits (newest first), grouped by version and
conventional-commit type (`feat` / `fix` / `perf` / `refactor` / `chore` / `ci`).

## Deploying to Render

Unchanged from the original setup — see `render.yaml` (repo root) and
`.github/workflows/deploy-landing.yml`:

- `render.yaml` defines the `orrery-landing` **static site** serving this
  folder. `autoDeploy` is off; `buildFilter` is scoped to `landing/**`. It's a
  multi-page site (no SPA rewrite), so `/` serves `index.html` and
  `/changelog.html` serves the changelog directly.
- The GitHub workflow fires Render API deploys via the `RENDER_API_KEY` secret,
  **only** when files under `landing/` change.
