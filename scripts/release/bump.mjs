import { execFileSync } from 'node:child_process';
import { readFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

// One-word release flow: `pnpm release` / `pnpm release:patch`.
//   release → next minor (0.1.17 → 0.2.0)  — a feature release
//   patch   → next patch (0.1.17 → 0.1.18) — a bugfix release
// Stamps package.json / tauri.conf.json / Cargo.toml(+lock), commits
// `release: vX.Y.Z`, tags `vX.Y.Z`, and pushes branch + tag — the tag push
// triggers .github/workflows/release.yml. `--no-push` stops after the tag.

/** Next version for a bump kind. Throws on a non-semver current version. */
export function nextVersion(current, kind) {
  const m = /^(\d+)\.(\d+)\.(\d+)$/.exec(current);
  if (!m) throw new Error(`current version "${current}" is not plain semver`);
  const [major, minor, patch] = [Number(m[1]), Number(m[2]), Number(m[3])];
  if (kind === 'release') return `${major}.${minor + 1}.0`;
  if (kind === 'patch') return `${major}.${minor}.${patch + 1}`;
  throw new Error(`unknown bump kind "${kind}" (use release | patch)`);
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const kind = process.argv[2];
  const push = !process.argv.includes('--no-push');
  if (kind !== 'release' && kind !== 'patch') {
    console.error('usage: bump.mjs <release|patch> [--no-push]');
    process.exit(1);
  }
  const git = (...args) => execFileSync('git', args, { stdio: 'inherit' });
  const out = (...args) => execFileSync('git', args).toString().trim();

  if (out('status', '--porcelain') !== '') {
    console.error('working tree not clean — commit or stash first');
    process.exit(1);
  }
  const current = JSON.parse(readFileSync('package.json', 'utf8')).version;
  const next = nextVersion(current, kind);
  execFileSync('node', ['scripts/release/stamp-version.mjs', next], { stdio: 'inherit' });
  git('add', 'package.json', 'src-tauri/tauri.conf.json', 'src-tauri/Cargo.toml', 'src-tauri/Cargo.lock');
  git('commit', '-m', `release: v${next}`);
  git('tag', `v${next}`);
  if (push) {
    git('push', 'origin', 'HEAD', `refs/tags/v${next}`);
    console.log(`v${next} pushed — the Release workflow is building it`);
  } else {
    console.log(`v${next} committed + tagged (not pushed)`);
  }
}
