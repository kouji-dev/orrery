import { readFileSync, writeFileSync, readdirSync } from 'node:fs';
import { join } from 'node:path';
import { pathToFileURL } from 'node:url';

export function assetUrl(repo, tag, filename) {
  return `https://github.com/${repo}/releases/download/${tag}/${filename}`;
}

/** Build the updater `platforms` map. Tauri picks the entry matching how the app
 *  was installed: an NSIS install looks up `windows-x86_64`, an MSI install looks
 *  up `windows-x86_64-msi` (falling back to `windows-x86_64`). Publishing both so
 *  per-user (NSIS) and per-machine (MSI) installs each get an installer they can
 *  actually apply — otherwise an MSI machine is handed the NSIS setup it can't
 *  update in place and reinstalls forever.
 *  `installers`: `[{ key, file, signature }]`. */
export function buildPlatforms(repo, tag, installers) {
  const platforms = {};
  for (const { key, file, signature } of installers) {
    platforms[key] = { signature, url: assetUrl(repo, tag, file) };
  }
  return platforms;
}

export function buildLatestJson({ version, notes, pubDate, platforms }) {
  return {
    version,
    notes,
    pub_date: pubDate,
    platforms,
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
  const files = readdirSync(a.dir);

  // Each installer needs its sibling `.sig`; skip (don't fail) if one is missing
  // so a build that didn't sign the MSI still ships a working NSIS-only manifest.
  const installers = [];
  const stage = (key, file) => {
    if (!file) return;
    const sig = `${file}.sig`;
    if (!files.includes(sig)) {
      console.warn(`no signature ${sig} — omitting ${key}`);
      return;
    }
    installers.push({ key, file, signature: readFileSync(join(a.dir, sig), 'utf8').trim() });
  };
  stage('windows-x86_64', files.find((f) => f.endsWith('-setup.exe')));
  stage('windows-x86_64-msi', files.find((f) => f.endsWith('.msi')));
  // macOS updater payload (Apple Silicon builds; the .dmg is the human download,
  // the updater consumes the .app.tar.gz). Optional: a windows-only run still ships.
  stage('darwin-aarch64', files.find((f) => f.endsWith('.app.tar.gz')));

  if (!installers.some((i) => i.key.startsWith('windows-'))) {
    console.error(`no signed Windows installer (*-setup.exe / *.msi) in ${a.dir}`);
    process.exit(1);
  }

  const manifest = buildLatestJson({
    version: a.version,
    notes: a.notes || `Orrery v${a.version}`,
    pubDate: new Date().toISOString(),
    platforms: buildPlatforms(a.repo, tag, installers),
  });
  writeFileSync(a.out, JSON.stringify(manifest, null, 2));
  console.log(`wrote ${a.out} for ${installers.map((i) => i.key).join(', ')}`);
}
