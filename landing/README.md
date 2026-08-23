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
| `404.html` | Branded not-found page listing every URL the site publishes, plus the same links as a markdown block for agents. Render static sites have no custom-404 hook, so this only takes effect if the host ever honours a root `404.html` — see "Agent readiness" below. |
| `llms.txt` | [llmstxt.org](https://llmstxt.org/) file: what Orrery is, **when an agent should reach for it**, and links to the other machine-readable files. |
| `robots.txt` | Crawl permissions (wildcard + explicit AI-crawler groups) and the `Sitemap:` pointer. |
| `sitemap.xml` | Generated — `node scripts/landing/gen-sitemap.mjs`. Lists every page except `404.html`, with `lastmod` from git. |
| `og.png` | Generated — `node scripts/landing/gen-og.mjs`. 1200x630 Open Graph card, same tokens and epicycle mark as the site. |

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

## Agent readiness

The homepage carries `<link rel="canonical">`, the four Open Graph signals and a
`SoftwareApplication` JSON-LD block (identity, `offers`, `featureList`, publisher)
so agents can resolve what Orrery is without executing JavaScript. Its heading
tree is `h1 → h2 → h3`; `.kicker` pins `font-weight` precisely because
"How it works" is an `<h2>` and the UA bold would otherwise show.

Two audit findings cannot be fixed on a Render **static** site, and neither is
attempted here:

- **Custom 404 body.** Static rules are `redirect` (301) or `rewrite` (always
  200) with no status field — a `*` → `404.html` rewrite would serve the error
  page with **200** and break the genuine 404 the site returns today. `404.html`
  ships purely as a hedge in case the host honours it; no rewrite rule exists.
- **`Accept: text/markdown` negotiation** ([acceptmarkdown.com](https://acceptmarkdown.com))
  needs request-time logic — markdown variant, `Vary: Accept`, `406` — which a
  file host does not have.

Both would require moving the landing to a small server runtime.

When adding a page: run `gen-sitemap.mjs` and give it a canonical + OG tags.
`e2e/landing-agent-readiness.spec.ts` fails if a new page is missing from the
sitemap.
