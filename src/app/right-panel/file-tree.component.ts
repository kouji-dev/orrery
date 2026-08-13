import { ScrollingModule } from "@angular/cdk/scrolling";
import {
  ChangeDetectionStrategy,
  Component,
  computed,
  ElementRef,
  HostListener,
  inject,
  input,
  signal,
} from "@angular/core";
import { Agent, FileNode, Project } from "../models";
import { AgentWorkStore } from "../agents/agent-work.store";
import { BRIDGE, Commands } from "../data-source/bridge";
import { DragService } from "../shared/drag.service";
import { EditsStore } from "../stores/edits.store";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { revealLabelFor } from "../utils";

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
  imports: [IconComponent, ScrollingModule],
  template: `
    @let ag = agent();
    <!-- root-scoped context menu anywhere in the panel — header, empty space
         below the rows, or the empty/loading states. Row handlers stop
         propagation, so node-scoped menus still win on the rows themselves. -->
    <div (contextmenu)="onContext($event, null)" style="display:flex;flex-direction:column;min-height:0;flex:1">
      <div style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-4) var(--sp-6);border-bottom:1px solid var(--hair)">
        <app-icon name="folder" size="sm" [color]="project() ? project()!.color : 'var(--accent)'" />
        <span style="flex:1;min-width:0;font-size:var(--fs-sm);color:var(--ink-2);overflow:hidden;text-overflow:ellipsis;white-space:nowrap" [title]="ag.worktree">{{ wtName(ag.worktree) }}</span>
        @if (loading()) { <span class="tnum" style="font-size:var(--fs-2xs);color:var(--ink-4);flex:none">scanning…</span> }
        <button class="btn" (click)="refresh()" [disabled]="loading()" title="Rescan worktree" style="padding:var(--sp-1);border-radius:4px;flex:none"><app-icon name="refresh" size="sm" [px]="12" /></button>
      </div>

      @if (loading()) {
        <div style="padding:var(--sp-4) var(--sp-6);font-size:var(--fs-xs);color:var(--ink-4)">scanning worktree…</div>
      } @else if (rows().length) {
        <!-- virtualized: only visible rows are rendered -->
        <cdk-virtual-scroll-viewport itemSize="24" minBufferPx="240" maxBufferPx="480" style="flex:1" class="scroll-y">
          <div
            *cdkVirtualFor="let row of rows()"
            (click)="onRow(row.node)"
            (contextmenu)="onContext($event, row.node)"
            [attr.draggable]="row.node.isDir ? null : 'true'"
            (dragstart)="onDragStart($event, row.node)"
            (dragend)="dragSvc.end()"
            [style.padding-left.px]="8 + row.depth * 13"
            style="height:var(--sp-9);display:flex;align-items:center;gap:var(--sp-3);cursor:pointer;padding-right:var(--sp-4);border-radius:5px"
          >
            @if (row.node.isDir) {
              <app-icon [name]="isOpen(row.node) ? 'chevronD' : 'chevron'" size="sm" [px]="11" color="var(--ink-4)" />
              <app-icon [name]="isOpen(row.node) ? 'folderOpen' : 'folder'" size="sm" [px]="13" [color]="row.node.ignored ? 'var(--ink-4)' : 'var(--accent)'" />
            } @else {
              <span style="width:11px;flex:none"></span>
              <app-icon name="file" size="sm" [px]="12" [color]="stateOf(row.node.path) ? stateInk(stateOf(row.node.path)!) : 'var(--ink-4)'" />
            }
            <span
              [style.color]="row.node.ignored ? 'var(--ink-4)' : row.node.isDir ? 'var(--ink-2)' : 'var(--ink-3)'"
              [style.opacity]="row.node.ignored ? 0.7 : 1"
              style="font-size:var(--fs-sm);overflow:hidden;text-overflow:ellipsis;white-space:nowrap"
            >{{ row.node.name }}</span>
            @if (row.node.ignored) {
              <span class="chip" style="margin-left:auto;font-size:var(--fs-3xs);padding:0 var(--sp-2);color:var(--ink-4)">ignored</span>
            } @else if (!row.node.isDir && stateOf(row.node.path); as st) {
              <span class="tnum" [style.color]="stateInk(st)" style="margin-left:auto;flex:none;font-size:var(--fs-2xs);font-weight:700;padding-left:var(--sp-3)">{{ st }}</span>
            }
          </div>
        </cdk-virtual-scroll-viewport>
      } @else {
        <div style="padding:var(--sp-4) var(--sp-6);font-size:var(--fs-xs);color:var(--ink-4)">empty worktree</div>
      }
    </div>

    <!-- context menu: file CRUD (B1.1) -->
    @if (menu(); as m) {
      <div class="ft-menu rise" [style.left.px]="m.x" [style.top.px]="m.y" (mousedown)="$event.stopPropagation()" (contextmenu)="$event.preventDefault()">
        @switch (menuMode()) {
          @case ("actions") {
            <button class="ft-mi" (click)="startInput('create-file')"><app-icon name="file" size="sm" [px]="12" />New File…</button>
            <button class="ft-mi" (click)="startInput('create-dir')"><app-icon name="folder" size="sm" [px]="12" />New Folder…</button>
            @if (m.node) {
              <button class="ft-mi" (click)="startRename()"><app-icon name="rename" size="sm" [px]="12" />Rename…</button>
              <div class="ft-sep"></div>
              <button class="ft-mi" (click)="openExternal(m.node)"><app-icon name="ext" size="sm" [px]="12" />Open in Default App</button>
              <button class="ft-mi" (click)="reveal(m.node)"><app-icon name="folderOpen" size="sm" [px]="12" />{{ revealLabel }}</button>
              <div class="ft-sep"></div>
              <button class="ft-mi danger" (click)="menuMode.set('delete')"><app-icon name="trash" size="sm" [px]="12" />Delete</button>
            }
          }
          @case ("delete") {
            <div class="ft-label">Delete <b>{{ m.node?.name }}</b>{{ m.node?.isDir ? ' and its contents' : '' }}?</div>
            <div class="ft-row">
              <button class="btn ghost-hair" (click)="closeMenu()">Cancel</button>
              <button class="btn ghost-hair danger" (click)="confirmDelete()">Delete</button>
            </div>
          }
          @default {
            <div class="ft-label">{{ inputLabel() }}</div>
            <input
              class="ft-input"
              [value]="nameInput()"
              (input)="nameInput.set($any($event.target).value)"
              (keydown.enter)="commit()"
              (keydown.escape)="closeMenu()"
              spellcheck="false"
            />
            <div class="ft-row">
              <button class="btn ghost-hair" (click)="closeMenu()">Cancel</button>
              <button class="btn primary" [disabled]="!nameInput().trim()" (click)="commit()">OK</button>
            </div>
          }
        }
      </div>
    }
  `,
  styles: [
    `
      .ft-menu {
        position: fixed;
        z-index: 60;
        min-width: 190px;
        background: var(--elev);
        border: 1px solid var(--hair-2);
        border-radius: var(--r-md);
        box-shadow: var(--shadow);
        padding: var(--sp-2);
      }
      .ft-mi {
        display: flex;
        align-items: center;
        gap: var(--sp-3);
        width: 100%;
        text-align: left;
        padding: var(--sp-2) var(--sp-4);
        border: none;
        border-radius: 5px;
        background: transparent;
        color: var(--ink-2);
        font-size: var(--fs-sm);
        cursor: pointer;
      }
      .ft-mi:hover {
        background: var(--panel-3);
        color: var(--ink);
      }
      .ft-mi.danger:hover,
      .btn.danger {
        color: var(--code-del-ink);
      }
      .ft-sep {
        height: 1px;
        margin: var(--sp-2) var(--sp-1);
        background: var(--hair);
      }
      .ft-label {
        padding: var(--sp-2) var(--sp-4);
        font-size: var(--fs-xs);
        color: var(--ink-3);
        max-width: 260px;
        overflow-wrap: anywhere;
      }
      .ft-label b {
        color: var(--ink);
      }
      .ft-input {
        width: 100%;
        margin: var(--sp-1) 0;
        background: var(--panel-2);
        border: 1px solid var(--hair);
        border-radius: var(--r-sm);
        padding: var(--sp-2) var(--sp-3);
        color: var(--ink);
        font-family: var(--font-mono);
        font-size: var(--fs-sm);
        outline: none;
      }
      .ft-input:focus {
        border-color: color-mix(in oklch, var(--accent), transparent 50%);
      }
      .ft-row {
        display: flex;
        justify-content: flex-end;
        gap: var(--sp-3);
        padding: var(--sp-2) var(--sp-1) var(--sp-1);
      }
      .ft-row .btn {
        padding: var(--sp-1) var(--sp-4);
        font-size: var(--fs-xs);
      }
    `,
  ],
})
export class FileTreeComponent {
  private work = inject(AgentWorkStore);
  private ui = inject(UiStore);
  private bridge = inject(BRIDGE);
  private edits = inject(EditsStore);
  private host = inject(ElementRef<HTMLElement>);
  readonly dragSvc = inject(DragService);
  readonly agent = input.required<Agent>();
  readonly project = input<Project | undefined>(undefined);

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
    const id = this.agent().id;
    try {
      if (mode === "rename") {
        const from = m.node!.path.replace(/\\/g, "/");
        const dir = this.scopeDir({ ...m.node!, isDir: false }); // parent of the node
        const to = dir ? `${dir}/${name}` : name;
        await this.bridge.invoke(Commands.FileRename, { id, from, to });
        this.edits.close(id, from); // stale buffer under the old path
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
      this.closeMenu();
      this.work.loadTree(id);
    } catch (e) {
      this.ui.flash(e instanceof Error ? e.message : String(e));
    }
  }

  @HostListener("document:mousedown")
  onDocDown(): void {
    if (this.menu()) this.closeMenu();
  }

  @HostListener("document:keydown.escape")
  onDocEsc(): void {
    if (this.menu()) this.closeMenu();
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
      ? "var(--code-add-ink)"
      : state === "D"
        ? "var(--code-del-ink)"
        : state === "R"
          ? "var(--accent)"
          : "var(--accent-2)";
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
