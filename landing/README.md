# Orrery landing page

A zero-dependency static site (`index.html` + `styles.css` + `app.js` + `favicon.svg`).
No build step — what's in this folder is what ships.

## Local preview

```bash
# from the repo root
npx serve landing      # or: python -m http.server -d landing 8080
```

Open the printed URL. The hero runs a live `<canvas>` orrery; everything is
plain HTML/CSS/JS so opening `index.html` directly mostly works too (a local
server is recommended so Google Fonts and module loading behave).

## Deploying to Render

Two pieces work together:

1. **`render.yaml`** (repo root) — the Blueprint. It defines the
   `orrery-landing` **static site** that publishes this folder. `autoDeploy`
   is **off** and `buildFilter` is scoped to `landing/**`, so Render never
   rebuilds for unrelated commits.

2. **`.github/workflows/deploy-landing.yml`** — fires the actual redeploys via
   the Render API using the `RENDER_API_KEY` secret. It runs **only** when
   files under `landing/` (or the deploy infra) change. On the first run it
   creates the service if it doesn't exist yet, then triggers and waits for
   the deploy.

### One-time setup

- Add a repository secret **`RENDER_API_KEY`** (Settings → Secrets and
  variables → Actions). Generate the key in the Render dashboard
  (Account Settings → API Keys).
- Optional: connect `render.yaml` as a Blueprint in the Render dashboard if
  you prefer Render to own service creation. If you don't, the workflow
  bootstraps the service via the API on its first run — pick one path so you
  don't end up with two services of the same name.

Pushing any change under `landing/` to `main` then redeploys automatically.
