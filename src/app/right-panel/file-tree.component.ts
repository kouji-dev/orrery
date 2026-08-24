import { ScrollingModule } from "@angular/cdk/scrolling";
import {
  ChangeDetectionStrategy,
  Component,
  computed,
  ElementRef,
  inject,
  input,
  signal,
} from "@angular/core";
import { Agent, FileNode, Project } from "../models";
import { AgentWorkStore } from "../agents/agent-work.store";
import { BRIDGE, Commands } from "../data-source/bridge";
import { DragService } from "../shared/drag.service";
import { EditsStore } from "../stores/edits.store";
import { ScrollStateService } from "../workspace/scroll-state.service";
import { tokenPx } from "../ui/density";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { MenuPanelComponent } from "../context-menu/menu-panel.component";
import { revealLabelFor } from "../utils";
import {
  KjBadgeComponent,
  KjButtonComponent,
  KjConfirmPopupActionComponent,
  KjConfirmPopupActionsComponent,
  KjConfirmPopupCancelComponent,
  KjConfirmPopupComponent,
  KjConfirmPopupContentComponent,
  KjConfirmPopupMessageComponent,
  KjConfirmPopupTriggerComponent,
  KjDividerComponent,
  KjEmptyStateComponent,
  KjEmptyStateDescriptionComponent,
  KjEmptyStateIconComponent,
  KjSkeletonComponent,
} from "@kouji-ui/components";

interface FlatRow {
  node: FileNode;
  depth: number;
}

function msgOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

