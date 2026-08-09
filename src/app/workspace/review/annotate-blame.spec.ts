import { Component, provideZonelessChangeDetection } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { AnnotateBlameComponent, blameToRows } from "./annotate-blame.component";
import { IconComponent } from "../../shared/icon.component";
import { BlameLine } from "../../models";

// signal input.required components fail under vitest JIT — stub the icon
@Component({ selector: "app-icon", template: "", inputs: ["name", "size", "px", "color"] })
class IconStub {}

const bl = (n: number, sha: string, when: number, line: string): BlameLine => ({ n, sha, author: "Ann", when, summary: "msg", line });

describe("blameToRows", () => {
  it("flags first-of-commit rows and normalizes age 0..1", () => {
    const rows = blameToRows([bl(1, "aaa", 100, "x"), bl(2, "aaa", 100, "y"), bl(3, "bbb", 50, "z")]);
    expect(rows.map((r) => r.first)).toEqual([true, false, true]);
    expect(rows[0].age).toBeCloseTo(0, 5);   // newest
    expect(rows[2].age).toBeCloseTo(1, 5);   // oldest
    expect(rows[0].s).toBe("x");
  });

  it("makes rel time deterministic via now parameter", () => {
    // Fixed now = 1750768800000 ms
    const FIXED_NOW = 1750768800000;
    // when = 1750761600 (2 hours = 7200 seconds before FIXED_NOW)
    const WHEN_2H_AGO = 1750761600;
    const rows = blameToRows([bl(1, "aaa", WHEN_2H_AGO, "test")], FIXED_NOW);
    expect(rows[0].rel).toBe("2h");
  });
});

describe("AnnotateBlameComponent scoped find (B3.3)", () => {
  beforeEach(() => {
    // jsdom has no scrollIntoView; the reveal effect fires it in a microtask
    // after the test body, which vitest reports as an uncaught exception
    Element.prototype.scrollIntoView ??= () => {};
    TestBed.configureTestingModule({
      imports: [AnnotateBlameComponent],
      providers: [provideZonelessChangeDetection()],
    });
    TestBed.overrideComponent(AnnotateBlameComponent, {
      remove: { imports: [IconComponent] },
      add: { imports: [IconStub] },
    });
  });
  afterEach(() => TestBed.resetTestingModule());

  function render(lines: BlameLine[]) {
    const f = TestBed.createComponent(AnnotateBlameComponent);
    f.componentRef.setInput("lines", lines);
    f.detectChanges();
    return f;
  }

  const LINES = [
    bl(1, "aaa", 100, "const alpha = 1"),
    bl(2, "aaa", 100, "const beta = 2"),
    bl(3, "bbb", 50, "return alpha + beta"),
  ];

  it("matches rows case-insensitively and wraps next/prev", () => {
    const f = render(LINES);
    const c = f.componentInstance;
    c.findOpen.set(true);
    c.query.set("ALPHA");
    f.detectChanges();
    expect(c.matches()).toEqual([0, 2]);
    expect(c.isActiveHit(0)).toBe(true);
    c.next();
    expect(c.isActiveHit(2)).toBe(true);
    c.next(); // wraps
    expect(c.isActiveHit(0)).toBe(true);
    c.prev(); // wraps back
    expect(c.isActiveHit(2)).toBe(true);
  });

  it("renders hit + active classes and the match counter", () => {
    const f = render(LINES);
    const c = f.componentInstance;
    c.findOpen.set(true);
    c.query.set("beta");
    f.detectChanges();
    expect(f.nativeElement.querySelectorAll(".bf-hit").length).toBe(2);
    expect(f.nativeElement.querySelectorAll(".bf-on").length).toBe(1);
    expect(f.nativeElement.querySelector(".bf-count")?.textContent).toContain("1/2");
  });

  it("closeFind resets query and hides the bar", () => {
    const f = render(LINES);
    const c = f.componentInstance;
    c.findOpen.set(true);
    c.query.set("beta");
    f.detectChanges();
    c.closeFind();
    f.detectChanges();
    expect(f.nativeElement.querySelector(".bf-bar")).toBeNull();
    expect(c.matches()).toEqual([]);
  });
});
