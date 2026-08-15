import { pathToFileURL } from 'node:url';

// Install tracking via GitHub release asset download counts — the ground truth
// for completed downloads (the landing CTAs link the assets directly, so every
// download lands here). Buckets per release:
//   setup.exe — NSIS installer downloads (the landing's primary Windows CTA)
//   msi       — MSI installer downloads (secondary link)
//   dmg       — macOS downloads
//   updates   — latest.json fetches: every in-app update CHECK downloads it,
//               so it's a cumulative activity proxy, not an install count.
// `.sig` and `.app.tar.gz` are updater plumbing and are ignored.
//
// Counts are CUMULATIVE — GitHub keeps no history. If trends over time are
// wanted later, snapshot `--json` output periodically (e.g. a scheduled Action
// appending NDJSON) — totals lost before the first snapshot are unrecoverable.
//
// Usage: node scripts/download-stats.mjs [--json]
//        GITHUB_TOKEN env optional (only for API rate limits).

const REPO = 'kouji-dev/orrery-releases';

/** Bucket one release's assets into download-count sums. */
export function bucketAssets(assets) {
  const b = { setupExe: 0, msi: 0, dmg: 0, updates: 0 };
  for (const a of assets || []) {
    if (/-setup\.exe$/i.test(a.name)) b.setupExe += a.download_count;
    else if (/\.msi$/i.test(a.name)) b.msi += a.download_count;
    else if (/\.dmg$/i.test(a.name)) b.dmg += a.download_count;
    else if (a.name === 'latest.json') b.updates += a.download_count;
  }
  b.installers = b.setupExe + b.msi + b.dmg;
  return b;
}

async function fetchAllReleases(repo, token) {
  const headers = { Accept: 'application/vnd.github+json' };
  if (token) headers.Authorization = `Bearer ${token}`;
  const releases = [];
  for (let page = 1; ; page++) {
    const r = await fetch(`https://api.github.com/repos/${repo}/releases?per_page=100&page=${page}`, { headers });
    if (!r.ok) throw new Error(`GitHub API ${r.status} on page ${page}`);
    const batch = await r.json();
    releases.push(...batch);
    if (batch.length < 100) return releases;
  }
}

if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const json = process.argv.includes('--json');
  const releases = await fetchAllReleases(REPO, process.env.GITHUB_TOKEN);

  const rows = releases.map((r) => ({ tag: r.tag_name, published: r.published_at, ...bucketAssets(r.assets) }));
  const total = { setupExe: 0, msi: 0, dmg: 0, updates: 0, installers: 0 };
  for (const row of rows) for (const k of Object.keys(total)) total[k] += row[k];

  if (json) {
    console.log(JSON.stringify({ repo: REPO, fetched_at: new Date().toISOString(), total, releases: rows }, null, 2));
  } else {
    const pad = (v, w) => String(v).padStart(w);
    console.log('tag           setup.exe    msi    dmg  installers  update-checks');
    for (const r of rows)
      console.log(`${r.tag.padEnd(13)} ${pad(r.setupExe, 9)} ${pad(r.msi, 6)} ${pad(r.dmg, 6)} ${pad(r.installers, 11)} ${pad(r.updates, 14)}`);
    console.log('—'.repeat(64));
    console.log(`${'TOTAL'.padEnd(13)} ${pad(total.setupExe, 9)} ${pad(total.msi, 6)} ${pad(total.dmg, 6)} ${pad(total.installers, 11)} ${pad(total.updates, 14)}`);
    console.log(`\n${rows.length} releases · installers = completed downloads · update-checks = cumulative latest.json fetches (activity proxy)`);
  }
}
