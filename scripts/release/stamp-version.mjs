import { readFileSync, writeFileSync } from 'node:fs';
import { pathToFileURL } from 'node:url';

/** Replace the FIRST top-level "version": "x" in a JSON string (regex, so file
 *  formatting/key order is preserved). */
export function setJsonVersion(content, version) {
  return content.replace(/"version":\s*"[^"]*"/, `"version": "${version}"`);
}

/** Replace the [package] `version = "x"` line in Cargo.toml. Anchored to line
 *  start so inline dependency `version = "2"` entries are not matched. */
export function setCargoVersion(content, version) {
  return content.replace(/^version = "[^"]*"/m, `version = "${version}"`);
}

/** Update the app's OWN entry in Cargo.lock (name = "<crate>" → next version
 *  line). Without this a local bump leaves the lockfile stale and the next
 *  `cargo build` dirties the tree. */
export function setLockVersion(content, crate, version) {
  return content.replace(
    new RegExp(`(name = "${crate}"\\r?\\nversion = )"[^"]*"`),
    `$1"${version}"`,
  );
}

// CLI: node scripts/release/stamp-version.mjs <version>
// Use pathToFileURL so the main-module check matches on Windows too (a bare
// `file://${argv[1]}` yields `file://C:\…`, which never equals the canonical
// `file:///C:/…` in import.meta.url — silently skipping the stamp).
if (import.meta.url === pathToFileURL(process.argv[1]).href) {
  const version = process.argv[2];
  if (!version) {
    console.error('usage: stamp-version.mjs <version>');
    process.exit(1);
  }
  for (const f of ['package.json', 'src-tauri/tauri.conf.json']) {
    writeFileSync(f, setJsonVersion(readFileSync(f, 'utf8'), version));
  }
  writeFileSync('src-tauri/Cargo.toml', setCargoVersion(readFileSync('src-tauri/Cargo.toml', 'utf8'), version));
  writeFileSync('src-tauri/Cargo.lock', setLockVersion(readFileSync('src-tauri/Cargo.lock', 'utf8'), 'orrery', version));
  console.log(`stamped version ${version}`);
}
