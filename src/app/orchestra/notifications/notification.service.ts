import { inject, Injectable } from "@angular/core";
import { AgentNotification } from "../models";
import { NotificationStore } from "../stores/notifications.store";
import { AgentsStore } from "../stores/agents.store";
import { UiStore } from "../ui/ui.store";
import { AgentActionsService } from "../agents/agent-actions.service";

/**
 * User-facing actions on agent notifications. The feed itself lives in
 * NotificationStore (and is filled by AgentRuntimeService today, the backend
 * later); this service performs the side effect of a decision and records it.
 *
 * NOTE: forwarding "accept" as a PTY keystroke is best-effort and tool-specific
 * (claude uses an arrow-key menu, not y/n) — "Open terminal" is the reliable
 * path. Revisited when notifications move backend-side.
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

  /** Accept a permission request: send an affirmative keystroke to the PTY. */
  accept(n: AgentNotification) {
    if (n.kind === "permission") {
      void this.agentsStore.input(n.agentId, "y\r").catch(() => {});
    }
    this.store.decide(n.id, "accepted", "accepted");
    this.ui.flash("accepted · " + n.agentName);
  }

  /** Reject a permission request: send a negative keystroke to the PTY. */
  reject(n: AgentNotification) {
    if (n.kind === "permission") {
      void this.agentsStore.input(n.agentId, "n\r").catch(() => {});
    }
    this.store.decide(n.id, "rejected", "rejected");
    this.ui.flash("rejected · " + n.agentName);
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
