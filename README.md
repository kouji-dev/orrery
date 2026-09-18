# Orrery

Two roots, one cargo workspace and one pnpm workspace.

| Root | What it is |
|---|---|
| [`ade/`](ade) | The Orrery ADE — the Tauri 2 + Angular desktop app. Crate `orrery-ade`, npm package `orrery-ade`. Ships as `Orrery.exe`, bundle identifier `com.kouji.orrery`. |
| `harness/` | The Orrery Harness runtime — the `orrery` CLI and its crates. Plans live in [`harness/docs/plans`](harness/docs/plans). |
| `landing/` | The static marketing site, deployed on its own from the repo root. |

## Cargo

The repo root is a virtual cargo workspace (`resolver = "2"`). Run cargo **from
the root**; `target/` is shared by both trees.

```bash
cargo check                       # default-members = ade/src-tauri only
cargo test -p orrery-ade          # the ADE crate
cargo clippy --workspace -- -D warnings
```

Shared version pins live in the root `[workspace.dependencies]`; members take
them with `{ workspace = true }` so the ADE and the harness link one copy of
each. `clippy.toml` sits at the root so its `disallowed-methods` apply to both.

## Node / pnpm

```bash
pnpm install                      # from the root — workspace packages: ade
pnpm -C ade build                 # → ade/dist/orrery-ade/browser
pnpm -C ade dev                   # tauri dev
pnpm -C ade e2e
```

The root `package.json` is private and has no dependencies; its scripts
(`test`, `dev`, `build`, `e2e`, `release`, `ext:*`, …) are passthroughs to
`pnpm -C ade <script>`, so CI and muscle memory keep working from the root.

## What did not change

The bundle identifier (`com.kouji.orrery`), the product name (`Orrery`), the
window title, the app data directory and the WiX upgrade code are all
untouched by the restructure. Installed releases keep updating in place.
