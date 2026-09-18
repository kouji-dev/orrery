/**
 * Task 7: the same fixtures the Rust SDK runs, and the third-party proof.
 *
 * The files are `harness/clients/conformance/*.jsonl` — literally the ones
 * `clients/sdk-rs/tests/conformance.rs` replays. A scenario only one language
 * checks has stopped being a contract.
 *
 * The second half is mechanical rather than rhetorical: this package's
 * manifest may name nothing from the harness but the two published packages,
 * and no file here may reach outside the package with a relative import.
 */

import { readFileSync, readdirSync, statSync } from "node:fs";
import { dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import { render } from "ink-testing-library";
import { describe, expect, it } from "vitest";

import { conformance } from "@orrery/client";

import { Turn } from "../src/turn.js";

const here = dirname(fileURLToPath(import.meta.url));
const root = join(here, "..");
const scenarios = conformance.loadAll();

describe("the conformance fixtures", () => {
  it("are the same files the Rust SDK runs", () => {
    expect(conformance.fixturesDir().replaceAll("\\", "/")).toContain("clients/conformance");
    expect(scenarios.length).toBeGreaterThanOrEqual(10);
  });

  for (const scenario of scenarios) {
    it(`${scenario.name}: the store agrees at every checkpoint, then draws`, () => {
      // Data first.
      for (const { index, actual, expected } of conformance.checkpoints(scenario)) {
        expect(actual, `${scenario.name} checkpoint ${index}`).toEqual(expected);
      }

      // Pixels second: the whole transcript, drawn, snapshotted.
      const store = conformance.replay(scenario);
      const ui = render(
        <>
          {store.state().turns.map((turn) => (
            <Turn key={turn.id} turn={turn} width={72} />
          ))}
        </>,
      );
      const frame = ui.lastFrame() ?? "";
      expect(frame).not.toContain("[no renderer for");
      expect(frame).toMatchSnapshot();
      ui.unmount();
    });
  }
});

/** Every `.ts`/`.tsx` file under `src`. */
function sources(dir: string): string[] {
  return readdirSync(dir).flatMap((entry) => {
    const path = join(dir, entry);
    if (statSync(path).isDirectory()) return sources(path);
    return /\.tsx?$/.test(entry) ? [path] : [];
  });
}

describe("the third-party proof", () => {
  const manifest = JSON.parse(readFileSync(join(root, "package.json"), "utf8")) as {
    dependencies?: Record<string, string>;
    devDependencies?: Record<string, string>;
  };

  it("depends on nothing from the harness but the two published packages", () => {
    const all = { ...manifest.dependencies, ...manifest.devDependencies };
    const ours = Object.keys(all).filter((name) => name.startsWith("@orrery/"));
    expect(ours.sort()).toEqual(["@orrery/client", "@orrery/protocol"]);
    // And nothing else is wired in by path: a `file:` or `link:` range would
    // be a private path with a public name.
    for (const [name, range] of Object.entries(all)) {
      if (name.startsWith("@orrery/")) continue;
      expect(range, `${name} is not a registry dependency`).not.toMatch(/^(file:|link:|workspace:)/);
    }
  });

  it("imports nothing from outside the package", () => {
    for (const file of sources(join(root, "src"))) {
      const text = readFileSync(file, "utf8");
      for (const match of text.matchAll(/from\s+"([^"]+)"/g)) {
        const specifier = match[1]!;
        expect(specifier, `${file} reaches outside the package`).not.toMatch(/^\.\.\/\.\./);
        if (specifier.startsWith("@orrery/")) {
          expect(
            specifier === "@orrery/client" ||
              specifier === "@orrery/protocol" ||
              specifier.startsWith("@orrery/protocol/"),
            `${file} imports ${specifier}`,
          ).toBe(true);
        }
      }
    }
  });
});
