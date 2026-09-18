import {
  ChangeDetectionStrategy,
  Component,
  computed,
  ElementRef,
  inject,
  input,
  output,
  signal,
} from "@angular/core";
import { Agent } from "../../models";
import { fileDir, fileName, revealLabelFor } from "../../utils";
import { IconComponent } from "../../shared/icon.component";
import { MenuPanelComponent } from "../../context-menu/menu-panel.component";
import { StateBadgeComponent } from "../../shared/git/state-badge.component";
import { AddDelComponent } from "../../shared/git/add-del.component";
import { BRIDGE, Commands } from "../../data-source/bridge";
import { EditsStore } from "../../stores/edits.store";
import { UiStore } from "../../ui/ui.store";
import { ScrollStateService } from "../scroll-state.service";
import { buildDiffTree, DiffEntry, DiffRow, flattenTree } from "./diff-tree";
import {
  KjButtonComponent,
  KjConfirmPopupActionComponent,
  KjConfirmPopupActionsComponent,
  KjConfirmPopupCancelComponent,
  KjConfirmPopupComponent,
  KjConfirmPopupContentComponent,
  KjConfirmPopupMessageComponent,
  KjDividerComponent,
  KjTabComponent,
  KjTabListComponent,
  KjTabsComponent,
} from "@kouji-ui/components";
import { KjConfirmPopupTrigger } from "@kouji-ui/core";

function msgOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/** Worktree paths travel with forward slashes; the backend joins them itself. */
const norm = (p: string): string => p.replace(/\\/g, "/");

/** What the row context menu acts on: a changed file, a folder row (a real
 *  worktree folder, even though the tree synthesises the path segment), or
 *  null for the empty space below the rows (the worktree root). */
interface MenuTarget {
  path: string;
  name: string;
  isDir: boolean;
  /** The git state of a file row; null for folders (nothing to hand the OS). */
  state: string | null;
}

/**
 * THE changed-file list. One component behind every diff surface — the agent's
 * working-tree changes, a single commit's files, and a multi-commit compare —
 * so all three collapse, resize, sort and read identically. They used to ship
 * two lists: the compare's had no collapsible folders, no folder aggregates
 * and a fixed 232px column, which is exactly how it drifted from the panel it
 * is supposed to mirror.
 *
 * Tree/Flat and the collapsed folders come from UiStore (persisted with the
 * workspace, keyed by agent), so a tab switch or a relaunch never resets them.
 *
 * `allowCreate` is the one real difference between the surfaces: a worktree
 * list can create files and folders (and scopes a create to where the user
 * right-clicked), while a commit's file list is history — nothing to create,
 * so right-clicking anywhere but a file row opens nothing there.
 */
