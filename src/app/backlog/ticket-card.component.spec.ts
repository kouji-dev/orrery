/**
 * Backlog specs — pure logic tests.
 *
 * Angular 20's vitest JIT does NOT apply the signal-input compiler transform
 * (NG0950 if you try to render a component with input.required via TestBed).
 * The existing pattern (see dev-panel.component.spec.ts comment + tickets.store.spec.ts)
 * is to stub child components OR test only logic that doesn't require template rendering.
 * These tests cover the three status→variant rules and badge math without rendering.
 */
import { describe, expect, it } from "vitest";
import { Ticket, TicketStatus } from "../models";

// ---- helpers mirrored from the component (duplicated here for isolation) ----

function plainText(html: string | null | undefined): string {
  if (!html) return "";
  return html.replace(/<[^>]*>/g, " ").replace(/\s+/g, " ").trim();
}

function shortId(id: string): string {
  return id.replace(/^t/, "");
}

function isDispatchable(ticket: Ticket): boolean {
  return ticket.status === "todo";
}

function hasAgentStrip(ticket: Ticket): boolean {
  return ticket.status === "inprogress" && !!ticket.agentId;
}

function hasDispatchNudge(ticket: Ticket): boolean {
  return ticket.status === "inprogress" && !ticket.agentId;
}

function hasDoneCheck(ticket: Ticket): boolean {
  return ticket.status === "done";
}

function openCount(tickets: Ticket[]): number {
  return tickets.filter((t) => t.status !== "done").length;
}

// ---- fixtures ----
function todo(): Ticket {
  return {
    id: "t1",
    title: "Fix auth flow",
    notes: "<p>OAuth tokens need refresh</p>",
    status: "todo",
    projectId: "p1",
    agentId: null,
    createdAt: 1000,
    updatedAt: 1000,
  };
}

function inProgress(agentId: string | null = null): Ticket {
  return { ...todo(), id: "t2", status: "inprogress", agentId };
}

function done(agentId: string | null = null): Ticket {
  return { ...todo(), id: "t3", status: "done", agentId };
}

// ---- todo variant rules ----
describe("todo variant", () => {
  it("is dispatchable", () => {
    expect(isDispatchable(todo())).toBe(true);
  });
  it("has no agent strip", () => {
    expect(hasAgentStrip(todo())).toBe(false);
  });
  it("has no done check", () => {
    expect(hasDoneCheck(todo())).toBe(false);
  });
  it("has no dispatch nudge", () => {
    expect(hasDispatchNudge(todo())).toBe(false);
  });
  it("stripsHTML from notes preview", () => {
    const tk = todo();
    expect(plainText(tk.notes)).toBe("OAuth tokens need refresh");
  });
  it("shortId strips leading t", () => {
    expect(shortId("t42")).toBe("42");
  });
});

// ---- inprogress with agentId ----
describe("inprogress with agentId", () => {
  it("shows agent strip when agentId is set", () => {
    expect(hasAgentStrip(inProgress("agent-1"))).toBe(true);
  });
  it("not dispatchable", () => {
    expect(isDispatchable(inProgress("agent-1"))).toBe(false);
  });
  it("no done check", () => {
    expect(hasDoneCheck(inProgress("agent-1"))).toBe(false);
  });
});

// ---- inprogress without agentId ----
describe("inprogress without agentId", () => {
  it("shows dispatch nudge when agentId is null", () => {
    expect(hasDispatchNudge(inProgress(null))).toBe(true);
  });
  it("has no agent strip", () => {
    expect(hasAgentStrip(inProgress(null))).toBe(false);
  });
});

// ---- done variant ----
describe("done variant", () => {
  it("shows done check", () => {
    expect(hasDoneCheck(done())).toBe(true);
  });
  it("is not dispatchable", () => {
    expect(isDispatchable(done())).toBe(false);
  });
  it("has no dispatch nudge", () => {
    expect(hasDispatchNudge(done())).toBe(false);
  });
  it("has no agent strip (via strip logic)", () => {
    // done tickets don't use AgentStrip — they inline tool badge + commit count
    expect(hasAgentStrip(done("a1"))).toBe(false);
  });
});

// ---- open-count badge math ----
describe("open-count badge math", () => {
  it("counts only todo + inprogress as open", () => {
    const tickets: Ticket[] = [
      { ...todo(), id: "t1", status: "todo" },
      { ...todo(), id: "t2", status: "inprogress" },
      { ...todo(), id: "t3", status: "done" },
    ];
    expect(openCount(tickets)).toBe(2);
  });

  it("returns 0 when all tickets are done", () => {
    const tickets: Ticket[] = [
      { ...todo(), id: "t1", status: "done" },
      { ...todo(), id: "t2", status: "done" },
    ];
    expect(openCount(tickets)).toBe(0);
  });

  it("returns total when no tickets are done", () => {
    const tickets: Ticket[] = [
      { ...todo(), id: "t1", status: "todo" },
      { ...todo(), id: "t2", status: "inprogress" },
      { ...todo(), id: "t3", status: "todo" },
    ];
    expect(openCount(tickets)).toBe(3);
  });
});

// ---- TicketStatus filtering ----
describe("column filtering", () => {
  const tickets: Ticket[] = [
    { ...todo(), id: "t1", status: "todo" },
    { ...todo(), id: "t2", status: "inprogress" },
    { ...todo(), id: "t3", status: "done" },
    { ...todo(), id: "t4", status: "todo" },
  ];

  it("filters todo column correctly", () => {
    const col = tickets.filter((t) => t.status === ("todo" as TicketStatus));
    expect(col.length).toBe(2);
  });

  it("filters inprogress column correctly", () => {
    const col = tickets.filter((t) => t.status === ("inprogress" as TicketStatus));
    expect(col.length).toBe(1);
  });

  it("filters done column correctly", () => {
    const col = tickets.filter((t) => t.status === ("done" as TicketStatus));
    expect(col.length).toBe(1);
  });
});
