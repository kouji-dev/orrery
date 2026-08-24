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
import { NgTemplateOutlet } from "@angular/common";
import { Agent, CommitFile } from "../../models";
import { fileDir, fileName, revealLabelFor } from "../../utils";
import { IconComponent } from "../../shared/icon.component";
import { MenuPanelComponent } from "../../context-menu/menu-panel.component";
import { StateBadgeComponent } from "../../shared/git/state-badge.component";
import { AddDelComponent } from "../../shared/git/add-del.component";
import { BRIDGE, Commands } from "../../data-source/bridge";
import { EditsStore } from "../../stores/edits.store";
import { UiStore } from "../../ui/ui.store";
import {
  KjButtonComponent,
  KjConfirmPopupActionComponent,
  KjConfirmPopupActionsComponent,
  KjConfirmPopupCancelComponent,
  KjConfirmPopupComponent,
  KjConfirmPopupContentComponent,
  KjConfirmPopupMessageComponent,
  KjDividerComponent, KjTabComponent, KjTabListComponent, KjTabsComponent} from "@kouji-ui/components";
import { KjConfirmPopupTrigger } from "@kouji-ui/core";

function msgOf(e: unknown): string {
  return e instanceof Error ? e.message : String(e);
}

// ---- tree node types ----
interface DirNode {
  kind: "dir";
  name: string;
  path: string;
  children: TreeNode[];
}
interface FileNode {
  kind: "file";
  name: string;
  path: string;
  file: CommitFile;
}
type TreeNode = DirNode | FileNode;

function buildTree(files: CommitFile[]): TreeNode[] {
  const root: Record<string, unknown> = {};
  for (const f of files) {
    const parts = f.path.split("/");
    let cur = root as Record<string, unknown>;
    for (let i = 0; i < parts.length; i++) {
      const p = parts[i];
      const leaf = i === parts.length - 1;
      if (!cur[p]) {
        cur[p] = {
          name: p,
          leaf,
          path: parts.slice(0, i + 1).join("/"),
          file: leaf ? f : null,
          children: {},
        };
      }
      cur = ((cur[p] as Record<string, unknown>)["children"] as Record<string, unknown>);
    }
  }

  function walk(obj: Record<string, unknown>): TreeNode[] {
    return (Object.values(obj) as Array<{
      name: string;
      leaf: boolean;
      path: string;
      file: CommitFile | null;
      children: Record<string, unknown>;
    }>)
      .sort((a, b) =>
        a.leaf === b.leaf ? a.name.localeCompare(b.name) : a.leaf ? 1 : -1
      )
      .map((n): TreeNode =>
        n.leaf
          ? { kind: "file", name: n.name, path: n.path, file: n.file! }
          : { kind: "dir", name: n.name, path: n.path, children: walk(n.children) }
      );
  }

  return walk(root as Record<string, unknown>);
}

/**
 * Changed-files panel for commit/range diff views.
 * Shows a flat list (default) or tree view with state badge + ±N stats.
 * Emits `select` with the path when a file row is clicked.
 */
