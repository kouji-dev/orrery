import { readFileSync } from "node:fs";
import { resolve } from "node:path";
import { describe, expect, it } from "vitest";

/**
 * A0.6: no source maps may ship in prod — they multiply the webview heap (the
 * map for a bundled app easily exceeds the bundle) and leak source. Angular's
 * production default is `sourceMap: false`; these assertions lock that nobody
 * flips it on in the prod configuration or the base options unnoticed.
 */
describe("angular.json production build", () => {
  const raw = readFileSync(resolve(__dirname, "../../angular.json"), "utf8");
  const cfg = JSON.parse(raw) as {
    projects: Record<
      string,
      {
        architect: {
          build: {
            options?: { sourceMap?: unknown };
            configurations: { production?: { sourceMap?: unknown } };
            defaultConfiguration?: string;
          };
        };
      }
    >;
  };
  const build = cfg.projects["orrery"].architect.build;

  it("does not enable source maps in the production configuration", () => {
    expect(build.configurations.production?.sourceMap ?? false).toBe(false);
  });

  it("does not enable source maps in the base build options", () => {
    expect(build.options?.sourceMap ?? false).toBe(false);
  });

  it("defaults to the production configuration", () => {
    expect(build.defaultConfiguration).toBe("production");
  });
});
