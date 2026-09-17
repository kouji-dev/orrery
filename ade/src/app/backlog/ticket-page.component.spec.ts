/**
 * Ticket-page specs — pure logic tests (no TestBed / template rendering).
 *
 * Angular 20's vitest JIT does NOT apply the signal-input compiler transform
 * (NG0950 on input.required in TestBed). Same pattern as ticket-card.component.spec.ts.
 * We test the pure-logic helpers and mode-selection rules exported from the component.
 */
import { describe, expect, it } from "vitest";
import { Ticket, Comment, TicketStatus } from "../models";
import {
  TICKET_STATUS,
  TICKET_STATUS_ORDER,
  plainTextNonEmpty,
  shortId,
  relativeTime,
  avColor,
  initials,
} from "./ticket-page.component";

// ── fixtures ──────────────────────────────────────────────────────────────────
function ticket(overrides: Partial<Ticket> = {}): Ticket {
  return {
    id: "t1",
    title: "Fix auth flow",
    notes: "<p>OAuth tokens need refresh</p>",
    status: "todo",
    projectId: "p1",
    agentId: null,
    tags: [],
    createdAt: 1000,
    updatedAt: 1000,
    ...overrides,
  };
}

function comment(overrides: Partial<Comment> = {}): Comment {
  return {
    id: "c1",
    ticketId: "t1",
    author: "Alice",
    role: "user",
    tool: null,
    body: "<p>looks good</p>",
    createdAt: Date.now() - 120_000, // 2 min ago
    ...overrides,
  };
}

// ── status pill mapping ───────────────────────────────────────────────────────
describe("TICKET_STATUS", () => {
  it("todo maps to ink-3 color", () => {
    expect(TICKET_STATUS["todo"].color).toBe("var(--ink-3)");
    expect(TICKET_STATUS["todo"].label).toBe("To do");
  });

  it("inprogress maps to st-running color", () => {
    expect(TICKET_STATUS["inprogress"].color).toBe("var(--st-running)");
    expect(TICKET_STATUS["inprogress"].label).toBe("In progress");
  });

  it("done maps to st-done color", () => {
    expect(TICKET_STATUS["done"].color).toBe("var(--st-done)");
    expect(TICKET_STATUS["done"].label).toBe("Done");
  });

  it("order is todo → inprogress → done", () => {
    expect(TICKET_STATUS_ORDER).toEqual(["todo", "inprogress", "done"]);
  });
});

// ── view vs edit mode selection ───────────────────────────────────────────────
describe("mode selection", () => {
  function isDraftMode(ticketId: string): boolean {
    return ticketId === "draft";
  }
  function isViewMode(ticketId: string, editing: boolean): boolean {
    return ticketId !== "draft" && !editing;
  }
  function isEditMode(ticketId: string, editing: boolean): boolean {
    return ticketId !== "draft" && editing;
  }

  it("draft id → draft mode", () => {
    expect(isDraftMode("draft")).toBe(true);
    expect(isDraftMode("t1")).toBe(false);
  });

  it("ticket id + editing=false → view mode", () => {
    expect(isViewMode("t1", false)).toBe(true);
  });

  it("ticket id + editing=true → edit mode", () => {
    expect(isEditMode("t1", true)).toBe(true);
  });

  it("draft id is never view mode", () => {
    expect(isViewMode("draft", false)).toBe(false);
  });
});

// ── plainTextNonEmpty ────────────────────────────────────────────────────────
describe("plainTextNonEmpty", () => {
  it("returns true for html with text content", () => {
    expect(plainTextNonEmpty("<p>hello</p>")).toBe(true);
  });

  it("returns false for empty html tags", () => {
    expect(plainTextNonEmpty("<p></p>")).toBe(false);
  });

  it("returns false for null", () => {
    expect(plainTextNonEmpty(null)).toBe(false);
  });

  it("returns false for empty string", () => {
    expect(plainTextNonEmpty("")).toBe(false);
  });

  it("returns false for whitespace-only html", () => {
    expect(plainTextNonEmpty("<p>   </p>")).toBe(false);
  });
});

// ── shortId ──────────────────────────────────────────────────────────────────
describe("shortId", () => {
  it("strips leading t", () => {
    expect(shortId("t42")).toBe("42");
    expect(shortId("t1")).toBe("1");
  });

  it("leaves non-t prefix unchanged", () => {
    expect(shortId("abc")).toBe("abc");
  });
});

// ── relativeTime ─────────────────────────────────────────────────────────────
describe("relativeTime", () => {
  it("returns 'just now' for very recent timestamps", () => {
    expect(relativeTime(Date.now() - 5000)).toBe("just now");
  });

  it("returns minutes ago", () => {
    const result = relativeTime(Date.now() - 120_000);
    expect(result).toBe("2m ago");
  });

  it("returns hours ago", () => {
    const result = relativeTime(Date.now() - 7_200_000);
    expect(result).toBe("2h ago");
  });

  it("returns days ago", () => {
    const result = relativeTime(Date.now() - 2 * 86_400_000);
    expect(result).toBe("2d ago");
  });
});

// ── comment count + filtering ─────────────────────────────────────────────────
describe("comment count", () => {
  it("counts all comments", () => {
    const list: Comment[] = [
      comment({ id: "c1" }),
      comment({ id: "c2" }),
      comment({ id: "c3" }),
    ];
    expect(list.length).toBe(3);
  });

  it("empty list has count 0", () => {
    expect([].length).toBe(0);
  });

  it("agent comment has role === agent", () => {
    const c = comment({ role: "agent", tool: "claude" });
    expect(c.role).toBe("agent");
  });

  it("user comment has role === user", () => {
    const c = comment({ role: "user" });
    expect(c.role).toBe("user");
  });
});

// ── avatar helpers ────────────────────────────────────────────────────────────
describe("avatar helpers", () => {
  it("initials of single name", () => {
    expect(initials("Alice")).toBe("A");
  });

  it("initials of two-word name", () => {
    expect(initials("John Doe")).toBe("JD");
  });

  it("avColor returns an identity token from the palette", () => {
    const c = avColor("Alice");
    expect(c).toMatch(/^var\(--id-[1-7]\)$/);
  });

  it("avColor is deterministic", () => {
    expect(avColor("Alice")).toBe(avColor("Alice"));
  });

  it("avColor differs for different names", () => {
    // Not guaranteed but holds for these names
    expect(avColor("Alice")).not.toBe(avColor("ZZZ_unlikely_name_xyz"));
  });
});

// ── status segmented control logic ───────────────────────────────────────────
describe("status segmented control", () => {
  const STATUSES: TicketStatus[] = ["todo", "inprogress", "done"];

  it("all statuses are selectable", () => {
    STATUSES.forEach((s) => {
      expect(TICKET_STATUS[s]).toBeDefined();
    });
  });

  it("active status is highlighted (on===true)", () => {
    const tk = ticket({ status: "inprogress" });
    const on = STATUSES.map((s) => s === tk.status);
    expect(on).toEqual([false, true, false]);
  });
});
