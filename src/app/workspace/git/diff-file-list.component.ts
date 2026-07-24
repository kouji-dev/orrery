import {
  ChangeDetectionStrategy,
  Component,
  computed,
  input,
  output,
  signal,
} from "@angular/core";
import { NgTemplateOutlet } from "@angular/common";
import { CommitFile } from "../../models";
import { fileDir, fileName } from "../../utils";
import { IconComponent } from "../../shared/icon.component";
import { StateBadgeComponent } from "../../shared/git/state-badge.component";
import { AddDelComponent } from "../../shared/git/add-del.component";

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
  standalone: true,
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [NgTemplateOutlet, IconComponent, StateBadgeComponent, AddDelComponent],
  template: `
    <div style="display:flex;flex-direction:column;min-height:0;height:100%">

      <!-- header: label · count, ±total, tree/flat toggle -->
      <div style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-3) var(--sp-3) var(--sp-3) var(--sp-5);border-bottom:1px solid var(--hair);flex:none">
        <span class="up" style="font-size:var(--fs-2xs);color:var(--ink-3)">
          {{ title() || 'Changed' }} · {{ files().length }}
        </span>
        <app-add-del [add]="totalAdd()" [del]="totalDel()" />
        <!-- tree / flat toggle -->
        <div style="margin-left:auto;display:flex;gap:var(--sp-1);padding:var(--sp-1);background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-sm)">
          <button
            class="btn"
            (click)="treeMode.set(true)"
            title="Tree view"
            [style.background]="treeMode() ? 'var(--panel-3)' : 'transparent'"
            [style.color]="treeMode() ? 'var(--ink)' : 'var(--ink-3)'"
            [style.box-shadow]="treeMode() ? '0 0 0 1px var(--hair-2)' : 'none'"
            style="padding:var(--sp-1) var(--sp-3);border-radius:4px;gap:var(--sp-2);font-size:var(--fs-xs)"
          >
            <app-icon name="graph" size="sm" [px]="12" [color]="treeMode() ? 'var(--accent)' : null" />Tree
          </button>
          <button
            class="btn"
            (click)="treeMode.set(false)"
            title="Flat view"
            [style.background]="!treeMode() ? 'var(--panel-3)' : 'transparent'"
            [style.color]="!treeMode() ? 'var(--ink)' : 'var(--ink-3)'"
            [style.box-shadow]="!treeMode() ? '0 0 0 1px var(--hair-2)' : 'none'"
            style="padding:var(--sp-1) var(--sp-3);border-radius:4px;gap:var(--sp-2);font-size:var(--fs-xs)"
          >
            <app-icon name="dots" size="sm" [px]="12" [color]="!treeMode() ? 'var(--accent)' : null" />Flat
          </button>
        </div>
      </div>

      <!-- scrollable file list -->
      <div class="scroll-y" style="flex:1;padding:var(--sp-2) 0">

        <!-- flat mode -->
        @if (!treeMode()) {
          @for (f of files(); track f.path) {
            <div
              (click)="select.emit(f.path)"
              [style.background]="selPath() === f.path ? 'var(--panel-3)' : 'transparent'"
              style="display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-2) var(--sp-4);cursor:pointer;margin:1px var(--sp-2);border-radius:var(--r-sm)"
              (mouseenter)="onEnter($event, f.path)"
              (mouseleave)="onLeave($event, f.path)"
            >
              <app-state-badge [state]="f.state" />
              <div style="flex:1;min-width:0">
                <div style="font-size:var(--fs-sm);overflow:hidden;text-overflow:ellipsis;white-space:nowrap;color:var(--ink)">
                  {{ name(f.path) }}
                </div>
                @if (dir(f.path)) {
                  <div style="font-size:var(--fs-2xs);color:var(--ink-4);overflow:hidden;text-overflow:ellipsis;white-space:nowrap">
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
          <ng-container *ngTemplateOutlet="treeRows; context: { nodes: tree(), depth: 0 }" />
        }

      </div>
    </div>

    <!-- recursive tree row template -->
    <ng-template #treeRows let-nodes="nodes" let-depth="depth">
      @for (node of nodes; track node.path) {
        @if (node.kind === 'dir') {
          <div [style.padding-left.px]="8 + depth * 13" style="display:flex;align-items:center;gap:var(--sp-2);padding-top:var(--sp-1);padding-bottom:var(--sp-1);padding-right:var(--sp-2)">
            <app-icon name="folderOpen" size="sm" [px]="13" style="color:var(--ink-4);flex:none" />
            <span style="font-size:var(--fs-xs);color:var(--ink-3)">{{ node.name }}</span>
          </div>
          <ng-container *ngTemplateOutlet="treeRows; context: { nodes: node.children, depth: depth + 1 }" />
        } @else {
          <div
            (click)="select.emit(node.path)"
            [style.padding-left.px]="8 + depth * 13 + 4"
            [style.background]="selPath() === node.path ? 'var(--panel-3)' : 'transparent'"
            style="display:flex;align-items:center;gap:var(--sp-3);padding-top:var(--sp-1);padding-bottom:var(--sp-1);padding-right:var(--sp-3);cursor:pointer;margin:1px var(--sp-2);border-radius:var(--r-sm)"
            (mouseenter)="onEnter($event, node.path)"
            (mouseleave)="onLeave($event, node.path)"
          >
            <app-state-badge [state]="node.file.state" />
            <span
              style="flex:1;font-size:var(--fs-sm);overflow:hidden;text-overflow:ellipsis;white-space:nowrap"
              [style.color]="selPath() === node.path ? 'var(--ink)' : 'var(--ink-2)'"
            >{{ node.name }}</span>
            <app-add-del [add]="node.file.add" [del]="node.file.del" />
          </div>
        }
      }
    </ng-template>
  `,
  styles: [
    `
      :host {
        display: flex;
        flex-direction: column;
        min-height: 0;
        height: 100%;
      }
    `,
  ],
})
export class DiffFileListComponent {
  readonly files = input<CommitFile[]>([]);
  readonly selPath = input<string | null | undefined>(null);
  readonly title = input<string>("");

  readonly select = output<string>();

  readonly treeMode = signal(true);

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
