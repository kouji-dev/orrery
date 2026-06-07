import { Injectable, signal } from "@angular/core";

/**
 * What's currently being dragged across the shell. Set on dragstart (a sidebar
 * agent row, a rail popover row, or a top-bar tab) and read on drop (a pane, or
 * another tab). A tab drag also carries its first agent id so it can be dropped
 * into a split pane like a plain agent.
 */
export interface DragPayload {
  kind: "agent" | "tab";
  agentId: string | null;
  tabId?: string;
}

@Injectable({ providedIn: "root" })
export class DragService {
  readonly payload = signal<DragPayload | null>(null);

  start(p: DragPayload) {
    this.payload.set(p);
  }
  end() {
    this.payload.set(null);
  }
}
