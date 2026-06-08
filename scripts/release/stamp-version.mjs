import { readFileSync, writeFileSync } from 'node:fs';

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

// CLI: node scripts/release/stamp-version.mjs <version>
if (import.meta.url === `file://${process.argv[1]}`) {
  const version = process.argv[2];
  if (!version) {
    console.error('usage: stamp-version.mjs <version>');
    process.exit(1);
  }
  for (const f of ['package.json', 'src-tauri/tauri.conf.json']) {
    writeFileSync(f, setJsonVersion(readFileSync(f, 'utf8'), version));
  }
  writeFileSync('src-tauri/Cargo.toml', setCargoVersion(readFileSync('src-tauri/Cargo.toml', 'utf8'), version));
  console.log(`stamped version ${version}`);
}
