import { describe, expect, it } from "vitest";
import { MENU_GAP, MenuAnchor, placeMenu } from "./place";

const VP = { w: 1280, h: 720 };
const SIZE = { w: 200, h: 120 };
const at = (x: number, y: number, over: Partial<MenuAnchor> = {}): MenuAnchor => ({
  x,
  y,
  alignX: "left",
  flipY: null,
  ...over,
});

describe("placeMenu", () => {
  it("leaves a menu that fits exactly where it was asked for", () => {
    expect(placeMenu(at(400, 300), SIZE, VP)).toEqual({ x: 400, y: 300, bottom: null });
  });

  it("flips LEFT of the anchor when it would overflow the right edge", () => {
    // 1200 + 200 = 1400 > 1280 → right edge lands on the anchor
    expect(placeMenu(at(1200, 100), SIZE, VP).x).toBe(1000);
  });

  it("clamps instead of flipping when neither side has room", () => {
    const narrow = { w: 300, h: 80 };
    // anchor at 250: right side overflows (250+300=550 > 400-8), flipping puts
    // the panel at -50 → clamp to the right edge minus the gap
    const p = placeMenu(at(250, 10), narrow, { w: 400, h: 720 });
    expect(p.x).toBe(400 - 300 - MENU_GAP);
  });

  it("never leaves the left edge", () => {
    expect(placeMenu(at(-40, 100), SIZE, VP).x).toBe(MENU_GAP);
  });

  it("flips UP above the anchor when it would overflow the bottom", () => {
    // 680 + 120 = 800 > 720 → bottom edge lands on the anchor
    expect(placeMenu(at(100, 680), SIZE, VP)).toEqual({ x: 100, y: 560, bottom: null });
  });

  it("clamps upward when there is no room above either", () => {
    const tall = { w: 200, h: 700 };
    const p = placeMenu(at(100, 60), tall, VP);
    expect(p.y).toBe(720 - 700 - MENU_GAP);
    expect(p.bottom).toBeNull();
  });

  it("pins the BOTTOM to flipY when the caller gave an anchor element", () => {
    // dropup: anchor control's top at 600, panel 120 tall
    const p = placeMenu(at(100, 640, { flipY: 600 }), SIZE, VP);
    expect(p.bottom).toBe(720 - 600);
  });

  it("falls back to the point flip when the flipY anchor leaves no room above it", () => {
    const tall = { w: 200, h: 500 };
    const p = placeMenu(at(100, 640, { flipY: 300 }), tall, VP);
    expect(p.bottom).toBeNull();
    expect(p.y).toBe(140); // 640 - 500, still fully inside
    expect(p.y + tall.h).toBeLessThanOrEqual(VP.h - MENU_GAP);
  });

  it("alignX='right' hangs the panel's right edge on the anchor, and flips left-aligned on overflow", () => {
    expect(placeMenu(at(500, 100, { alignX: "right" }), SIZE, VP).x).toBe(300);
    // anchor sits so far right that even the right-aligned box overflows only
    // when the panel is wider than the anchor's offset from the edge
    expect(placeMenu(at(100, 100, { alignX: "right" }), SIZE, VP).x).toBe(MENU_GAP);
  });

  it("keeps a corner-opened menu fully inside the viewport", () => {
    const p = placeMenu(at(VP.w - 2, VP.h - 2), SIZE, VP);
    expect(p.x).toBeGreaterThanOrEqual(MENU_GAP);
    expect(p.x + SIZE.w).toBeLessThanOrEqual(VP.w - MENU_GAP);
    expect(p.y).toBeGreaterThanOrEqual(MENU_GAP);
    expect(p.y + SIZE.h).toBeLessThanOrEqual(VP.h - MENU_GAP);
  });
});