@Component({
  selector: "app-diff-file-list",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    IconComponent,
    MenuPanelComponent,
    StateBadgeComponent,
    AddDelComponent,
    KjButtonComponent,
    KjDividerComponent,
    KjConfirmPopupComponent,
    KjConfirmPopupTrigger,
    KjConfirmPopupContentComponent,
    KjConfirmPopupMessageComponent,
    KjConfirmPopupActionsComponent,
    KjConfirmPopupActionComponent,
    KjConfirmPopupCancelComponent,
    KjTabsComponent,
    KjTabListComponent,
    KjTabComponent,
  ],
  template: `
    <div style="display:flex;flex-direction:column;min-height:0;height:100%;background:var(--panel)">

      <!-- header: label · count, ±total, tree/flat toggle, optional rescan -->
      <div class="pane-head" style="gap:var(--sp-4);padding:var(--sp-3) var(--sp-3) var(--sp-3) var(--sp-5)">
        <!-- the label never wraps: at the column's minimum width a two-word
             title used to break onto a second line and grow the header -->
        <span class="up trunc" style="color:var(--ink-3);flex:none">{{ title() || 'Changed' }} · {{ files().length }}</span>
        <app-add-del [add]="totalAdd()" [del]="totalDel()" />
        <kj-tabs variant="pills" class="tabs-xs" style="margin-left:auto"
                 [value]="treeMode() ? 'tree' : 'flat'" (valueChange)="setTreeMode($event === 'tree')">
          <kj-tab-list aria-label="File list layout">
            <kj-tab value="tree" title="Tree view">
              <app-icon size="md" name="graph" [color]="treeMode() ? 'var(--ui-ink)' : null" />Tree
            </kj-tab>
            <kj-tab value="flat" title="Flat view">
              <app-icon size="md" name="dots" [color]="!treeMode() ? 'var(--ui-ink)' : null" />Flat
            </kj-tab>
          </kj-tab-list>
        </kj-tabs>
        @if (canRefresh()) {
          <kj-button kjSize="icon" kjVariant="ghost" (click)="refresh.emit()" title="Rescan changes"><app-icon size="md" name="refresh" /></kj-button>
        }
      </div>

      <!-- The listing is also the root-scoped menu surface (worktree lists
           only): row handlers stop propagation, so a right-click on the empty
           space below them creates at the worktree root. -->
      <div class="scroll-y" style="flex:1;padding:var(--sp-2) 0" (contextmenu)="onRootContext($event)">
        @if (!files().length) {
          <div style="padding:var(--sp-5) var(--sp-6);color:var(--ink-4)">{{ emptyLabel() }}</div>
        } @else if (treeMode()) {
          @for (row of rows(); track row.path) {
            @if (row.dir) {
              <div
                class="diff-dir list-row"
                [class.gone]="row.state === 'D'"
                (click)="toggleDir(row.path)"
                (contextmenu)="onDirContext($event, row)"
                [style.padding-left.px]="8 + row.depth * 13"
              >
                <app-icon [name]="isDirOpen(row.path) ? 'chevronD' : 'chevron'" size="sm" color="var(--ink-4)" />
                <app-icon [name]="isDirOpen(row.path) ? 'folderOpen' : 'folder'" size="sm" color="var(--ink-4)" />
                <span class="dname">{{ row.name }}</span>
                @if (row.state) {
                  <app-state-badge [state]="row.state" />
                }
                <app-add-del class="counts-chip" [add]="row.add ?? 0" [del]="row.del ?? 0" />
              </div>
            } @else {
              <div
                class="diff-file list-row"
                [class.sel]="selPath() === row.path"
                (click)="select.emit(row.path)"
                (contextmenu)="onFileContext($event, row.file!)"
                [style.padding-left.px]="12 + row.depth * 13"
              >
                <app-state-badge [state]="row.file!.state" />
                <span [title]="rowTitle(row.file!)" class="fname trunc">{{ row.name }}</span>
                <app-add-del class="counts-chip" [add]="row.file!.add" [del]="row.file!.del" />
              </div>
            }
          }
        } @else {
          <!-- flat view: the SAME single-line row as the tree — the directory
               (or rename origin) rides inline, muted, so both modes share
               --row-h -->
          @for (f of files(); track f.path) {
            <div
              class="diff-file list-row"
              [class.sel]="selPath() === f.path"
              (click)="select.emit(f.path)"
              (contextmenu)="onFileContext($event, f)"
            >
              <app-state-badge [state]="f.state" />
              <span [title]="rowTitle(f)" class="fname trunc">{{ fname(f.path) }}</span>
              @if (f.state === 'R' && f.oldPath) {
                <span class="fdir trunc" style="color:var(--vcs-renamed)">← {{ f.oldPath }}</span>
              } @else if (fdir(f.path)) {
                <span class="fdir trunc">{{ fdir(f.path) }}</span>
              }
              <app-add-del class="counts-chip" [add]="f.add" [del]="f.del" />
            </div>
          }
        }
      </div>
    </div>

    <!-- context menu: file/folder CRUD + the OS hand-offs. A deleted file has
         nothing left on disk to open or show, so both hand-offs disable. -->
    @if (menu(); as m) {
      <app-menu-panel [x]="m.x" [y]="m.y" (closed)="closeMenu()">
        @switch (menuMode()) {
          @case ("actions") {
            @if (allowCreate()) {
              <kj-button kjVariant="ghost" [kjFullWidth]="true" class="menu-item" (click)="startInput('create-file')"><app-icon size="md" name="file" />New File…</kj-button>
              <kj-button kjVariant="ghost" [kjFullWidth]="true" class="menu-item" (click)="startInput('create-dir')"><app-icon size="md" name="folder" />New Folder…</kj-button>
            }
            @if (m.target; as t) {
              <kj-button kjVariant="ghost" [kjFullWidth]="true" class="menu-item" (click)="startRename()"><app-icon size="md" name="rename" />Rename…</kj-button>
              <kj-divider />
              <kj-button kjVariant="ghost" [kjFullWidth]="true" class="menu-item" [kjDisabled]="t.state === 'D'" (click)="openExternal(t)"><app-icon size="md" name="ext" />Open in Default App</kj-button>
              <kj-button kjVariant="ghost" [kjFullWidth]="true" class="menu-item" [kjDisabled]="t.state === 'D'" (click)="reveal(t)"><app-icon size="md" name="folderOpen" />{{ revealLabel }}</kj-button>
              <kj-divider />
              <kj-confirm-popup [kjDestructive]="true" (kjConfirmed)="confirmDelete()">
                <kj-button kjConfirmPopupTrigger #delTrig="kjConfirmPopupTrigger" kjVariant="danger" [kjFullWidth]="true" class="menu-item"><app-icon size="md" name="trash" />Delete</kj-button>
                <kj-confirm-popup-content [kjFor]="delTrig">
                  <kj-confirm-popup-message>Delete <b>{{ t.name }}</b>{{ t.isDir ? ' and its contents' : '' }}?</kj-confirm-popup-message>
                  <kj-confirm-popup-actions>
                    <kj-confirm-popup-cancel><kj-button kjVariant="outline">Cancel</kj-button></kj-confirm-popup-cancel>
                    <kj-confirm-popup-action><kj-button kjVariant="danger">Delete</kj-button></kj-confirm-popup-action>
                  </kj-confirm-popup-actions>
                </kj-confirm-popup-content>
              </kj-confirm-popup>
            }
          }
          @default {
            <div class="menu-label">{{ inputLabel() }}</div>
            <input
              class="menu-input"
              [value]="nameInput()"
              (input)="nameInput.set($any($event.target).value)"
              (keydown.enter)="commit()"
              (keydown.escape)="closeMenu()"
              spellcheck="false"
            />
            <div class="menu-row">
              <kj-button kjVariant="outline" (click)="closeMenu()">Cancel</kj-button>
              <kj-button kjVariant="default" [kjDisabled]="!nameInput().trim()" (click)="commit()">OK</kj-button>
            </div>
          }
        }
      </app-menu-panel>
    }
  `,
})
export class DiffFileListComponent {
  private readonly bridge = inject(BRIDGE);
  private readonly ui = inject(UiStore);
  private readonly edits = inject(EditsStore);
  private readonly scroll = inject(ScrollStateService);
  private readonly host = inject(ElementRef<HTMLElement>);

