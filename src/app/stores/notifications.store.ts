import { computed, Injectable, signal } from "@angular/core";
import { AgentNotification, NotificationKind, NotificationStatus } from "../models";

/**
 * In-memory feed of agent notifications (questions, permission requests, work
 * done). Holds data + the user's decision only — side effects (sending a PTY
 * keystroke, opening the terminal, merging) are performed by NotificationService.
 */
@Injectable({ providedIn: "root" })
export class NotificationStore {
  private list = signal<AgentNotification[]>([]);
  private seq = 0;

  /** Newest first. */
  readonly all = computed(() => [...this.list()].sort((a, b) => b.createdAt - a.createdAt));
  readonly pending = computed(() => this.all().filter((n) => n.status === "pending"));
  readonly unread = computed(() => this.pending().length);

  /**
   * Add a notification. De-dupes a still-pending one of the same agent+kind so a
   * needs-input signal that re-fires can't stack duplicates.
   */
  push(
    input: {
      agentId: string;
      agentName: string;
      kind: NotificationKind;
      title: string;
      detail: string;
    } & Partial<
      Pick<
        AgentNotification,
        | "tool"
        | "command"
        | "description"
        | "filePath"
        | "mode"
        | "suggestions"
        | "summary"
        | "questions"
      >
    >,
  ): AgentNotification | null {
    const dup = this.list().find(
      (n) => n.agentId === input.agentId && n.kind === input.kind && n.status === "pending",
    );
    if (dup) return null;
    const note: AgentNotification = {
      id: `n${++this.seq}`,
      createdAt: Date.now(),
      status: "pending",
      ...input,
    };
    this.list.update((l) => [...l, note]);
    return note;
  }

  /** Record the user's decision (and resolve the notification). */
  decide(id: string, status: NotificationStatus, decision?: string) {
    this.list.update((l) => l.map((n) => (n.id === id ? { ...n, status, decision } : n)));
  }

  dismiss(id: string) {
    this.decide(id, "dismissed");
  }

  /** Auto-resolve an agent's pending notifications of the given kinds (e.g. it moved on). */
  dismissPendingFor(agentId: string, kinds: NotificationKind[]) {
    this.list.update((l) =>
      l.map((n) =>
        n.agentId === agentId && n.status === "pending" && kinds.includes(n.kind)
          ? { ...n, status: "dismissed" as const }
          : n,
      ),
    );
  }

  /** Drop every notification for an agent (e.g. when its worktree is removed). */
  clearAgent(agentId: string) {
    this.list.update((l) => l.filter((n) => n.agentId !== agentId));
  }

  /** Remove all resolved notifications, keeping only the pending ones. */
  clearResolved() {
    this.list.update((l) => l.filter((n) => n.status === "pending"));
  }
}
