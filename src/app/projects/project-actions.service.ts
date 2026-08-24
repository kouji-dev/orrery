import { inject, Injectable, signal } from "@angular/core";
import { Commit, MenuItem, Project } from "../models";
import { ProjectsStore } from "../stores/projects.store";
import { UiStore } from "../ui/ui.store";

export interface AddProjectRequest {
  path: string;
  name: string;
  icon: string;
  color: string;
  gitInit: boolean;
  /** clone source — forwarded as-is; the backend decides what happens */
  sourceUrl?: string;
  /** "root" → clone into path/<repo-name>"project" → path IS the project ("." clone) */
  sourceMode?: "root" | "project";
  /** shallow-clone depth (only the default branch); omit for full history */
  depth?: number;
}

/**
 * Project-level actions (create / remove / relocate / init-git) and the merged
 * commit feed. Pure project domain — depends only on the projects data store
 * and the UI store (for toasts).
 */
@Injectable({ providedIn: "root" })
export class ProjectActionsService {
  private projectsStore = inject(ProjectsStore);
  private ui = inject(UiStore);

  readonly all = this.projectsStore.all;
  readonly commits = signal<Commit[]>([]);

  projectOf(id: string | undefined): Project | undefined {
    return this.all().find((p) => p.id === id);
  }

  /** Pull real git log for every project and merge into one feed (newest first). */
  async refreshCommits(projectIds: string[]) {
    const lists = await Promise.all(
      projectIds.map((id) => this.projectsStore.commits(id).catch(() => [] as Commit[])),
    );
    this.commits.set(lists.flat().sort((a, b) => (b.ts ?? Infinity) - (a.ts ?? Infinity)));
  }

  addProject(req: AddProjectRequest) {
    if (req.sourceUrl) this.ui.flash("cloning " + req.sourceUrl + "…");
    void this.projectsStore
      .create({
        name: req.name,
        path: req.path,
        icon: req.icon,
        color: req.color,
        withGit: req.gitInit,
        sourceUrl: req.sourceUrl,
        sourceMode: req.sourceMode,
        depth: req.depth,
      })
      .then((p) => this.ui.flash("added project " + p.name))
      .catch((e: { kind?: string; message?: string }) =>
        this.ui.flash(
          e?.kind === "project" || e?.kind === "notFound" ? (e.message ?? "failed") : "add failed",
        ),
      );
    this.ui.closeAddProject();
  }

  removeProject(id: string) {
    const p = this.projectOf(id);
    // backend cascade-deletes the project's agents and emits agent://deleted for each
    void this.projectsStore
      .remove(id)
      .then(() => this.ui.flash("removed project " + (p ? p.name : id)))
      .catch(() => {});
  }

  // Re-point a project at a new folder (e.g. after it was moved).
  relocateProject(id: string) {
    void this.projectsStore.pickDirectory().then((path) => {
      if (!path) return;
      void this.projectsStore
        .update(id, { path })
        .then((u) => this.ui.flash("relocated " + u.name + " → " + u.path))
        .catch((e: { message?: string }) => this.ui.flash(e?.message ?? "relocate failed"));
    });
  }

  initGitForProject(id: string) {
    void this.projectsStore
      .initGit(id)
      .then((u) => this.ui.flash("git initialized · " + u.name))
      .catch((e: { message?: string }) => this.ui.flash(e?.message ?? "git init failed"));
  }

  projectMenu(id: string): MenuItem[] {
    const p = this.all().find((x) => x.id === id);
    if (!p) return [];
    const items: MenuItem[] = [
      {
        label: "Spawn agent here",
        icon: "plus",
        accent: "var(--ui-ink)",
        onClick: () => this.ui.spawning.set({ project: id }),
      },
      { sep: true },
      {
        label: "Pull latest",
        icon: "refresh",
        disabled: !p.folderExists,
        onClick: () => this.ui.flash("pulled " + p.name),
      },
    ];
    if (p.folderExists && !p.hasGit) {
      items.push({ label: "Initialize git", icon: "git", onClick: () => this.initGitForProject(id) });
    }
    items.push(
      {
        label: "Open in terminal",
        icon: "terminal",
        disabled: !p.folderExists,
        onClick: () => this.ui.flash("opened terminal · " + p.path),
      },
      { label: "Copy path", icon: "dup", onClick: () => this.ui.flash(p.path) },
      p.folderExists
        ? { label: "Relocate folder…", icon: "folderOpen", onClick: () => this.relocateProject(id) }
        : {
            label: "Folder missing — relocate…",
            icon: "folderOpen",
            accent: "var(--st-blocked)",
            onClick: () => this.relocateProject(id),
          },
      { sep: true },
      { label: "Remove project", icon: "trash", danger: true, onClick: () => this.removeProject(id) },
    );
    return items;
  }
}