  readonly agent = input.required<Agent>();
  readonly files = input<readonly DiffEntry[]>([]);
  readonly selPath = input<string | null | undefined>(null);
  readonly title = input<string>("");
  /** Worktree lists can create; a commit's file list is history. Also gates
   *  the folder-row and empty-space menus — there is nothing else to do to a
   *  folder in a read-only listing. */
  readonly allowCreate = input(false);
  /** Shows the rescan button; the host owns the reload. */
  readonly canRefresh = input(false);
  readonly emptyLabel = input("no changes");

  readonly select = output<string>();
  readonly refresh = output<void>();
  /** A worktree write landed (create / rename / delete) — the host re-scans
   *  whatever feeds it. */
  readonly mutated = output<void>();

  // Tree/Flat and the collapsed folders are workspace preferences, not view
  // state: this component is destroyed on every tab switch.
  readonly treeMode = computed(() => this.ui.diffTreeMode());
  setTreeMode(on: boolean): void {
    this.ui.diffTreeMode.set(on);
  }

  private readonly agentId = computed(() => this.agent().id);

  readonly tree = computed(() => buildDiffTree(this.files()));
  private readonly dirOpen = computed<Record<string, boolean>>(() =>
    this.ui.diffDirOpenFor(this.agentId()),
  );
  /** The visible rows — collapsed folders keep their children off screen. */
  readonly rows = computed<DiffRow[]>(() =>
    flattenTree(this.tree(), (path) => this.dirOpen()[path] !== false),
  );
  isDirOpen(path: string): boolean {
    return this.dirOpen()[path] !== false;
  }
  toggleDir(path: string): void {
    this.ui.toggleDiffDir(this.agentId(), path);
  }

  readonly totalAdd = computed(() => this.files().reduce((s, f) => s + f.add, 0));
  readonly totalDel = computed(() => this.files().reduce((s, f) => s + f.del, 0));

  // ----- row context menu: file/folder CRUD + the OS hand-offs -----
  readonly menu = signal<{ x: number; y: number; target: MenuTarget | null } | null>(null);
  readonly menuMode = signal<"actions" | "create-file" | "create-dir" | "rename">("actions");
  readonly nameInput = signal("");
  readonly revealLabel = revealLabelFor(navigator.userAgent);

  readonly inputLabel = computed(() => {
    const t = this.menu()?.target ?? null;
    switch (this.menuMode()) {
      case "create-file":
        return `New file in ${this.scopeDir(t) || "worktree root"}`;
      case "create-dir":
        return `New folder in ${this.scopeDir(t) || "worktree root"}`;
      case "rename":
        return `Rename ${t?.name ?? ""}`;
      default:
        return "";
    }
  });

