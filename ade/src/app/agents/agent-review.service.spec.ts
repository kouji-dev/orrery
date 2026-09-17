import { Injector } from "@angular/core";
import { describe, it, expect, beforeEach } from "vitest";
import { AgentReviewService } from "./agent-review.service";
import { ReviewStore } from "./review.store";
import { AgentsStore } from "../stores/agents.store";
import { AgentRuntimeService } from "./agent-runtime.service";
import { UiStore } from "../ui/ui.store";

describe("AgentReviewService.sendReview", () => {
  let inputs: Array<{ id: string; data: string }>;
  let svc: AgentReviewService;
  let started: string[];

  beforeEach(() => {
    inputs = [];
    started = [];
    const agentsStore = { input: (id: string, data: string) => { inputs.push({ id, data }); return Promise.resolve(); } } as unknown as AgentsStore;
    const runtime = {
      agents: () => [{ id: "a", status: "running", started: true }],
      startProcess: (id: string) => started.push(id),
    } as unknown as AgentRuntimeService;
    const ui = { openAgent: () => {}, flash: () => {} } as unknown as UiStore;
    const injector = Injector.create({
      providers: [
        { provide: AgentsStore, useValue: agentsStore },
        { provide: AgentRuntimeService, useValue: runtime },
        { provide: UiStore, useValue: ui },
        ReviewStore,
        AgentReviewService,
      ],
    });
    svc = injector.get(AgentReviewService);
  });

  it("running agent: pastes bracketed-wrapped message then submits", async () => {
    svc.sendReview("a", { global: "", comments: [{ file: "f", fromLine: 1, toLine: 1, snippet: "s", note: "n", block: false }] });
    await Promise.resolve(); await Promise.resolve();
    expect(inputs[0].id).toBe("a");
    expect(inputs[0].data.startsWith("\x1b[200~")).toBe(true);
    expect(inputs[0].data.endsWith("\x1b[201~")).toBe(true);
    expect(inputs[0].data).toContain("f:1");
    expect(inputs[1].data).toBe("\r");
  });
});