@Component({
  selector: "app-file-tree",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    IconComponent,
    MenuPanelComponent,
    ScrollingModule,
    KjButtonComponent,
    KjDividerComponent,
    KjBadgeComponent,
    KjConfirmPopupComponent,
    KjConfirmPopupTriggerComponent,
    KjConfirmPopupContentComponent,
    KjConfirmPopupMessageComponent,
    KjConfirmPopupActionsComponent,
    KjConfirmPopupActionComponent,
    KjConfirmPopupCancelComponent,
    KjEmptyStateComponent,
    KjEmptyStateIconComponent,
    KjEmptyStateDescriptionComponent,
    KjSkeletonComponent,
  ],
  template: `
    @let ag = agent();
    <!-- root-scoped context menu anywhere in the panel — header, empty space
         below the rows, or the empty/loading states. Row handlers stop
         propagation, so node-scoped menus still win on the rows themselves. -->
    <div (contextmenu)="onContext($event, null)" style="display:flex;flex-direction:column;min-height:0;flex:1">
      <div class="pane-head tight">
        <app-icon name="folder" size="sm" [color]="project() ? project()!.color : 'var(--ui-ink)'" />
        <span class="trunc" style="flex:1;color:var(--ink-2)" [title]="ag.worktree">{{ wtName(ag.worktree) }}</span>
        @if (loading()) { <span class="tnum" style="font-size:var(--fs-meta);color:var(--ink-4);flex:none">scanning…</span> }
        <kj-button kjSize="icon" kjVariant="ghost" (click)="refresh()" [kjDisabled]="loading()" title="Rescan worktree"><app-icon size="sm" name="refresh" /></kj-button>
      </div>

      @if (rows().length) {
        <!-- virtualized: only visible rows are rendered. Data wins over the
             loading flag so background watcher scans never unmount the
             viewport (which would drop its scroll offset and flicker). -->
        <cdk-virtual-scroll-viewport [itemSize]="rowH()" minBufferPx="240" maxBufferPx="480" style="flex:1" class="scroll-y">
          <div
            *cdkVirtualFor="let row of rows(); trackBy: trackPath"
            (click)="onRow(row.node)"
            (contextmenu)="onContext($event, row.node)"
            [attr.draggable]="row.node.isDir ? null : 'true'"
            (dragstart)="onDragStart($event, row.node)"
            (dragend)="dragSvc.end()"
            [style.padding-left.px]="indentFor(row)"
            style="height:var(--sp-9);display:flex;align-items:center;gap:var(--sp-3);cursor:pointer;padding-right:var(--sp-4);border-radius:5px"
          >
            @if (row.node.isDir) {
              <app-icon size="md" [name]="isOpen(row.node) ? 'chevronD' : 'chevron'" color="var(--ink-4)" />
              <app-icon size="lg" [name]="isOpen(row.node) ? 'folderOpen' : 'folder'" [color]="row.node.ignored ? 'var(--ink-4)' : 'var(--ui-ink)'" />
            } @else {
              <app-icon size="md" name="file" [color]="stateOf(row.node.path) ? stateInk(stateOf(row.node.path)!) : 'var(--ink-4)'" />
            }
            <span
              [style.color]="row.node.ignored ? 'var(--ink-4)' : row.node.isDir ? 'var(--ink-2)' : 'var(--ink-3)'"
              [style.opacity]="row.node.ignored ? 0.7 : 1"
              class="trunc"
            >{{ row.node.name }}</span>
            @if (row.node.ignored) {
              <kj-badge style="margin-left:auto;font-size:var(--fs-badge);padding:0 var(--sp-2);color:var(--ink-4)">ignored</kj-badge>
            } @else if (!row.node.isDir && stateOf(row.node.path); as st) {
              <span class="tnum" [style.color]="stateInk(st)" style="margin-left:auto;flex:none;font-size:var(--fs-meta);font-weight:var(--fw-strong);padding-left:var(--sp-3)">{{ st }}</span>
            }
          </div>
        </cdk-virtual-scroll-viewport>
      } @else if (loading()) {
        <div aria-busy="true" style="padding:var(--sp-4) var(--sp-6)">
          <kj-skeleton kjSkeletonShape="text-block" [kjLines]="6" />
        </div>
      } @else {
        <kj-empty-state kjSize="sm">
          <kj-empty-state-icon><app-icon name="folder" size="lg" color="var(--hair-2)" /></kj-empty-state-icon>
          <kj-empty-state-description>empty worktree</kj-empty-state-description>
        </kj-empty-state>
      }
    </div>

    <!-- context menu: file CRUD (B1.1) — shared menu chrome -->
    @if (menu(); as m) {
      <app-menu-panel [x]="m.x" [y]="m.y" (closed)="closeMenu()">
        @switch (menuMode()) {
          @case ("actions") {
            <kj-button kjVariant="ghost" class="menu-item" [kjFullWidth]="true" (click)="startInput('create-file')"><app-icon size="md" name="file" />New File…</kj-button>
            <kj-button kjVariant="ghost" class="menu-item" [kjFullWidth]="true" (click)="startInput('create-dir')"><app-icon size="md" name="folder" />New Folder…</kj-button>
            @if (m.node) {
              <kj-button kjVariant="ghost" class="menu-item" [kjFullWidth]="true" (click)="startRename()"><app-icon size="md" name="rename" />Rename…</kj-button>
              <kj-divider />
              <kj-button kjVariant="ghost" class="menu-item" [kjFullWidth]="true" (click)="openExternal(m.node)"><app-icon size="md" name="ext" />Open in Default App</kj-button>
              <kj-button kjVariant="ghost" class="menu-item" [kjFullWidth]="true" (click)="reveal(m.node)"><app-icon size="md" name="folderOpen" />{{ revealLabel }}</kj-button>
              <kj-divider />
              <kj-confirm-popup [kjDestructive]="true" (kjConfirmed)="confirmDelete()">
                <kj-confirm-popup-trigger #delTrig="kjConfirmPopupTrigger">
                  <kj-button kjVariant="danger" class="menu-item" [kjFullWidth]="true"><app-icon size="md" name="trash" />Delete</kj-button>
                </kj-confirm-popup-trigger>
                <kj-confirm-popup-content [kjFor]="delTrig">
                  <kj-confirm-popup-message>Delete <b>{{ m.node.name }}</b>{{ m.node.isDir ? ' and its contents' : '' }}?</kj-confirm-popup-message>
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
export class FileTreeComponent {
  private work = inject(AgentWorkStore);
  private ui = inject(UiStore);

  /**
   * Row height, read from the same `--sp-9` token the row CSS uses. The
   * viewport previously hardcoded `itemSize="24"`, so at any density other
   * than regular the virtual scroller's arithmetic drifted from the real row
   * height and rows misaligned as you scrolled. Recomputed per density switch.
   */
  readonly rowH = computed(() => {
    void this.ui.tweaks().density;
    return tokenPx("--sp-9", 24);
  });

  /** Per-depth indent. Tracks the chevron+icon width, hence a spacing token. */
  readonly indentStep = computed(() => {
    void this.ui.tweaks().density;
    return tokenPx("--sp-6", 12);
  });

  /** Row indent; leaf rows take one extra step to clear the missing twisty. */
  indentFor(row: FlatRow): number {
    const step = this.indentStep();
    return tokenPx("--sp-4", 8) + row.depth * step + (row.node.isDir ? 0 : step);
  }

  private bridge = inject(BRIDGE);
  private edits = inject(EditsStore);
  private scroll = inject(ScrollStateService);
  private host = inject(ElementRef<HTMLElement>);
  readonly dragSvc = inject(DragService);
  readonly agent = input.required<Agent>();
  readonly project = input<Project | undefined>(undefined);

  // ----- context-menu file CRUD (B1.1) -----
  readonly menu = signal<{ x: number; y: number; node: FileNode | null } | null>(null);
  readonly menuMode = signal<"actions" | "create-file" | "create-dir" | "rename">("actions");
  readonly nameInput = signal("");

  readonly inputLabel = computed(() => {
    const m = this.menu();
    switch (this.menuMode()) {
      case "create-file":
        return `New file in ${this.scopeDir(m?.node ?? null) || "worktree root"}`;
      case "create-dir":
        return `New folder in ${this.scopeDir(m?.node ?? null) || "worktree root"}`;
      case "rename":
        return `Rename ${m?.node?.name ?? ""}`;
      default:
        return "";
    }
  });

  onContext(e: MouseEvent, node: FileNode | null): void {
    e.preventDefault();
    e.stopPropagation();
    this.menu.set({ x: e.clientX, y: e.clientY, node });
    this.menuMode.set("actions");
    this.nameInput.set("");
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
    this.nameInput.set(this.menu()?.node?.name ?? "");
    this.focusInput();
  }

  private focusInput(): void {
    queueMicrotask(() => {
      const el = this.host.nativeElement.querySelector(".ft-input") as HTMLInputElement | null;
      el?.focus();
      el?.select();
    });
  }

  /** Directory a create scopes to: the node itself (dir), its parent (file), or root. */
  private scopeDir(node: FileNode | null): string {
    if (!node) return "";
    const p = node.path.replace(/\\/g, "/");
    if (node.isDir) return p;
    const i = p.lastIndexOf("/");
    return i === -1 ? "" : p.slice(0, i);
  }

  async commit(): Promise<void> {
    const m = this.menu();
    const name = this.nameInput().trim();
    if (!m || !name) return;
    const mode = this.menuMode();
    const id = this.agent().id;
    try {
      if (mode === "rename") {
        const from = m.node!.path.replace(/\\/g, "/");
        const dir = this.scopeDir({ ...m.node!, isDir: false }); // parent of the node
        const to = dir ? `${dir}/${name}` : name;
        await this.bridge.invoke(Commands.FileRename, { id, from, to });
        this.edits.close(id, from); // stale buffer under the old path
        this.scroll.clear(id, from);
      } else {
        const base = this.scopeDir(m.node);
        const path = base ? `${base}/${name}` : name;
        const cmd = mode === "create-dir" ? Commands.DirCreate : Commands.FileCreate;
        await this.bridge.invoke(cmd, { id, path });
        if (mode === "create-file") this.ui.openFileInWorkspace(id, path);
      }
      this.closeMenu();
      this.work.loadTree(id);
    } catch (e) {
      this.ui.flash(e instanceof Error ? e.message : String(e));
    }
  }

  /** File-manager wording the user's OS actually uses. */
  readonly revealLabel = revealLabelFor(navigator.userAgent);

  /** Hand the node to the OS: its associated app (an .html → the browser, a
   *  .png → the image viewer). Directories open in the file manager. */
  openExternal(node: FileNode): void {
    this.toOs(Commands.FileOpenExternal, node, "couldn't open");
  }

  /** Show the node in the OS file manager with the item selected. */
  reveal(node: FileNode): void {
    this.toOs(Commands.FileReveal, node, "couldn't reveal");
  }

  private toOs(command: string, node: FileNode, failed: string): void {
    const path = node.path.replace(/\\/g, "/");
    this.closeMenu();
    void this.bridge
      .invoke(command, { id: this.agent().id, path })
      .catch((e: unknown) => this.ui.flash(`${failed} ${node.name}: ${msgOf(e)}`));
  }

  async confirmDelete(): Promise<void> {
    const m = this.menu();
    if (!m?.node) return;
    const id = this.agent().id;
    try {
      const path = m.node.path.replace(/\\/g, "/");
      await this.bridge.invoke(Commands.FileDelete, { id, path });
      this.edits.close(id, path);
      this.scroll.clear(id, path);
      this.closeMenu();
      this.work.loadTree(id);
    } catch (e) {
      this.ui.flash(e instanceof Error ? e.message : String(e));
    }
  }

  readonly nodes = computed<FileNode[]>(() => this.work.treeFor(this.agent().id).data);
  readonly loading = computed(() => this.work.treeFor(this.agent().id).status === "loading");
  readonly openMap = signal<Record<string, boolean>>({});

  // git status by (normalized) path → mark changed files in the tree with A/M/D
  readonly stateMap = computed<Record<string, string>>(() => {
    const map: Record<string, string> = {};
    for (const f of this.work.changesFor(this.agent().id).data) {
      map[f.path.replace(/\\/g, "/")] = f.state;
    }
    return map;
  });
  stateOf(path: string): string | undefined {
    return this.stateMap()[path.replace(/\\/g, "/")];
  }
  stateInk(state: string): string {
    return state === "A"
      ? "var(--vcs-added)"
      : state === "D"
        ? "var(--vcs-deleted)"
        : state === "R"
          ? "var(--vcs-renamed)"
          : "var(--vcs-modified)";
  }

  // flatten the open tree into the list of visible rows (depth carries indentation)
  readonly rows = computed<FlatRow[]>(() => {
    const open = this.openMap();
    const out: FlatRow[] = [];
    const walk = (list: FileNode[], depth: number) => {
      for (const n of list) {
        out.push({ node: n, depth });
        if (n.isDir && open[n.path] === true && n.children) walk(n.children, depth + 1);
      }
    };
    walk(this.nodes(), 0);
    return out;
  });

  // keyed by path: each rescan delivers fresh FileNode identities
  readonly trackPath = (_: number, r: FlatRow): string => r.node.path;

  isOpen(node: FileNode): boolean {
    return this.openMap()[node.path] === true;
  }
  // dirs expand/collapse; files open in the agent's workspace (closable file tab)
  onRow(node: FileNode) {
    if (node.isDir) {
      this.toggle(node);
      return;
    }
    this.ui.openFileInWorkspace(this.agent().id, node.path);
  }
  toggle(node: FileNode) {
    if (!node.isDir) return;
    const isOpen = this.isOpen(node);
    if (!isOpen && node.children === null) {
      this.work.expandDir(this.agent().id, node.path); // lazy-load stub folders
    }
    this.openMap.update((m) => ({ ...m, [node.path]: !isOpen }));
  }

  /** Drag a file row toward a prompt / terminal (B1.5). */
  onDragStart(e: DragEvent, node: FileNode): void {
    if (node.isDir) return;
    this.dragSvc.start({ kind: "file", agentId: this.agent().id, relPath: node.path });
    e.dataTransfer?.setData("text/plain", node.path);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "copy";
  }

  wtName(path: string): string {
    return path.replace(/\\/g, "/").split("/").pop() || path;
  }
  /** Manual fallback: re-scan this agent's worktree file tree. */
  refresh() {
    this.work.loadTree(this.agent().id);
  }
}
