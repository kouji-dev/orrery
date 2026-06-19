import { inject, Injectable } from "@angular/core";
import { AgentsStore } from "../stores/agents.store";
import { AgentRuntimeService } from "./agent-runtime.service";
import { UiStore } from "../ui/ui.store";
import { assembleReviewMessage, ReviewPayload } from "./review.store";

const PASTE_START = "\x1b[200~";
const PASTE_END = "\x1b[201~";

/** Delivers an assembled review to the agent's PTY as one bracketed paste. */
@Injectable({ providedIn: "root" })
export class AgentReviewService {
  private agents = inject(AgentsStore);
  private runtime = inject(AgentRuntimeService);
  private ui = inject(UiStore);

  sendReview(agentId: string, payload: ReviewPayload): void {
    if (!payload.comments.length) return;
    const text = assembleReviewMessage(payload);
    const ag = this.runtime.agents().find((a) => a.id === agentId);
    this.ui.openAgent(agentId, "terminal");
    const send = () =>
      void this.agents
        .input(agentId, PASTE_START + text + PASTE_END)
        .then(() => this.agents.input(agentId, "\r"))
        .catch((e: { message?: string }) => this.ui.flash(e?.message ?? "send failed"));
    if (ag && ag.status === "running") {
      send();
    } else {
      this.runtime.startProcess(agentId, { resume: !!ag?.started });
      setTimeout(send, 1800);
    }
  }
}
