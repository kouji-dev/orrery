import { Component, provideZonelessChangeDetection } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { afterEach, describe, it, expect, beforeEach } from "vitest";
import { ReviewCodeComponent } from "./review-code.component";
import { ReviewStore } from "../../agents/review.store";
import { diffToHunks, diffToRows } from "./unified-diff";
import { IconComponent } from "../../shared/icon.component";

// Stub out IconComponent (uses signal input.required — NG0950 under JIT)
@Component({ selector: "app-icon", template: "", inputs: ["name", "size", "px", "color"] })
class IconStub {}

describe("ReviewCodeComponent", () => {
  let store: ReviewStore;
  function setup() {
    const f = TestBed.createComponent(ReviewCodeComponent);
    f.componentRef.setInput("agent", "a");
    f.componentRef.setInput("file", "src/x.ts");
    f.componentRef.setInput("view", "diff");
    f.componentRef.setInput("lang", "ts");
    f.componentRef.setInput("rows", diffToRows(diffToHunks("old\n", "new\n")));
    f.detectChanges();
    return f;
  }
  beforeEach(() => {
    TestBed.configureTestingModule({
      imports: [ReviewCodeComponent],
      providers: [provideZonelessChangeDetection()],
    });
    TestBed.overrideComponent(ReviewCodeComponent, {
      remove: { imports: [IconComponent] },
      add: { imports: [IconStub] },
    });
    store = TestBed.inject(ReviewStore);
  });
  afterEach(() => TestBed.resetTestingModule());

  it("hovering a code row then clicking + opens a composer; save persists a comment", () => {
    const f = setup();
    const el: HTMLElement = f.nativeElement;
    const codeRow = el.querySelectorAll<HTMLElement>("[data-rowkind='code']")[0];
    codeRow.dispatchEvent(new MouseEvent("mouseenter", { bubbles: true }));
    f.detectChanges();
    const plus = el.querySelector<HTMLButtonElement>("[data-plus]")!;
    plus.dispatchEvent(new MouseEvent("mousedown", { bubbles: true }));
    window.dispatchEvent(new MouseEvent("mouseup"));
    f.detectChanges();
    const ta = el.querySelector<HTMLTextAreaElement>("textarea")!;
    ta.value = "wrap it";
    ta.dispatchEvent(new Event("input", { bubbles: true }));
    ta.dispatchEvent(new KeyboardEvent("keydown", { key: "Enter", ctrlKey: true, bubbles: true }));
    f.detectChanges();
    expect(store.count("a")).toBe(1);
    expect(store.list("a")[0].note).toBe("wrap it");
    expect(el.textContent).toContain("wrap it"); // saved card rendered
  });
});
