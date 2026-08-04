import { beforeEach, describe, expect, it } from "vitest";
import { liveEditorCount, MAX_LIVE_EDITORS, registerEditor, resetEditorCap } from "./editor-cap";

describe("editor-cap (A0.6 concurrent CodeMirror bound)", () => {
  beforeEach(() => resetEditorCap());

  it("keeps registrations below or at the cap without demoting", () => {
    const demoted: number[] = [];
    for (let i = 0; i < MAX_LIVE_EDITORS; i++) {
      registerEditor(() => demoted.push(i));
    }
    expect(liveEditorCount()).toBe(MAX_LIVE_EDITORS);
    expect(demoted).toEqual([]);
  });

  it("demotes the OLDEST editor when the cap is exceeded", () => {
    const demoted: number[] = [];
    for (let i = 0; i < MAX_LIVE_EDITORS + 2; i++) {
      registerEditor(() => demoted.push(i));
    }
    expect(demoted).toEqual([0, 1]); // oldest-first demotion
    expect(liveEditorCount()).toBe(MAX_LIVE_EDITORS);
  });

  it("unregister removes the entry so it is never demoted", () => {
    const demoted: number[] = [];
    const un0 = registerEditor(() => demoted.push(0));
    for (let i = 1; i < MAX_LIVE_EDITORS; i++) {
      registerEditor(() => demoted.push(i));
    }
    un0(); // editor 0 destroyed through the normal path
    expect(liveEditorCount()).toBe(MAX_LIVE_EDITORS - 1);
    registerEditor(() => demoted.push(99)); // back at the cap — no demote
    expect(demoted).toEqual([]);
    registerEditor(() => demoted.push(100)); // over cap → oldest LIVE (1)
    expect(demoted).toEqual([1]);
  });

  it("unregister after demotion is a harmless no-op", () => {
    const unregs: Array<() => void> = [];
    for (let i = 0; i < MAX_LIVE_EDITORS + 1; i++) {
      unregs.push(registerEditor(() => {}));
    }
    expect(liveEditorCount()).toBe(MAX_LIVE_EDITORS);
    unregs[0](); // entry 0 was already demoted+removed
    expect(liveEditorCount()).toBe(MAX_LIVE_EDITORS);
  });
});
