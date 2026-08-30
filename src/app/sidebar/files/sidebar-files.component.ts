import {
  ChangeDetectionStrategy,
  Component,
  computed,
  effect,
  inject,
  signal,
} from "@angular/core";
import { Agent, Project } from "../../models";
import { AgentRuntimeService } from "../../agents/agent-runtime.service";
import { AgentWorkStore, projectRootKey } from "../../agents/agent-work.store";
import { CommandRegistryService } from "../../commands/command-registry.service";
import { ProjectActionsService } from "../../projects/project-actions.service";
import { UiStore } from "../../ui/ui.store";
import { IconComponent } from "../../shared/icon.component";
import { StatusDotComponent } from "../../shared/status-dot.component";
import { SidebarFileTreeComponent } from "./file-tree.component";

/** Default section height (px) when the user has never resized it. */
export const FILES_SECTION_DEFAULT_H = 288;
const MIN_H = 120;
/** Keep at least this much room for the agents section above. */
const AGENTS_MIN_ROOM = 320;

/**
 * v2 sidebar files section — the repo tree, moved from the deleted right
 * panel. The ROOT CHIP is the critical element: it names the worktree the tree
 * is rooted at (`main` or any agent worktree) and is always visible — the
 * scope follows the active tab but is never implicit. Collapses to its header
 * strip; height is drag-resizable and persisted (WorkspaceDoc).
 */
@Component({
  selector: "app-sidebar-files",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, StatusDotComponent, SidebarFileTreeComponent],
  template: `
    @let open = !ui.sidebarFilesCollapsed();
    @let root = rootKey();
    <div
      style="flex:none;display:flex;flex-direction:column;min-height:0;position:relative;border-top:1px solid var(--hair)"
      [style.height]="open ? height() + 'px' : 'auto'"
    >
      <!-- drag handle: resizes the agents/files split -->
      @if (open) {
        <div
          (mousedown)="startDrag($event)"
          title="drag to resize"
          style="position:absolute;left:0;right:0;top:-3px;height:var(--sp-2);cursor:row-resize;z-index:5"
        ></div>
      }

      <!-- header — the root chip keeps the scope visible, never implicit -->
      <div style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-3) var(--sp-4) var(--sp-3) var(--sp-6);flex:none;position:relative">
        <span class="up" style="font-size:var(--fs-3xs);color:var(--ink-4);margin-right:var(--sp-1)">Files</span>
        <button
          (click)="pick.set(!pick())"
          title="Worktree root — the tree is rooted here"
          [style.border-color]="overridden() ? 'var(--ui-line)' : 'var(--hair)'"
          style="display:inline-flex;align-items:center;gap:var(--sp-3);min-width:0;flex:0 1 auto;height:var(--ctl-h-sm);padding:0 var(--sp-4);background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-sm);cursor:pointer;color:var(--ink);font-family:var(--font-mono);font-size:var(--fs-2xs)"
        >
          @if (rootAgent(); as ra) {
            <app-status-dot [status]="ra.status" />
          } @else if (rootProject(); as rp) {
            <app-icon [name]="rp.icon" size="sm" [color]="rp.color" />
          } @else {
            <app-icon name="folder" size="sm" color="var(--ink-3)" />
          }
          <span style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ rootLabel() }}</span>
          <app-icon name="chevronD" size="sm" color="var(--ink-4)" />
        </button>
        @if (treeLoading()) {
          <span class="tnum" style="font-size:var(--fs-3xs);color:var(--ink-4);flex:none">scanning…</span>
        }
        <button class="pane-btn" (click)="openFuzzy()" [title]="'Fuzzy finder — scoped to ' + rootLabel()" style="margin-left:auto">
          <app-icon name="search" size="sm" />
        </button>
        <button class="pane-btn" (click)="refresh()" [disabled]="treeLoading()" title="Rescan worktree">
          <app-icon name="refresh" size="sm" />
        </button>
        <button class="pane-btn" (click)="toggleOpen()" [title]="open ? 'Collapse files' : 'Expand files'">
          <app-icon [name]="open ? 'chevronD' : 'chevron'" size="sm" />
        </button>

        <!-- root dropdown — main or any agent worktree, grouped by project -->
        @if (pick()) {
          <div (click)="pick.set(false)" style="position:fixed;inset:0;z-index:39"></div>
          <div
            class="rise scroll-y"
            style="position:absolute;left:var(--sp-5);right:var(--sp-5);top:calc(100% - 2px);z-index:40;background:var(--elev);border:1px solid var(--hair-2);border-radius:var(--r-md);box-shadow:var(--shadow);padding:var(--sp-2);max-height:round(calc(300px * var(--density)), 1px);overflow-y:auto"
          >
            @for (p of projects.all(); track p.id) {
              <div class="up" style="font-size:var(--fs-3xs);color:var(--ink-4);padding:var(--sp-3) var(--sp-4) var(--sp-1)">{{ p.name }}</div>
              <button
                class="menu-item"
                [style.background]="root === projKey(p.id) ? 'var(--ui-sel)' : null"
                (click)="pickRoot(projKey(p.id))"
              >
                <app-icon [name]="p.icon" size="sm" [color]="p.color" />
                <span>main</span>
                <span style="margin-left:auto;font-size:var(--fs-3xs);color:var(--ink-4)">repo root</span>
              </button>
              @for (a of agentsOf(p.id); track a.id) {
                <button
                  class="menu-item"
                  [style.background]="root === a.id ? 'var(--ui-sel)' : null"
                  (click)="pickRoot(a.id)"
                >
                  <app-status-dot [status]="a.status" />
                  <span style="overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ wtName(a.worktree) }}</span>
                  <span style="margin-left:auto;font-size:var(--fs-3xs);color:var(--ink-4);overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ a.branch }}</span>
                </button>
              }
            }
          </div>
        }
      </div>

      @if (open && root) {
        <app-sidebar-file-tree [rootKey]="root" />
      }
    </div>
  `,
})
export class SidebarFilesComponent {
  readonly ui = inject(UiStore);
  readonly projects = inject(ProjectActionsService);
  private readonly runtime = inject(AgentRuntimeService);
  private readonly work = inject(AgentWorkStore);
  private readonly registry = inject(CommandRegistryService);

