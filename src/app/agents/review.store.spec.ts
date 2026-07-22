import { Injector, runInInjectionContext } from "@angular/core";
import { describe, it, expect, beforeEach } from "vitest";
import { ReviewStore, assembleReviewMessage } from "./review.store";

function base(file = "src/a.ts") {
  return { file, view: "diff" as const, lang: "ts", fromLine: 42, toLine: 42, side: "new" as const, snippet: "const t = parse(x)", lines: ["const t = parse(x)"], note: "wrap it" };
}

describe("ReviewStore", () => {
  let store: ReviewStore;
  beforeEach(() => {
    const injector = Injector.create({ providers: [] });
    store = runInInjectionContext(injector, () => new ReviewStore());
  });

  it("add/list/count/remove/clear scoped per agent", () => {
    const id = store.add("a", base());
    store.add("a", base("src/b.ts"));
    store.add("z", base());
    expect(store.count("a")).toBe(2);
    expect(store.count("z")).toBe(1);
    store.remove("a", id);
    expect(store.count("a")).toBe(1);
    store.clear("a");
    expect(store.count("a")).toBe(0);
    expect(store.count("z")).toBe(1); // other agents untouched
  });

  it("buildPayload maps comments + flags blocks", () => {
    store.add("a", { ...base(), fromLine: 10, toLine: 12, lines: ["x", "y", "z"] });
    const p = store.buildPayload("a", "  tighten  ");
    expect(p.global).toBe("tighten");
    expect(p.comments[0]).toMatchObject({ file: "src/a.ts", fromLine: 10, toLine: 12, block: true });
  });
});

describe("assembleReviewMessage", () => {
  it("renders the exact structured message", () => {
    const msg = assembleReviewMessage({
      global: "tighten error handling",
      comments: [
        { file: "src/auth.ts", fromLine: 42, toLine: 42, snippet: "const t = parse(x)", note: "this can throw, wrap it", block: false },
        { file: "src/api.ts", fromLine: 10, toLine: 13, snippet: "fetch(u)", note: "extract a helper", block: true },
      ],
    });
    expect(msg).toBe(
      [
        "Review feedback:",
        "[general] tighten error handling",
        "",
        "src/auth.ts:42",
        "  → this can throw, wrap it",
        "src/api.ts:10-13",
        "  → extract a helper",
      ].join("\n"),
    );
  });

  it("omits the global line when empty", () => {
    const msg = assembleReviewMessage({ global: "", comments: [{ file: "f", fromLine: 1, toLine: 1, snippet: "s", note: "n", block: false }] });
    expect(msg.startsWith("Review feedback:\n\nf:1")).toBe(true);
  });
});
