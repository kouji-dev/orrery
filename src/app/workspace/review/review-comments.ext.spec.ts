import { describe, it, expect, beforeEach, afterEach, vi } from "vitest";
import * as cmState from "@codemirror/state";
import * as cmView from "@codemirror/view";
import * as cmLanguage from "@codemirror/language";
import * as cmMerge from "@codemirror/merge";
import type { CMCore } from "../code-lang";
import { clampComments, reviewCommentsExt, ReviewCommentsApi, CommentHost } from "./review-comments.ext";

// The runtime loads CM from esm.sh; under vitest the same modules resolve from
// local node_modules — hand them to the factory as a CMCore.
const core = { state: cmState, view: cmView, language: cmLanguage, merge: cmMerge, themeOneDark: {} } as unknown as CMCore;

describe("clampComments", () => {
  const c = (fromLine: number, toLine: number) => ({ id: "x", fromLine, toLine, note: "n" });

  it("keeps in-range, clamps overhang, drops out-of-range", () => {
    expect(clampComments([c(1, 3)], 10)).toEqual([c(1, 3)]);
    expect(clampComments([c(8, 14)], 10)).toEqual([c(8, 10)]);
    expect(clampComments([c(11, 12)], 10)).toEqual([]);
    expect(clampComments([c(0, 2)], 10)).toEqual([]);
  });
});

describe("reviewCommentsExt", () => {
  let host: CommentHost;
  let api: ReviewCommentsApi;
  let view: cmView.EditorView;

  beforeEach(() => {
    host = { save: vi.fn(), remove: vi.fn() };
    api = reviewCommentsExt(core, host);
    view = new cmView.EditorView({
      doc: "alpha\nbeta\ngamma\ndelta",
      extensions: api.extension,
      parent: document.body,
    });
  });
  afterEach(() => view.destroy());

  it("renders a saved-comment card as a block widget with note + pending chip", () => {
    api.setComments(view, [{ id: "rc1", fromLine: 2, toLine: 3, note: "tighten this" }]);
    const card = view.dom.querySelector(".rc-card");
    expect(card).toBeTruthy();
    expect(card!.textContent).toContain("tighten this");
    expect(card!.textContent).toContain("pending");
    expect(card!.textContent).toContain("lines 2-3");
  });

  it("card delete button calls host.remove with the comment id", () => {
    api.setComments(view, [{ id: "rc9", fromLine: 1, toLine: 1, note: "n" }]);
    (view.dom.querySelector(".rc-card-del") as HTMLButtonElement).click();
    expect(host.remove).toHaveBeenCalledWith("rc9");
  });

  it("covered lines get the rc-covered tint", () => {
    api.setComments(view, [{ id: "rc1", fromLine: 2, toLine: 3, note: "n" }]);
    expect(view.dom.querySelectorAll(".rc-covered").length).toBe(2);
  });

  it("out-of-doc comments are clamped, not crashing the deco build", () => {
    api.setComments(view, [{ id: "rc1", fromLine: 3, toLine: 99, note: "overhang" }]);
    expect(view.dom.querySelector(".rc-card")).toBeTruthy();
    api.setComments(view, [{ id: "rc2", fromLine: 50, toLine: 60, note: "gone" }]);
    expect(view.dom.querySelector(".rc-card")).toBeNull();
  });

  it("openComposer renders the composer over the range with tint", () => {
    api.openComposer(view, 2, 3);
    const composer = view.dom.querySelector(".rc-composer");
    expect(composer).toBeTruthy();
    expect(composer!.textContent).toContain("lines 2–3");
    expect(view.dom.querySelectorAll(".rc-selected").length).toBe(2);
  });

  it("composer Save calls host.save with range + note and closes", () => {
    api.openComposer(view, 3, 2); // reversed range is normalized
    const ta = view.dom.querySelector(".rc-composer-ta") as HTMLTextAreaElement;
    ta.value = "  extract a helper  ";
    ta.dispatchEvent(new Event("input"));
    const save = view.dom.querySelectorAll(".rc-composer-btn")[1] as HTMLButtonElement;
    expect(save.disabled).toBe(false);
    save.click();
    expect(host.save).toHaveBeenCalledWith(2, 3, "extract a helper");
    expect(view.dom.querySelector(".rc-composer")).toBeNull();
  });

  it("composer Escape cancels without saving", () => {
    api.openComposer(view, 1, 1);
    const ta = view.dom.querySelector(".rc-composer-ta") as HTMLTextAreaElement;
    ta.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape" }));
    expect(host.save).not.toHaveBeenCalled();
    expect(view.dom.querySelector(".rc-composer")).toBeNull();
  });

  it("Ctrl+Enter in the textarea saves", () => {
    api.openComposer(view, 4, 4);
    const ta = view.dom.querySelector(".rc-composer-ta") as HTMLTextAreaElement;
    ta.value = "note";
    ta.dispatchEvent(new Event("input"));
    ta.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", ctrlKey: true }));
    expect(host.save).toHaveBeenCalledWith(4, 4, "note");
  });
});
