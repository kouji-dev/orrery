import { provideZonelessChangeDetection } from "@angular/core";
import { TestBed } from "@angular/core/testing";
import { afterEach, beforeEach, describe, expect, it } from "vitest";
import { treeAgentIds } from "../workspace/pane-model";
import { UiStore } from "./ui.store";

function make(): UiStore {
  TestBed.configureTestingModule({ providers: [provideZonelessChangeDetection(), UiStore] });
  return TestBed.inject(UiStore);
}

afterEach(() => TestBed.resetTestingModule());

describe("UiStore closeTabsForAgent", () => {
  let ui: UiStore;
  beforeEach(() => {
    localStorage.clear();
    ui = make();
  });

  it("closes a tab tiling only the removed agent and resets focus", () => {
    ui.openAgent("a1");
    const tabId = ui.activeTab();
    expect(ui.tabs().some((t) => t.id === tabId)).toBe(true);

    ui.closeTabsForAgent("a1");

    expect(ui.tabs().some((t) => t.id === tabId)).toBe(false);
    expect(ui.paneRoots()[tabId]).toBeUndefined();
    expect(ui.activeTab()).toBe("orchestrator");
    expect(ui.scopeAgentId()).toBeNull();
  });

  it("drops only the agent's panes from a grouped tab, keeping the tab alive", () => {
    ui.openAgent("a1");
    const tabId = ui.activeTab();
    ui.addAgentToTab("a2", tabId);

    ui.closeTabsForAgent("a1");

    expect(ui.tabs().some((t) => t.id === tabId)).toBe(true);
    expect(treeAgentIds(ui.paneRoots()[tabId])).toEqual(["a2"]);
  });

  it("leaves tabs of other agents and the pinned tabs untouched", () => {
    ui.openAgent("a1");
    ui.openAgent("a2");
    const otherTab = ui.activeTab();

    ui.closeTabsForAgent("a1");

    expect(ui.tabs().some((t) => t.id === otherTab)).toBe(true);
    expect(ui.tabs().some((t) => t.id === "orchestrator")).toBe(true);
    expect(ui.tabs().some((t) => t.id === "backlog")).toBe(true);
  });

  it("clears scope only when the removed agent was focused", () => {
    ui.openAgent("a1");
    ui.openAgent("a2"); // focus moves to a2
    ui.closeTabsForAgent("a1");
    expect(ui.scopeAgentId()).toBe("a2");
  });
});

describe("UiStore delete-worktree modal flag", () => {
  it("opens with the agent id and closes back to null", () => {
    const ui = make();
    expect(ui.deletingWorktree()).toBeNull();
    ui.openDeleteWorktree("a1");
    expect(ui.deletingWorktree()).toBe("a1");
    ui.closeDeleteWorktree();
    expect(ui.deletingWorktree()).toBeNull();
  });
});

describe("UiStore reorderTab", () => {
  let ui: UiStore;
  beforeEach(() => {
    localStorage.clear();
    ui = make();
  });
  const order = () => ui.tabs().map((t) => t.id);
  const open = (aid: string) => {
    ui.openAgent(aid);
    return ui.activeTab();
  };

  it("moves an agent tab before / after another agent tab", () => {
    const a = open("a1");
    const b = open("a2");
    const c = open("a3");
    ui.reorderTab(c, a, true);
    expect(order()).toEqual(["orchestrator", "backlog", c, a, b]);
    ui.reorderTab(a, b, false);
    expect(order()).toEqual(["orchestrator", "backlog", c, b, a]);
  });

  it("never lets an agent tab land in front of Orchestrator or Backlog", () => {
    const a = open("a1");
    const b = open("a2");
    ui.reorderTab(b, "orchestrator", true);
    expect(order()).toEqual(["orchestrator", "backlog", b, a]);
    ui.reorderTab(a, "backlog", true);
    expect(order()).toEqual(["orchestrator", "backlog", a, b]);
  });

  it("refuses to move the fixed tabs themselves", () => {
    const a = open("a1");
    ui.reorderTab("backlog", a, false);
    ui.reorderTab("orchestrator", a, false);
    expect(order()).toEqual(["orchestrator", "backlog", a]);
  });
});