  private open(e: MouseEvent, target: MenuTarget | null): void {
    e.preventDefault();
    e.stopPropagation();
    this.menu.set({ x: e.clientX, y: e.clientY, target });
    this.menuMode.set("actions");
    this.nameInput.set("");
  }
  /** File rows: the file's state drives the OS hand-offs. */
  onFileContext(e: MouseEvent, file: DiffEntry): void {
    this.open(e, { path: file.path, name: fileName(file.path), isDir: false, state: file.state });
  }
  /** Folder rows carry no file — build the target from the tree row. Only a
   *  worktree list acts on folders. */
  onDirContext(e: MouseEvent, row: DiffRow): void {
    if (!this.allowCreate()) return;
    this.open(e, { path: row.path, name: row.name, isDir: true, state: null });
  }
  /** Empty space below the rows = the worktree root: creates only. */
  onRootContext(e: MouseEvent): void {
    if (!this.allowCreate()) return;
    this.open(e, null);
  }
  closeMenu(): void {
    this.menu.set(null);
  }

  startInput(mode: "create-file" | "create-dir"): void {
    this.menuMode.set(mode);
    this.nameInput.set("");
    this.focusInput();
  }
  startRename(): void {
    this.menuMode.set("rename");
    this.nameInput.set(this.menu()?.target?.name ?? "");
    this.focusInput();
  }
  private focusInput(): void {
    queueMicrotask(() => {
      const el = this.host.nativeElement.querySelector(".menu-input") as HTMLInputElement | null;
      el?.focus();
      el?.select();
    });
  }

  /** Directory a create scopes to: the row itself (folder), its parent (file),
   *  or "" for the worktree root. */
  private scopeDir(t: MenuTarget | null): string {
    if (!t) return "";
    const p = norm(t.path);
    if (t.isDir) return p;
    const i = p.lastIndexOf("/");
    return i === -1 ? "" : p.slice(0, i);
  }

  async commit(): Promise<void> {
    const m = this.menu();
    const name = this.nameInput().trim();
    if (!m || !name) return;
    const mode = this.menuMode();
    const id = this.agentId();
    try {
      if (mode === "rename") {
        const from = norm(m.target!.path);
        // the parent of the row, folder and file alike
        const dir = this.scopeDir({ ...m.target!, isDir: false });
        const to = dir ? `${dir}/${name}` : name;
        await this.bridge.invoke(Commands.FileRename, { id, from, to });
        this.edits.close(id, from); // stale buffer under the old path
        this.scroll.clear(id, from);
      } else {
        const base = this.scopeDir(m.target);
        const path = base ? `${base}/${name}` : name;
        const cmd = mode === "create-dir" ? Commands.DirCreate : Commands.FileCreate;
        await this.bridge.invoke(cmd, { id, path });
        if (mode === "create-file") this.ui.openFileInWorkspace(id, path);
      }
      this.closeMenu();
      this.mutated.emit();
    } catch (e) {
      this.ui.flash(msgOf(e));
    }
  }

  openExternal(t: MenuTarget): void {
    this.toOs(Commands.FileOpenExternal, t.path, "couldn't open");
  }
  reveal(t: MenuTarget): void {
    this.toOs(Commands.FileReveal, t.path, "couldn't reveal");
  }
  /** Dismiss first, then hand the worktree-relative path to the OS; a failure
   *  reports through the flash rather than by leaving the menu hanging open. */
  private toOs(command: string, raw: string, failed: string): void {
    const path = norm(raw);
    this.closeMenu();
    void this.bridge
      .invoke(command, { id: this.agentId(), path })
      .catch((e: unknown) => this.ui.flash(`${failed} ${fileName(path)}: ${msgOf(e)}`));
  }

  async confirmDelete(): Promise<void> {
    const t = this.menu()?.target;
    if (!t) return;
    const id = this.agentId();
    const path = norm(t.path);
    // dismiss first, like every other row action: the confirmation has already
    // been given, and a failure reports through the flash rather than by
    // leaving the menu hanging open.
    this.closeMenu();
    try {
      await this.bridge.invoke(Commands.FileDelete, { id, path });
      this.edits.close(id, path);
      this.scroll.clear(id, path);
      this.mutated.emit();
    } catch (e) {
      this.ui.flash(msgOf(e));
    }
  }

  /** A renamed row says where it came from; every other row says its path. */
  rowTitle(f: DiffEntry): string {
    return f.state === "R" && f.oldPath ? `renamed from ${f.oldPath}` : f.path;
  }

  readonly fname = fileName;
  readonly fdir = fileDir;
}