  readonly pick = signal(false);
  readonly projKey = projectRootKey;

  /** What the section follows when the user hasn't overridden: the active
   *  tab's scoped agent, else the first project's main worktree. */
  private readonly followKey = computed<string | null>(() => {
    const ag = this.runtime.activeAgent();
    if (ag) return ag.id;
    const first = this.projects.all()[0];
    return first ? projectRootKey(first.id) : null;
  });

  /** The effective root: the active tab's explicit pick, else follow. A stale
   *  override (agent removed) falls back to follow. */
  readonly rootKey = computed<string | null>(() => {
    const override = this.ui.filesRootOverride()[this.ui.activeTab()];
    if (override && this.rootExists(override)) return override;
    return this.followKey();
  });
  readonly overridden = computed(
    () => this.rootKey() !== null && this.rootKey() !== this.followKey(),
  );

  readonly rootAgent = computed<Agent | null>(() => {
    const key = this.rootKey();
    if (!key || key.startsWith("proj:")) return null;
    return this.runtime.agents().find((a) => a.id === key) ?? null;
  });
  readonly rootProject = computed<Project | undefined>(() => {
    const key = this.rootKey();
    if (!key?.startsWith("proj:")) return undefined;
    return this.projects.all().find((p) => p.id === key.slice(5));
  });
  readonly rootLabel = computed(() => {
    const ag = this.rootAgent();
    if (ag) return this.wtName(ag.worktree);
    return this.rootProject() ? "main" : "—";
  });

  // "loading" only counts before the FIRST scan lands: watcher re-scans keep
  // the previous rows on screen, so surfacing them as loading just flashes
  // "scanning…" on every worktree event (the flicker storm while an agent runs).
  readonly treeLoading = computed(() => {
    const key = this.rootKey();
    if (!key) return false;
    const t = this.work.treeFor(key);
    return t.status === "loading" && !t.data.length;
  });

  readonly height = computed(() => this.ui.sidebarFilesH() ?? FILES_SECTION_DEFAULT_H);

  constructor() {
    // Lazy: load the tree the first time a root becomes current (agent roots
    // are usually warm already — AgentRuntimeService ensures the active one).
    effect(() => {
      const key = this.rootKey();
      if (key && !this.ui.sidebarFilesCollapsed()) this.work.ensureTree(key);
    });
  }

  private rootExists(key: string): boolean {
    if (key.startsWith("proj:")) return this.projects.all().some((p) => p.id === key.slice(5));
    return this.runtime.agents().some((a) => a.id === key);
  }

  agentsOf(projectId: string): Agent[] {
    return this.runtime.agents().filter((a) => a.projectId === projectId);
  }

  pickRoot(key: string): void {
    this.pick.set(false);
    // picking what follow already shows clears the override (chip un-highlights)
    this.ui.setFilesRootOverride(this.ui.activeTab(), key === this.followKey() ? null : key);
  }

  toggleOpen(): void {
    this.ui.sidebarFilesCollapsed.update((v) => !v);
  }

  openFuzzy(): void {
    this.registry.open("search", "files");
  }

  refresh(): void {
    const key = this.rootKey();
    if (key) this.work.loadTree(key);
  }

  wtName(path: string): string {
    return path.replace(/\\/g, "/").split("/").pop() || path;
  }

  // ---- drag-resize (the agents/files split) ----
  private drag: { y: number; h: number } | null = null;
  private readonly onMove = (e: MouseEvent) => {
    if (!this.drag) return;
    const max = Math.max(MIN_H, window.innerHeight - AGENTS_MIN_ROOM);
    this.ui.sidebarFilesH.set(
      Math.max(MIN_H, Math.min(max, this.drag.h + (this.drag.y - e.clientY))),
    );
  };
  private readonly onUp = () => {
    this.drag = null;
    document.removeEventListener("mousemove", this.onMove);
    document.removeEventListener("mouseup", this.onUp);
  };
  startDrag(e: MouseEvent): void {
    e.preventDefault();
    this.drag = { y: e.clientY, h: this.height() };
    document.addEventListener("mousemove", this.onMove);
    document.addEventListener("mouseup", this.onUp);
  }
}
