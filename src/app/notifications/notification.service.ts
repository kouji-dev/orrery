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
 * Hooks are fire-and-forget (we don't forward the decision yet — like orca), so
 * Accept/Reject send a best-effort PTY keystroke"Open terminal" is the fully
 * reliable path where the user approves in the agent's own TUI.
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

  /** Accept: send the tool's correct ALLOW keystrokes (approve in the terminal for real). */
  accept(n: AgentNotification) {
    if (n.kind === "permission") {
      void this.agentsStore.allow(n.agentId).catch(() => {});
    }
    this.store.decide(n.id, "accepted", "accepted");
    this.ui.flash("accepted · " + n.agentName);
  }

  /** Reject: send the tool's correct DENY keystrokes (Esc for SELECT-style tools)
   *  to decline/cancel the prompt for real. Used for both permission requests and
   *  AskUserQuestion-style questions (Esc cancels the numbered select). */
  reject(n: AgentNotification) {
    if (n.kind === "permission" || n.kind === "question") {
      void this.agentsStore.deny(n.agentId).catch(() => {});
    }
    this.store.decide(n.id, "rejected", "rejected");
    this.ui.flash("rejected · " + n.agentName);
  }

  /** Open the agent's terminal for full context (and resolve a question). */
  open(n: AgentNotification) {
    this.ui.openAgent(n.agentId, "terminal");
    if (n.kind === "question") this.store.decide(n.id, "accepted", "opened");
  }

  // A finished agent's work is pushed to origin (deterministic) so the user can
  // open the PR — orrery no longer auto-merges into the base branch.
  push(n: AgentNotification) {
    this.agents.pushAgent(n.agentId);
    this.store.decide(n.id, "accepted", "push");
  }

  review(n: AgentNotification) {
    this.ui.openAgent(n.agentId, "diff");
    this.store.decide(n.id, "accepted", "review");
  }

  dismiss(n: AgentNotification) {
    this.store.dismiss(n.id);
  }
}
