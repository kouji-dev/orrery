import { Injectable, computed, inject, signal } from "@angular/core";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { Agent, Project } from "../models";
import { ProjectActionsService } from "../projects/project-actions.service";
import { pseudoProjectAgent } from "../projects/pseudo-agent";

/** How wide a panel searches: the scoped worktree, every worktree of the
 *  scoped project, or all of them. */
export type ScopeKind = "worktree" | "project" | "all";

/**
 * The project/worktree a panel acts on.
 *
 * A tab can tile agents from several projects, so a panel cannot just read the
 * focused agent: two docked panels would silently disagree about "here". Each
 * panel therefore owns an EXPLICIT scope that by default FOLLOWS the focused
 * agent, and stops following the moment the user picks from either select
 * (`outOfSync()` then offers the way back).
 *
 * Not `providedIn` anything on purpose: every consumer decides its own
 * lifetime — `providers: [ScopeSelection]` for a panel-local scope, or the
 * root `LookupScopeStore` below for one shared across overlays.
 */
@Injectable()
export class ScopeSelection {
  private readonly projects = inject(ProjectActionsService);
  private readonly runtime = inject(AgentRuntimeService);

  // null = follow the focused agent (the default); set by the two selects.
  private readonly explicit = signal<{ projectId: string | null; agentId: string | null }>({
    projectId: null,
    agentId: null,
  });

  /** The focused agent of the active tab (what the scope follows by default). */
  readonly focus = computed<Agent | null>(() => this.runtime.activeAgent());

  readonly project = computed<Project | undefined>(() => {
    const all = this.projects.all();
    const ex = this.explicit();
    return (
      (ex.projectId ? all.find((p) => p.id === ex.projectId) : undefined) ??
      all.find((p) => p.id === this.focus()?.projectId) ??
      all[0]
    );
  });

  readonly projAgents = computed<Agent[]>(() => {
    const p = this.project();
    return p ? this.runtime.agents().filter((a) => a.projectId === p.id) : [];
  });

  readonly projectOptions = computed(() => this.projects.all().map((p) => ({ value: p.id, label: p.name })));

  /** The scoped project's MAIN checkout as a pseudo-agent (id = project id) —
   *  the worktree select offers it first, so a project tab's focus resolves to
   *  the checkout itself rather than falling back to some agent worktree. */
  readonly mainPseudo = computed<Agent | null>(() => {
    const p = this.project();
    return p ? pseudoProjectAgent(p, this.runtime.shellRunning(p.id)) : null;
  });

  /** Options for the worktree select: the main checkout, then agent worktrees. */
  readonly scopeAgents = computed<Agent[]>(() => {
    const ps = this.mainPseudo();
    return ps ? [ps, ...this.projAgents()] : this.projAgents();
  });

  readonly agentOptions = computed(() => {
    const pid = this.project()?.id;
    const list = this.scopeAgents();
    return list.length
      ? list.map((a) => ({ value: a.id, label: a.id === pid ? "checkout · " + a.branch : a.name }))
      : [{ value: "", label: "no worktrees" }];
  });

  readonly agent = computed<Agent | null>(() => {
    const list = this.scopeAgents();
    const ex = this.explicit();
    return (
      (ex.agentId ? list.find((a) => a.id === ex.agentId) : undefined) ??
      list.find((a) => a.id === this.focus()?.id) ??
      list[0] ??
      null
    );
  });

  /** The scoped agent for panels that treat "no agent" as the project checkout
   *  (branches): the pseudo maps back to null there. */
  readonly realAgent = computed<Agent | null>(() => {
    const ag = this.agent();
    return ag && ag.id !== this.project()?.id ? ag : null;
  });

  /** An explicit scope is set and it diverges from the focused agent. */
  readonly outOfSync = computed(() => {
    const f = this.focus();
    const ex = this.explicit();
    if (!f || (!ex.projectId && !ex.agentId)) return false;
    if (!this.projects.all().some((p) => p.id === f.projectId)) return false;
    return this.agent()?.id !== f.id;
  });

  /** How wide the consumer searches within the scope above. Panels that only
   *  ever mean "this worktree" simply ignore it (the default). */
  readonly kind = signal<ScopeKind>("worktree");

  readonly kindOptions = [
    { value: "worktree", label: "This worktree" },
    { value: "project", label: "This project" },
    { value: "all", label: "All worktrees" },
  ];

  /** One-line "where am I" for headers and result counts. The pseudo checkout
   *  reads as "main" rather than repeating the project name twice. */
  readonly label = computed<string>(() => {
    const p = this.project();
    if (!p) return "no project";
    const ag = this.agent();
    if (!ag) return p.name;
    return p.name + " · " + (ag.id === p.id ? "main" : ag.name);
  });

  pickProject(id: string): void {
    this.explicit.set({ projectId: id, agentId: null });
  }
  pickAgent(id: string): void {
    this.explicit.update((s) => ({ projectId: s.projectId ?? this.project()?.id ?? null, agentId: id || null }));
  }
  follow(): void {
    this.explicit.set({ projectId: null, agentId: null });
  }
}

/**
 * The ONE shared scope for the lookup overlays (Ctrl+E recent files,
 * Ctrl+Shift+F find in files, Search Everywhere). Overlays are destroyed on
 * close, so a per-instance scope would reset the user's choice on every
 * reopen — the exact problem this feature fixes.
 */
@Injectable({ providedIn: "root" })
export class LookupScopeStore extends ScopeSelection {}
