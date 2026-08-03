import { Injectable, signal } from "@angular/core";

/** The panels the bottom tool window can show (design toolwindow.jsx
 *  TOOL_ORDER, git panels first — search/cost/github register here when their
 *  features land). */
export type ToolPanelKind = "graph" | "branches" | "history";

/** One registered dock panel (tab strip renders FROM this list). */
export interface ToolPanelDef {
  kind: ToolPanelKind;
  label: string;
  icon: string;
}

export const TOOL_PANELS: ToolPanelDef[] = [
  { kind: "graph", label: "Git Graph", icon: "graph" },
  { kind: "branches", label: "Branches", icon: "branch" },
  { kind: "history", label: "Local History", icon: "clock" },
];

/**
 * State of the IntelliJ-style bottom tool window (design toolwindow.jsx):
 * which panel is open (null = dock hidden). Kept out of UiStore so the dock
 * stays self-contained; commands in the registry drive open/toggle.
 */
@Injectable({ providedIn: "root" })
export class ToolWindowStore {
  /** The open panel, or null when the dock is hidden. Closed by default. */
  readonly panel = signal<ToolPanelKind | null>(null);

  open(kind: ToolPanelKind): void {
    this.panel.set(kind);
  }

  /** Command semantics: focus the panel; pressing again while it is the open
   *  panel hides the dock (IntelliJ tool-window toggle behavior). */
  toggle(kind: ToolPanelKind): void {
    this.panel.update((cur) => (cur === kind ? null : kind));
  }

  close(): void {
    this.panel.set(null);
  }
}
