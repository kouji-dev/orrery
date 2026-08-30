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
import { FileNode } from "../../models";
import { AgentWorkStore } from "../../agents/agent-work.store";
import { BRIDGE, Commands } from "../../data-source/bridge";
import { DragService } from "../../shared/drag.service";
import { EditsStore } from "../../stores/edits.store";
import { ScrollStateService } from "../../workspace/scroll-state.service";
import { UiStore } from "../../ui/ui.store";
import { IconComponent } from "../../shared/icon.component";
import { MenuPanelComponent } from "../../context-menu/menu-panel.component";
import { revealLabelFor } from "../../utils";

interface FlatRow {
  node: FileNode;
  depth: number;
}

function msgOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

/**
 * The sidebar files tree (v2): a QUIET navigation tree — name, chevron, muted
 * type glyph, and a small modified dot. No ± counts or state letters; those
 * belong to the center's changed-file list. Rooted at any worktree via
 * `rootKey` (an agent id, or `proj:<id>` for a repo's main worktree — see
 * AgentWorkStore). File CRUD / drag / open are agent-root features for now
 * (the backend file commands are agent-scoped until project tabs land).
 */
@Component({
  selector: "app-sidebar-file-tree",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, MenuPanelComponent, ScrollingModule],
  template: `
    <!-- root-scoped context menu anywhere in the panel — empty space below the
         rows included. Row handlers stop propagation, so node menus win. -->
    <div (contextmenu)="onContext($event, null)" style="display:flex;flex-direction:column;min-height:0;flex:1">
      @if (rows().length) {
        <!-- virtualized: only visible rows are rendered. Data wins over the
             loading flag so background watcher scans never unmount the
             viewport (which would drop its scroll offset and flicker). -->
        <cdk-virtual-scroll-viewport itemSize="24" minBufferPx="240" maxBufferPx="480" style="flex:1" class="scroll-y">
          <div
            *cdkVirtualFor="let row of rows(); trackBy: trackPath"
            (click)="onRow(row.node, $event)"
            (dblclick)="onRowDbl(row.node)"
            (contextmenu)="onContext($event, row.node)"
            [attr.draggable]="row.node.isDir ? null : 'true'"
            (dragstart)="onDragStart($event, row.node)"
            (dragend)="dragSvc.end()"
            [style.padding-left.px]="8 + row.depth * 13"
            [title]="row.node.isDir ? null : row.node.path + ' — click: preview · double-click: pin · Alt+click: split'"
            style="height:var(--sp-9);display:flex;align-items:center;gap:var(--sp-3);cursor:pointer;padding-right:var(--sp-4);border-radius:5px"
          >
            @if (row.node.isDir) {
              <app-icon [name]="isOpen(row.node) ? 'chevronD' : 'chevron'" size="sm" color="var(--ink-4)" />
              <app-icon [name]="isOpen(row.node) ? 'folderOpen' : 'folder'" size="sm" color="var(--ink-4)" />
            } @else {
              <span style="width:round(calc(11px * var(--density)), 1px);flex:none"></span>
              <app-icon name="file" size="sm" color="var(--ink-4)" />
            }
            <span
              [style.color]="row.node.ignored ? 'var(--ink-4)' : row.node.isDir ? 'var(--ink-2)' : 'var(--ink-3)'"
              [style.opacity]="row.node.ignored ? 0.7 : 1"
              style="flex:1;min-width:0;font-size:var(--fs-sm);overflow:hidden;text-overflow:ellipsis;white-space:nowrap"
            >{{ row.node.name }}</span>
            @if (!row.node.isDir && isModified(row.node.path)) {
              <span title="modified in this worktree" style="width:var(--sp-2);height:var(--sp-2);border-radius:50%;background:var(--vcs-modified);flex:none;margin-right:var(--sp-1)"></span>
            }
          </div>
        </cdk-virtual-scroll-viewport>
      } @else if (loading()) {
        <div style="padding:var(--sp-4) var(--sp-6);font-size:var(--fs-xs);color:var(--ink-4)">scanning worktree…</div>
      } @else {
        <div style="padding:var(--sp-4) var(--sp-6);font-size:var(--fs-xs);color:var(--ink-4)">empty worktree</div>
      }
    </div>

    <!-- context menu: file CRUD (B1.1) — shared menu chrome; agent roots only -->
    @if (menu(); as m) {
      <app-menu-panel [x]="m.x" [y]="m.y" (closed)="closeMenu()">
        @switch (menuMode()) {
          @case ("actions") {
            <button class="menu-item" (click)="startInput('create-file')"><app-icon name="file" size="sm" />New File…</button>
            <button class="menu-item" (click)="startInput('create-dir')"><app-icon name="folder" size="sm" />New Folder…</button>
            @if (m.node) {
              <button class="menu-item" (click)="startRename()"><app-icon name="rename" size="sm" />Rename…</button>
              <div class="menu-sep"></div>
              <button class="menu-item" (click)="openExternal(m.node)"><app-icon name="ext" size="sm" />Open in Default App</button>
              <button class="menu-item" (click)="reveal(m.node)"><app-icon name="folderOpen" size="sm" />{{ revealLabel }}</button>
              <div class="menu-sep"></div>
              <button class="menu-item danger" (click)="menuMode.set('delete')"><app-icon name="trash" size="sm" />Delete</button>
            }
          }
          @case ("delete") {
            <div class="menu-label">Delete <b>{{ m.node?.name }}</b>{{ m.node?.isDir ? ' and its contents' : '' }}?</div>
            <div class="menu-row">
              <button class="btn ghost-hair" (click)="closeMenu()">Cancel</button>
              <button class="btn ghost-hair danger" (click)="confirmDelete()">Delete</button>
            </div>
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
              <button class="btn ghost-hair" (click)="closeMenu()">Cancel</button>
              <button class="btn primary" [disabled]="!nameInput().trim()" (click)="commit()">OK</button>
            </div>
          }
        }
      </app-menu-panel>
    }
  `,
})
export class SidebarFileTreeComponent {
  private work = inject(AgentWorkStore);
  private ui = inject(UiStore);
  private bridge = inject(BRIDGE);
  private edits = inject(EditsStore);
  private scroll = inject(ScrollStateService);
  private host = inject(ElementRef<HTMLElement>);
  readonly dragSvc = inject(DragService);

