import { ScrollingModule } from "@angular/cdk/scrolling";
import { ChangeDetectionStrategy, Component, computed, inject, input, signal } from "@angular/core";
import { Agent, FileNode, Project } from "../models";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
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
        <span style="font-size:11.5px;color:var(--ink-2);overflow:hidden;text-overflow:ellipsis;white-space:nowrap" [title]="ag.worktree">{{ wtName(ag.worktree) }}</span>
        @if (loading()) { <span class="tnum" style="margin-left:auto;font-size:9px;color:var(--ink-4)">scanning…</span> }
      </div>

      @if (loading()) {
        <div style="padding:8px 12px;font-size:10.5px;color:var(--ink-4)">scanning worktree…</div>
      } @else if (rows().length) {
        <!-- virtualized: only visible rows are rendered -->
        <cdk-virtual-scroll-viewport itemSize="24" minBufferPx="240" maxBufferPx="480" style="flex:1" class="scroll-y">
          <div
            *cdkVirtualFor="let row of rows()"
            (click)="toggle(row.node)"
            [style.padding-left.px]="8 + row.depth * 13"
            style="height:24px;display:flex;align-items:center;gap:6px;cursor:pointer;padding-right:8px;border-radius:5px"
          >
            @if (row.node.isDir) {
              <app-icon [name]="isOpen(row.node) ? 'chevronD' : 'chevron'" size="sm" [px]="11" color="var(--ink-4)" />
              <app-icon [name]="isOpen(row.node) ? 'folderOpen' : 'folder'" size="sm" [px]="13" [color]="row.node.ignored ? 'var(--ink-4)' : 'var(--accent)'" />
            } @else {
              <span style="width:11px;flex:none"></span>
              <app-icon name="file" size="sm" [px]="12" color="var(--ink-4)" />
            }
            <span
              [style.color]="row.node.ignored ? 'var(--ink-4)' : row.node.isDir ? 'var(--ink-2)' : 'var(--ink-3)'"
              [style.opacity]="row.node.ignored ? 0.7 : 1"
              style="font-size:11.5px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap"
            >{{ row.node.name }}</span>
            @if (row.node.ignored) { <span class="chip" style="margin-left:auto;font-size:8px;padding:0 4px;color:var(--ink-4)">ignored</span> }
          </div>
        </cdk-virtual-scroll-viewport>
      } @else {
        <div style="padding:8px 12px;font-size:10.5px;color:var(--ink-4)">empty worktree</div>
      }
    </div>
  `,
})
export class FileTreeComponent {
  private runtime = inject(AgentRuntimeService);
  readonly agent = input.required<Agent>();
  readonly project = input<Project | undefined>(undefined);

  readonly nodes = computed<FileNode[]>(() => this.agent().files?.nodes ?? []);
  readonly loading = computed(() => this.agent().files?.loading ?? false);
  readonly openMap = signal<Record<string, boolean>>({});

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
  toggle(node: FileNode) {
    if (!node.isDir) return;
    const isOpen = this.isOpen(node);
    if (!isOpen && node.children === null) {
      this.runtime.expandDir(this.agent().id, node.path); // lazy-load stub folders
    }
    this.openMap.update((m) => ({ ...m, [node.path]: !isOpen }));
  }

  wtName(path: string): string {
    return path.replace(/\\/g, "/").split("/").pop() || path;
  }
}
