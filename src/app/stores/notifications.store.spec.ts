import { describe, it, expect } from "vitest";
import { NotificationStore } from "./notifications.store";

const base = {
  agentId: "a1",
  agentName: "nova",
  kind: "permission" as const,
  title: "needs permission",
  detail: "Bash: ls",
};

describe("NotificationStore", () => {
  it("de-dupes a still-pending notification of the same agent+kind", () => {
    const s = new NotificationStore();
    expect(s.push(base)).not.toBeNull();
    expect(s.push(base)).toBeNull(); // duplicate pending → dropped
    expect(s.pending().length).toBe(1);
  });

  it("allows a new notification once the prior one is resolved", () => {
    const s = new NotificationStore();
    const a = s.push(base)!;
    s.decide(a.id, "accepted");
    expect(s.push(base)).not.toBeNull(); // no longer pending → not a dup
    expect(s.pending().length).toBe(1);
  });

  it("decide resolves a notification out of the pending feed", () => {
    const s = new NotificationStore();
    const n = s.push(base)!;
    s.decide(n.id, "accepted", "accepted");
    expect(s.pending().length).toBe(0);
    expect(s.all()[0].status).toBe("accepted");
  });

  it("clearResolved keeps only pending notifications", () => {
    const s = new NotificationStore();
    const keep = s.push({ ...base, agentId: "keep" })!;
    const gone = s.push({ ...base, agentId: "gone" })!;
    s.decide(gone.id, "rejected");
    s.clearResolved();
    expect(s.all().map((n) => n.id)).toEqual([keep.id]);
  });
});