  /** Root the tree is rooted at: an agent id or `proj:<projectId>`. */
  readonly rootKey = input.required<string>();
  /** The agent id when the root is an agent worktree, else null. */
  readonly agentId = computed<string | null>(() =>
    this.rootKey().startsWith("proj:") ? null : this.rootKey(),
  );
  /** The project id when the root is a main worktree, else null. */
  readonly projectId = computed<string | null>(() =>
    this.rootKey().startsWith("proj:") ? this.rootKey().slice(5) : null,
  );
  /** The uuid backend commands take — agent id, or the project id (the backend
   *  resolves a project id to a pseudo record rooted at the repo root). */
  readonly workId = computed<string>(() => this.agentId() ?? this.projectId()!);

  // ----- context-menu file CRUD (B1.1) -----
  readonly menu = signal<{ x: number; y: number; node: FileNode | null } | null>(null);
  readonly menuMode = signal<"actions" | "create-file" | "create-dir" | "rename" | "delete">("actions");
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
    const id = this.workId();
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
        if (mode === "create-file") this.openInWorkspace(path);
      }
      this.closeMenu();
      this.work.loadTree(this.rootKey());
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
    const id = this.workId();
    const path = node.path.replace(/\\/g, "/");
    this.closeMenu();
    void this.bridge
      .invoke(command, { id, path })
      .catch((e: unknown) => this.ui.flash(`${failed} ${node.name}: ${msgOf(e)}`));
  }

  async confirmDelete(): Promise<void> {
    const m = this.menu();
    if (!m?.node) return;
    const id = this.workId();
    try {
      const path = m.node.path.replace(/\\/g, "/");
      await this.bridge.invoke(Commands.FileDelete, { id, path });
      this.edits.close(id, path);
      this.scroll.clear(id, path);
      this.closeMenu();
      this.work.loadTree(this.rootKey());
    } catch (e) {
      this.ui.flash(e instanceof Error ? e.message : String(e));
    }
  }

  readonly nodes = computed<FileNode[]>(() => this.work.treeFor(this.rootKey()).data);
  readonly loading = computed(() => this.work.treeFor(this.rootKey()).status === "loading");
  readonly openMap = signal<Record<string, boolean>>({});

  // changed paths of the AGENT root → the quiet modified dot. Project roots
  // have no per-file change feed yet (arrives with project tabs) — no dots.
  readonly changedSet = computed<Set<string>>(() => {
    const id = this.agentId();
    if (!id) return new Set();
    return new Set(this.work.changesFor(id).data.map((f) => f.path.replace(/\\/g, "/")));
  });
  isModified(path: string): boolean {
    return this.changedSet().has(path.replace(/\\/g, "/"));
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
  // dirs expand/collapse; files open in the root's workspace. v2 semantics:
  // single click = PREVIEW tab (italic — the next preview replaces it),
  // double click = pinned, ⌥click = pinned into a fresh split (the diff stays
  // visible in the other leaf).
  onRow(node: FileNode, e?: MouseEvent) {
    if (node.isDir) {
      this.toggle(node);
      return;
    }
    if (e?.altKey) {
      this.ensureTab();
      this.ui.openFileInSplitWorkspace(this.workId(), node.path);
      return;
    }
    this.ensureTab();
    this.ui.openFilePreviewInWorkspace(this.workId(), node.path);
  }

  /** Double-click pins the file (upright tab that previews never replace). */
  onRowDbl(node: FileNode) {
    if (node.isDir) return;
    this.openInWorkspace(node.path);
  }

  /** A main root opens in the PROJECT tab — create it on first use (v2). */
  private ensureTab(): void {
    const pid = this.projectId();
    if (pid) this.ui.openProject(pid);
  }

  /** Open a file PINNED at this root. */
  private openInWorkspace(path: string): void {
    this.ensureTab();
    this.ui.openFileInWorkspace(this.workId(), path);
  }
  toggle(node: FileNode) {
    if (!node.isDir) return;
    const isOpen = this.isOpen(node);
    if (!isOpen && node.children === null) {
      this.work.expandDir(this.rootKey(), node.path); // lazy-load stub folders
    }
    this.openMap.update((m) => ({ ...m, [node.path]: !isOpen }));
  }

  /** Drag a file row toward a prompt / terminal (B1.5). */
  onDragStart(e: DragEvent, node: FileNode): void {
    if (node.isDir) return;
    this.dragSvc.start({ kind: "file", agentId: this.workId(), relPath: node.path });
    e.dataTransfer?.setData("text/plain", node.path);
    if (e.dataTransfer) e.dataTransfer.effectAllowed = "copy";
  }
}
