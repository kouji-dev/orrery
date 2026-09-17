// Builds the branded updater stub (src-tauri/updater-stub) in release mode and
// stages it where tauri.conf.json's bundle.resources expects it
// (src-tauri/binaries/orrery-updater.exe). Runs in beforeBuildCommand AND
// beforeDevCommand — tauri-build validates the resource path on EVERY cargo
// build of the app, so a fresh clone must run this once before `cargo build`
// or `cargo test` in src-tauri will fail with "resource path doesn't exist".
import { spawnSync } from "node:child_process";
import { copyFileSync, mkdirSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

// The stub is Windows-only (MSI/NSIS install machinery); on macOS the app uses
// tauri-plugin-updater's stock installer and the resource isn't bundled (it
// lives in tauri.windows.conf.json), so there is nothing to build or stage.
if (process.platform !== "win32") {
  console.log(`stage-updater-stub: skipped on ${process.platform} (Windows-only stub)`);
  process.exit(0);
}

const root = join(dirname(fileURLToPath(import.meta.url)), "..");
const srcTauri = join(root, "src-tauri");

const build = spawnSync("cargo", ["build", "--release", "-p", "orrery-updater"], {
  cwd: srcTauri,
  stdio: "inherit",
  shell: process.platform === "win32", // cargo.exe resolution through PATH
});
if (build.status !== 0) {
  console.error("stage-updater-stub: cargo build failed");
  process.exit(build.status ?? 1);
}

const out = join(srcTauri, "binaries");
mkdirSync(out, { recursive: true });
copyFileSync(
  join(srcTauri, "target", "release", "orrery-updater.exe"),
  join(out, "orrery-updater.exe"),
);
console.log("stage-updater-stub: staged src-tauri/binaries/orrery-updater.exe");
