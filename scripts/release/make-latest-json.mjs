import { readFileSync, writeFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

export function assetUrl(repo, tag, filename) {
  return `https://github.com/${repo}/releases/download/${tag}/${filename}`;
}

export function buildLatestJson({ version, notes, pubDate, signature, url }) {
  return {
    version,
    notes,
    pub_date: pubDate,
    platforms: {
      'windows-x86_64': { signature, url },
    },
  };
}

function parseArgs(argv) {
  const out = {};
  for (let i = 0; i < argv.length; i += 2) out[argv[i].replace(/^--/, '')] = argv[i + 1];
  return out;
}

// CLI: node make-latest-json.mjs --version X --dir DIR --out FILE --repo O/R [--notes "..."]
// pathToFileURL: see stamp-version.mjs — the bare-string check fails to detect
// the main module on Windows.
if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const a = parseArgs(process.argv.slice(2));
  const tag = `v${a.version}`;
  const setup = readdirSync(a.dir).find((f) => f.endsWith('-setup.exe'));
  if (!setup) {
    console.error(`no *-setup.exe in ${a.dir}`);
    process.exit(1);
  }
  const signature = readFileSync(join(a.dir, `${setup}.sig`), 'utf8').trim();
  const manifest = buildLatestJson({
    version: a.version,
    notes: a.notes || `Orrery v${a.version}`,
    pubDate: new Date().toISOString(),
    signature,
    url: assetUrl(a.repo, tag, setup),
  });
  writeFileSync(a.out, JSON.stringify(manifest, null, 2));
  console.log(`wrote ${a.out} for ${setup}`);
}
