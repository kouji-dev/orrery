import { describe, expect, it, vi } from "vitest";

import { buildCard, buildComposer } from "./review-comments.monaco";

// The full controller needs a live Monaco editor (WebView-only — covered by
// the manual tauri-dev checkpoint). The DOM factories carry the behavioral
// contract from review-comments.ext.spec.ts, so they are what runs in jsdom.

describe("buildCard", () => {
  it("renders note, pending chip, and the line range", () => {
    const card = buildCard({ id: "rc1", fromLine: 2, toLine: 3, note: "tighten this" }, vi.fn());
    expect(card.className).toBe("rc-card");
    expect(card.textContent).toContain("tighten this");
    expect(card.textContent).toContain("pending");
    expect(card.textContent).toContain("lines 2-3");
    const single = buildCard({ id: "rc2", fromLine: 5, toLine: 5, note: "n" }, vi.fn());
    expect(single.textContent).toContain("line 5");
  });

  it("delete button calls back with the comment id", () => {
    const onDelete = vi.fn();
    const card = buildCard({ id: "rc9", fromLine: 1, toLine: 1, note: "n" }, onDelete);
    (card.querySelector(".rc-card-del") as HTMLButtonElement).click();
    expect(onDelete).toHaveBeenCalledWith("rc9");
  });
});

describe("buildComposer", () => {
  it("labels the range and disables Save until there is a note", () => {
    const composer = buildComposer(2, 3, { save: vi.fn(), cancel: vi.fn() });
    expect(composer.textContent).toContain("lines 2–3");
    const save = composer.querySelectorAll(".rc-composer-btn")[1] as HTMLButtonElement;
    expect(save.disabled).toBe(true);
    const ta = composer.querySelector(".rc-composer-ta") as HTMLTextAreaElement;
    ta.value = "x";
    ta.dispatchEvent(new Event("input"));
    expect(save.disabled).toBe(false);
  });

  it("Save trims the note and calls back; empty notes never save", () => {
    const save = vi.fn();
    const composer = buildComposer(2, 3, { save, cancel: vi.fn() });
    const ta = composer.querySelector(".rc-composer-ta") as HTMLTextAreaElement;
    const btn = composer.querySelectorAll(".rc-composer-btn")[1] as HTMLButtonElement;
    btn.click();
    expect(save).not.toHaveBeenCalled();
    ta.value = "  extract a helper  ";
    ta.dispatchEvent(new Event("input"));
    btn.click();
    expect(save).toHaveBeenCalledWith("extract a helper");
  });

  it("Escape cancels, Ctrl+Enter saves", () => {
    const save = vi.fn();
    const cancel = vi.fn();
    const composer = buildComposer(4, 4, { save, cancel });
    const ta = composer.querySelector(".rc-composer-ta") as HTMLTextAreaElement;
    ta.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(cancel).toHaveBeenCalled();
    ta.value = "note";
    ta.dispatchEvent(new Event("input"));
    ta.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", ctrlKey: true }));
    expect(save).toHaveBeenCalledWith("note");
  });
});