@Component({
  selector: "app-diff-file-list",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [
    NgTemplateOutlet,
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
    <div style="display:flex;flex-direction:column;min-height:0;height:100%">

      <!-- header: label · count, ±total, tree/flat toggle -->
      <div class="pane-head" style="gap:var(--sp-3);padding:var(--sp-3) var(--sp-3) var(--sp-3) var(--sp-5)">
        <span class="up" style="color:var(--ink-3)">
          {{ title() || 'Changed' }} · {{ files().length }}
        </span>
        <app-add-del [add]="totalAdd()" [del]="totalDel()" />
        <!-- tree / flat toggle -->
        <kj-tabs variant="pills" class="tabs-xs" style="margin-left:auto"
                 [value]="treeMode() ? 'tree' : 'flat'" (valueChange)="treeMode.set($event === 'tree')">
          <kj-tab-list aria-label="File list layout">
            <kj-tab value="tree" title="Tree view">
              <app-icon size="md" name="graph" [color]="treeMode() ? 'var(--ui-ink)' : null" />Tree
            </kj-tab>
            <kj-tab value="flat" title="Flat view">
              <app-icon size="md" name="dots" [color]="!treeMode() ? 'var(--ui-ink)' : null" />Flat
            </kj-tab>
          </kj-tab-list>
        </kj-tabs>
      </div>

      <!-- scrollable file list -->
      <div class="scroll-y" style="flex:1;padding:var(--sp-2) 0">

        <!-- flat mode -->
        @if (!treeMode()) {
          @for (f of files(); track f.path) {
            <div
              (click)="select.emit(f.path)"
              (contextmenu)="onContext($event, f)"
              [style.background]="selPath() === f.path ? 'var(--panel-3)' : 'transparent'"
              style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-2) var(--sp-4);cursor:pointer;margin:1px var(--sp-2);border-radius:var(--r-sm)"
              (mouseenter)="onEnter($event, f.path)"
              (mouseleave)="onLeave($event, f.path)"
            >
              <app-state-badge [state]="f.state" />
              <div style="flex:1;min-width:0">
                <div class="trunc" style="color:var(--ink)">
                  {{ name(f.path) }}
                </div>
                @if (dir(f.path)) {
                  <div class="trunc" style="font-size:var(--fs-meta);color:var(--ink-4)">
                    {{ dir(f.path) }}
                  </div>
                }
              </div>
              <app-add-del [add]="f.add" [del]="f.del" />
            </div>
          }
        }

        <!-- tree mode -->
        @if (treeMode()) {
          <ng-template [ngTemplateOutlet]="treeRows" [ngTemplateOutletContext]="{ nodes: tree(), depth: 0 }" />
        }

      </div>
    </div>

    <!-- context menu: file actions, only ever opened from a file row — shared menu chrome -->
    @if (menu(); as m) {
      <app-menu-panel [x]="m.x" [y]="m.y" (closed)="closeMenu()">
        @switch (menuMode()) {
          @case ("actions") {
            <kj-button kjVariant="ghost" [kjFullWidth]="true" class="menu-item" (click)="startRename()"><app-icon size="md" name="rename" />Rename…</kj-button>
            <kj-divider />
            <kj-button kjVariant="ghost" [kjFullWidth]="true" class="menu-item" (click)="openExternal(m.file)"><app-icon size="md" name="ext" />Open in Default App</kj-button>
            <kj-button kjVariant="ghost" [kjFullWidth]="true" class="menu-item" (click)="reveal(m.file)"><app-icon size="md" name="folderOpen" />{{ revealLabel }}</kj-button>
            <kj-divider />
            <kj-confirm-popup [kjDestructive]="true" (kjConfirmed)="confirmDelete()">
              <kj-button kjConfirmPopupTrigger #delTrig="kjConfirmPopupTrigger" kjVariant="danger" [kjFullWidth]="true" class="menu-item"><app-icon size="md" name="trash" />Delete</kj-button>
              <kj-confirm-popup-content [kjFor]="delTrig">
                <kj-confirm-popup-message>Delete <b>{{ name(m.file.path) }}</b>?</kj-confirm-popup-message>
                <kj-confirm-popup-actions>
                  <kj-confirm-popup-cancel><kj-button kjVariant="outline">Cancel</kj-button></kj-confirm-popup-cancel>
                  <kj-confirm-popup-action><kj-button kjVariant="danger">Delete</kj-button></kj-confirm-popup-action>
                </kj-confirm-popup-actions>
              </kj-confirm-popup-content>
            </kj-confirm-popup>
          }
          @default {
            <div class="menu-label">Rename {{ name(m.file.path) }}</div>
            <input
              class="menu-input"
              [value]="nameInput()"
              (input)="nameInput.set($any($event.target).value)"
              (keydown.enter)="commitRename()"
              (keydown.escape)="closeMenu()"
              spellcheck="false"
            />
            <div class="menu-row">
              <kj-button kjVariant="outline" (click)="closeMenu()">Cancel</kj-button>
              <kj-button kjVariant="default" [kjDisabled]="!nameInput().trim()" (click)="commitRename()">OK</kj-button>
            </div>
          }
        }
      </app-menu-panel>
    }

    <!-- recursive tree row template -->
    <ng-template #treeRows let-nodes="nodes" let-depth="depth">
      @for (node of nodes; track node.path) {
        @if (node.kind === 'dir') {
          <div [style.padding-left.px]="8 + depth * 13" style="display:flex;align-items:center;gap:var(--sp-2);padding-top:var(--sp-1);padding-bottom:var(--sp-1);padding-right:var(--sp-2)">
            <app-icon size="lg" name="folderOpen" style="color:var(--ink-4);flex:none" />
            <span style="font-size:var(--fs-meta);color:var(--ink-3)">{{ node.name }}</span>
          </div>
          <ng-template [ngTemplateOutlet]="treeRows" [ngTemplateOutletContext]="{ nodes: node.children, depth: depth + 1 }" />
        } @else {
          <div
            (click)="select.emit(node.path)"
            (contextmenu)="onContext($event, node.file)"
            [style.padding-left.px]="8 + depth * 13 + 4"
            [style.background]="selPath() === node.path ? 'var(--panel-3)' : 'transparent'"
            style="display:flex;align-items:center;gap:var(--sp-3);padding-top:var(--sp-1);padding-bottom:var(--sp-1);padding-right:var(--sp-3);cursor:pointer;margin:1px var(--sp-2);border-radius:var(--r-sm)"
            (mouseenter)="onEnter($event, node.path)"
            (mouseleave)="onLeave($event, node.path)"
          >
            <app-state-badge [state]="node.file.state" />
            <span
              class="trunc"
              style="flex:1"
              [style.color]="selPath() === node.path ? 'var(--ink)' : 'var(--ink-2)'"
            >{{ node.name }}</span>
            <app-add-del [add]="node.file.add" [del]="node.file.del" />
          </div>
        }
      }
    </ng-template>
  `,
})
export class DiffFileListComponent {
  private readonly bridge = inject(BRIDGE);
  private readonly ui = inject(UiStore);
  private readonly edits = inject(EditsStore);
  private readonly host = inject(ElementRef<HTMLElement>);

  readonly agent = input.required<Agent>();
  readonly files = input<CommitFile[]>([]);
  readonly selPath = input<string | null | undefined>(null);
  readonly title = input<string>("");

  readonly select = output<string>();

  readonly treeMode = signal(true);

  // ----- context-menu file actions (rename / open / reveal / delete) -----
  readonly menu = signal<{ x: number; y: number; file: CommitFile } | null>(null);
  readonly menuMode = signal<"actions" | "rename">("actions");
  readonly nameInput = signal("");
  readonly revealLabel = revealLabelFor(navigator.userAgent);

  onContext(e: MouseEvent, file: CommitFile): void {
    e.preventDefault();
    e.stopPropagation();
    this.menu.set({ x: e.clientX, y: e.clientY, file });
    this.menuMode.set("actions");
    this.nameInput.set("");
  }

  closeMenu(): void {
    this.menu.set(null);
  }

  startRename(): void {
    this.menuMode.set("rename");
    this.nameInput.set(this.name(this.menu()!.file.path));
    queueMicrotask(() => {
      const el = this.host.nativeElement.querySelector(".dfl-input") as HTMLInputElement | null;
      el?.focus();
      el?.select();
    });
  }

  async commitRename(): Promise<void> {
    const m = this.menu();
    const newName = this.nameInput().trim();
    if (!m || !newName) return;
    const from = m.file.path.replace(/\\/g, "/");
    const i = from.lastIndexOf("/");
    const dir = i === -1 ? "" : from.slice(0, i);
    const to = dir ? `${dir}/${newName}` : newName;
    try {
      await this.bridge.invoke(Commands.FileRename, { id: this.agent().id, from, to });
      this.edits.close(this.agent().id, from);
      this.closeMenu();
    } catch (e) {
      this.ui.flash(msgOf(e));
    }
  }

  openExternal(file: CommitFile): void {
    this.toOs(Commands.FileOpenExternal, file, "couldn't open");
  }

  reveal(file: CommitFile): void {
    this.toOs(Commands.FileReveal, file, "couldn't reveal");
  }

  private toOs(command: string, file: CommitFile, failed: string): void {
    const path = file.path.replace(/\\/g, "/");
    this.closeMenu();
    void this.bridge
      .invoke(command, { id: this.agent().id, path })
      .catch((e: unknown) => this.ui.flash(`${failed} ${this.name(path)}: ${msgOf(e)}`));
  }

  async confirmDelete(): Promise<void> {
    const m = this.menu();
    if (!m) return;
    const path = m.file.path.replace(/\\/g, "/");
    // dismiss first, like every other row action (toOs): the confirmation has
    // already been given, so the menu has nothing left to say — and a failed
    // delete reports through the flash, not by leaving the menu hanging open.
    this.closeMenu();
    try {
      await this.bridge.invoke(Commands.FileDelete, { id: this.agent().id, path });
      this.edits.close(this.agent().id, path);
    } catch (e) {
      this.ui.flash(msgOf(e));
    }
  }

  readonly totalAdd = computed(() => this.files().reduce((s, f) => s + f.add, 0));
  readonly totalDel = computed(() => this.files().reduce((s, f) => s + f.del, 0));

  readonly tree = computed(() => buildTree(this.files()));

  name(path: string): string {
    return fileName(path);
  }

  dir(path: string): string {
    return fileDir(path);
  }

  onEnter(event: MouseEvent, path: string): void {
    if (this.selPath() !== path) {
      (event.currentTarget as HTMLElement).style.background = "var(--panel-2)";
    }
  }

  onLeave(event: MouseEvent, path: string): void {
    if (this.selPath() !== path) {
      (event.currentTarget as HTMLElement).style.background = "transparent";
    }
  }
}
