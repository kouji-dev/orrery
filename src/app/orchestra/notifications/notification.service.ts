import { inject, Injectable } from "@angular/core";
import { AgentNotification } from "../models";
import { NotificationStore } from "../stores/notifications.store";
import { AgentsStore } from "../stores/agents.store";
import { UiStore } from "../ui/ui.store";
import { AgentActionsService } from "../agents/agent-actions.service";

/**
 * User-facing actions on agent notifications. The feed itself lives in
 * NotificationStore (filled by the backend hook bridge via AgentRuntimeService);
 * this service performs the side effect of a decision and records it.
 *
 * For a hook-driven permission (`requestId` present), Accept/Reject resolve the
 * held tool call through the backend — the agent then proceeds or refuses for
 * real. Without a requestId (un-hooked tools, e.g. gemini) we fall back to a
 * best-effort PTY keystroke; "Open terminal" stays the fully reliable path.
 */
@Injectable({ providedIn: "root" })
export class NotificationService {
  private store = inject(NotificationStore);
  private agentsStore = inject(AgentsStore);
  private agents = inject(AgentActionsService);
  private ui = inject(UiStore);

  readonly all = this.store.all;
  readonly pending = this.store.pending;
  readonly unread = this.store.unread;

  clearResolved() {
    this.store.clearResolved();
  }

  /** Accept a permission request: resolve the held hook (or fall back to a keystroke). */
  accept(n: AgentNotification) {
    if (n.kind === "permission") this.resolvePermission(n, true);
    this.store.decide(n.id, "accepted", "accepted");
    this.ui.flash("accepted · " + n.agentName);
  }

  /** Reject a permission request: deny the held hook (or fall back to a keystroke). */
  reject(n: AgentNotification) {
    if (n.kind === "permission") this.resolvePermission(n, false);
    this.store.decide(n.id, "rejected", "rejected");
    this.ui.flash("rejected · " + n.agentName);
  }

  private resolvePermission(n: AgentNotification, allow: boolean) {
    if (n.requestId) {
      void this.agentsStore.permissionDecide(n.requestId, allow).catch(() => {});
    } else {
      void this.agentsStore.input(n.agentId, allow ? "y\r" : "n\r").catch(() => {});
    }
  }

  /** Open the agent's terminal for full context (and resolve a question). */
  open(n: AgentNotification) {
    this.ui.openAgent(n.agentId, "terminal");
    if (n.kind === "question") this.store.decide(n.id, "accepted", "opened");
  }

  merge(n: AgentNotification) {
    this.agents.mergeAgent(n.agentId);
    this.store.decide(n.id, "accepted", "merge");
  }

  review(n: AgentNotification) {
    this.ui.openAgent(n.agentId, "diff");
    this.store.decide(n.id, "accepted", "review");
  }

  dismiss(n: AgentNotification) {
    this.store.dismiss(n.id);
  }
}
