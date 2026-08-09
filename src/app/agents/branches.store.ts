import { inject, Injectable, signal } from "@angular/core";

import { BranchInfo, BRIDGE, Commands, RemoteInfo } from "../data-source/bridge";
import { UiStore } from "../ui/ui.store";

/**
 * Branches-panel state (A3.2): the project's local branches (with upstream /
 * ahead-behind / worktree occupancy) + remotes, and every native mutation.
 * Each op reloads the lists on success; failures surface via `ui.flash` and
 * resolve false so callers can keep their inline UI open.
 */
@Injectable({ providedIn: "root" })
export class BranchesStore {
  private bridge = inject(BRIDGE);
  private ui = inject(UiStore);

  readonly branches = signal<BranchInfo[]>([]);
  readonly remotes = signal<RemoteInfo[]>([]);
  readonly busy = signal(false);
  readonly loadedFor = signal<string | null>(null);

  async load(projectId: string): Promise<void> {
    this.busy.set(true);
    try {
      const [branches, remotes] = await Promise.all([
        this.bridge.invoke<BranchInfo[]>(Commands.ProjectBranchesDetail, { id: projectId }),
        this.bridge.invoke<RemoteInfo[]>(Commands.ProjectRemotes, { id: projectId }),
      ]);
      this.branches.set(branches);
      this.remotes.set(remotes);
      this.loadedFor.set(projectId);
    } catch {
      // not a repo / backend unavailable — panel falls back to detected names
      this.branches.set([]);
      this.remotes.set([]);
      this.loadedFor.set(null);
    } finally {
      this.busy.set(false);
    }
  }

  private async op(projectId: string, run: () => Promise<unknown>, done?: string): Promise<boolean> {
    if (this.busy()) return false;
    this.busy.set(true);
    try {
      await run();
      if (done) this.ui.flash(done);
      return true;
    } catch (e) {
      this.ui.flash(e instanceof Error ? e.message : String(e));
      return false;
    } finally {
      this.busy.set(false);
      void this.load(projectId);
    }
  }

  fetch(projectId: string, remote?: string): Promise<boolean> {
    return this.op(
      projectId,
      () => this.bridge.invoke(Commands.ProjectFetch, { id: projectId, remote: remote ?? null }),
      "Fetched" + (remote ? ` ${remote}` : ""),
    );
  }

  pull(projectId: string): Promise<boolean> {
    return this.op(
      projectId,
      () => this.bridge.invoke(Commands.ProjectPull, { id: projectId }),
      "Pulled (fast-forward)",
    );
  }

  pullAgent(projectId: string, agentId: string): Promise<boolean> {
    return this.op(
      projectId,
      () => this.bridge.invoke(Commands.AgentPull, { id: agentId }),
      "Pulled worktree (fast-forward)",
    );
  }

  create(projectId: string, name: string, from?: string): Promise<boolean> {
    return this.op(
      projectId,
      () =>
        this.bridge.invoke(Commands.ProjectBranchCreate, {
          id: projectId,
          name,
          from: from ?? null,
        }),
      `Created ${name}`,
    );
  }

  rename(projectId: string, old: string, next: string): Promise<boolean> {
    return this.op(
      projectId,
      () => this.bridge.invoke(Commands.ProjectBranchRename, { id: projectId, old, new: next }),
      `Renamed to ${next}`,
    );
  }

  delete(projectId: string, name: string, force = false): Promise<boolean> {
    return this.op(
      projectId,
      () => this.bridge.invoke(Commands.ProjectBranchDelete, { id: projectId, name, force }),
      `Deleted ${name}`,
    );
  }

  setUpstream(projectId: string, name: string, upstream: string | null): Promise<boolean> {
    return this.op(
      projectId,
      () => this.bridge.invoke(Commands.ProjectBranchUpstream, { id: projectId, name, upstream }),
      upstream ? `Upstream → ${upstream}` : "Upstream cleared",
    );
  }

  checkoutAgent(projectId: string, agentId: string, branch: string): Promise<boolean> {
    return this.op(
      projectId,
      () => this.bridge.invoke(Commands.AgentCheckout, { id: agentId, branch }),
      `Checked out ${branch}`,
    );
  }
}
