import { ScrollingModule } from "@angular/cdk/scrolling";
import { ChangeDetectionStrategy, Component, computed, inject, input, signal } from "@angular/core";
import { Agent, FileNode, Project } from "../models";
import { AgentWorkStore } from "../agents/agent-work.store";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";

interface FlatRow {
  node: FileNode;
  depth: number;
}

@Component({
  selector: "app-file-tree",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, ScrollingModule],
  template: `
    @let ag = agent();
    <div style="display:flex;flex-direction:column;min-height:0;flex:1">
      <div style="display:flex;align-items:center;gap:7px;padding:8px 12px;border-bottom:1px solid var(--hair)">
        <app-icon name="folder" size="sm" [color]="project() ? project()!.color : 'var(--accent)'" />
        <span style="flex:1;min-width:0;font-size:11.5px;color:var(--ink-2);overflow:hidden;text-overflow:ellipsis;white-space:nowrap" [title]="ag.worktree">{{ wtName(ag.worktree) }}</span>
        @if (loading()) { <span class="tnum" style="font-size:9px;color:var(--ink-4);flex:none">scanning…</span> }
        <button class="btn" (click)="refresh()" [disabled]="loading()" title="Rescan worktree" style="padding:3px;border-radius:4px;flex:none"><app-icon name="refresh" size="sm" [px]="12" /></button>
      </div>

      @if (loading()) {
        <div style="padding:8px 12px;font-size:10.5px;color:var(--ink-4)">scanning worktree…</div>
      } @else if (rows().length) {
        <!-- virtualized: only visible rows are rendered -->
        <cdk-virtual-scroll-viewport itemSize="24" minBufferPx="240" maxBufferPx="480" style="flex:1" class="scroll-y">
          <div
            *cdkVirtualFor="let row of rows()"
            (click)="onRow(row.node)"
            [style.padding-left.px]="8 + row.depth * 13"
            style="height:24px;display:flex;align-items:center;gap:6px;cursor:pointer;padding-right:8px;border-radius:5px"
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
              style="font-size:11.5px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap"
            >{{ row.node.name }}</span>
            @if (row.node.ignored) {
              <span class="chip" style="margin-left:auto;font-size:8px;padding:0 4px;color:var(--ink-4)">ignored</span>
            } @else if (!row.node.isDir && stateOf(row.node.path); as st) {
              <span class="tnum" [style.color]="stateInk(st)" style="margin-left:auto;flex:none;font-size:9px;font-weight:700;padding-left:6px">{{ st }}</span>
            }
          </div>
        </cdk-virtual-scroll-viewport>
      } @else {
        <div style="padding:8px 12px;font-size:10.5px;color:var(--ink-4)">empty worktree</div>
      }
    </div>
  `,
})
export class FileTreeComponent {
  private work = inject(AgentWorkStore);
  private ui = inject(UiStore);
  readonly agent = input.required<Agent>();
  readonly project = input<Project | undefined>(undefined);

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

  wtName(path: string): string {
    return path.replace(/\\/g, "/").split("/").pop() || path;
  }
  /** Manual fallback: re-scan this agent's worktree file tree. */
  refresh() {
    this.work.loadTree(this.agent().id);
  }
}
