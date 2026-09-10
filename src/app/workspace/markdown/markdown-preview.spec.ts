import { describe, expect, it } from "vitest";

import { resolveRelative, slug } from "./markdown-preview.component";

// Pure helpers behind link following in the preview. The component itself is
// exercised end-to-end (e2e/md-preview-links.spec.ts) — signal inputs do not
// render under vitest's JIT (NG0950), so only the path/slug math lives here.
describe("resolveRelative", () => {
  it("joins onto the current file's folder and collapses . and ..", () => {
    expect(resolveRelative("docs/guide", "./notes/design.md")).toBe("docs/guide/notes/design.md");
    expect(resolveRelative("docs/guide", "../README.md")).toBe("docs/README.md");
    expect(resolveRelative("docs/guide", "../../CHANGELOG.md")).toBe("CHANGELOG.md");
    expect(resolveRelative("", "README.md")).toBe("README.md");
  });

  it("treats a leading slash as the worktree root, and never escapes above it", () => {
    expect(resolveRelative("docs/guide", "/src/main.ts")).toBe("src/main.ts");
    expect(resolveRelative("docs", "../../../etc")).toBe("etc");
  });
});

describe("slug", () => {
  it("matches GitHub-style heading anchors", () => {
    expect(slug("Second part")).toBe("second-part");
    expect(slug("  Retry: design & limits!  ")).toBe("retry-design--limits");
    expect(slug("API v2.0")).toBe("api-v20");
  });
});
