import { describe, it, expect } from "vitest";
import { entryFromCommits, upsertRelease } from "./changelog-json.mjs";

describe("entryFromCommits", () => {
  it("shapes conventional commits, drops merges/release bumps, derives a summary", () => {
    const e = entryFromCommits({
      tag: "v0.5.0",
      channel: "beta",
      date: "May 1, 2026",
      ref: "abcdef1234567",
      commits: [
        { hash: "aaaaaaa1111", subject: "feat(updater): show a what's-new digest" },
        { hash: "bbbbbbb2222", subject: "Merge pull request #9 from x" },
        { hash: "ccccccc3333", subject: "release: v0.5.0" },
        { hash: "ddddddd4444", subject: "fix(ui): clamp progress bar" },
      ],
    });
    expect(e.tag).toBe("v0.5.0");
    expect(e.ref).toBe("abcdef1"); // shortHash (7)
    expect(e.commits.map((c) => c.type)).toEqual(["feat", "fix"]); // merge + release dropped
    expect(e.commits[0]).toMatchObject({ type: "feat", scope: "updater", hash: "aaaaaaa", msg: "show a what's-new digest" });
    expect(e.summary).toBe("show a what's-new digest"); // newest feat
  });

  it("honors an explicit summary over the derived one", () => {
    const e = entryFromCommits({
      tag: "v1",
      channel: "beta",
      date: "d",
      ref: "r",
      summary: "hand-written copy",
      commits: [{ hash: "h", subject: "feat: x" }],
    });
    expect(e.summary).toBe("hand-written copy");
  });
});

describe("upsertRelease", () => {
  it("prepends newest-first and replaces an existing entry for the same tag", () => {
    const a = { tag: "v1", commits: [] };
    const b = { tag: "v2", commits: [] };
    expect(upsertRelease([a], b).map((r) => r.tag)).toEqual(["v2", "v1"]);

    const b2 = { tag: "v2", commits: [{ type: "feat" }] };
    expect(upsertRelease([b, a], b2)).toEqual([b2, a]); // replaced, not duplicated
  });
});
