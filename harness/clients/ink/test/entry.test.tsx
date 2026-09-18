/**
 * Task 1's other half: the two ways in, and what happens when neither is used.
 *
 * Open question 2 is answered here too: the Node floor is a message, not a
 * stack trace.
 */

import { describe, expect, it } from "vitest";

import {
  Misconfigured,
  NODE_FLOOR,
  checkNode,
  resolveEndpoint,
  resolveSession,
} from "../src/index.js";

describe("the entry point", () => {
  it("takes --endpoint", () => {
    expect(resolveEndpoint(["--endpoint", "http://127.0.0.1:7777"], {})).toBe(
      "http://127.0.0.1:7777",
    );
    expect(resolveEndpoint(["--endpoint=http://127.0.0.1:7777"], {})).toBe(
      "http://127.0.0.1:7777",
    );
  });

  it("falls back to ORRERY_ENDPOINT", () => {
    expect(resolveEndpoint([], { ORRERY_ENDPOINT: "http://127.0.0.1:9" })).toBe(
      "http://127.0.0.1:9",
    );
  });

  it("says what to do when neither is set", () => {
    expect(() => resolveEndpoint([], {})).toThrow(Misconfigured);
    try {
      resolveEndpoint([], {});
    } catch (error) {
      const message = (error as Error).message;
      expect(message).toContain("--endpoint");
      expect(message).toContain("ORRERY_ENDPOINT");
      expect(message).not.toContain("undefined");
    }
  });

  it("defaults the session", () => {
    expect(resolveSession([], {})).toBe("default");
    expect(resolveSession(["--session", "sess-1"], {})).toBe("sess-1");
    expect(resolveSession([], { ORRERY_SESSION: "sess-2" })).toBe("sess-2");
  });

  it("refuses an old Node in words", () => {
    expect(() => checkNode("18.20.0")).toThrow(/needs Node 20\.19\.0 or newer \(found 18\.20\.0\)/);
    expect(() => checkNode(NODE_FLOOR)).not.toThrow();
    expect(() => checkNode("22.0.0")).not.toThrow();
  });
});
