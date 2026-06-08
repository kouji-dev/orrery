import { ChangeDetectionStrategy, Component, computed, input, output } from "@angular/core";
import { FileNode } from "../models";
import { IconComponent } from "../shared/icon.component";

@Component({
  selector: "app-tree-node",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    @let n = node();
    @if (n.isDir) {
      <div>
        <div
          class="tree-row"
          (click)="toggle.emit(n)"
          [style.padding-left.px]="pad()"
          style="display:flex;align-items:center;gap:6px;padding:3px 8px;cursor:pointer;border-radius:5px"
        >
          <app-icon [name]="open() ? 'chevronD' : 'chevron'" size="sm" [px]="11" color="var(--ink-4)" />
          <app-icon [name]="open() ? 'folderOpen' : 'folder'" size="sm" [px]="13" [color]="n.ignored ? 'var(--ink-4)' : 'var(--accent)'" />
          <span [style.color]="n.ignored ? 'var(--ink-4)' : 'var(--ink-2)'" [style.opacity]="n.ignored ? 0.7 : 1" style="font-size:11.5px">{{ n.name }}</span>
          @if (n.ignored) { <span class="chip" style="margin-left:auto;font-size:8px;padding:0 4px;color:var(--ink-4)">ignored</span> }
        </div>
        @if (open()) {
          @if (n.children) {
            @for (c of n.children; track c.path) {
              <app-tree-node [node]="c" [depth]="depth() + 1" [openMap]="openMap()" (toggle)="toggle.emit($event)" />
            }
          } @else {
            <div [style.padding-left.px]="pad() + 30" style="font-size:10px;color:var(--ink-4);padding-top:2px;padding-bottom:2px">loading…</div>
          }
        }
      </div>
    } @else {
      <div
        class="tree-row"
        [style.padding-left.px]="pad() + 17"
        style="display:flex;align-items:center;gap:6px;padding:3px 8px;border-radius:5px"
      >
        <app-icon name="file" size="sm" [px]="12" color="var(--ink-4)" />
        <span [style.color]="n.ignored ? 'var(--ink-4)' : 'var(--ink-3)'" [style.opacity]="n.ignored ? 0.7 : 1" style="font-size:11.5px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ n.name }}</span>
      </div>
    }
  `,
})
export class TreeNodeComponent {
  readonly node = input.required<FileNode>();
  readonly depth = input<number>(0);
  readonly openMap = input<Record<string, boolean>>({});
  readonly toggle = output<FileNode>();

  readonly pad = computed(() => 8 + this.depth() * 13);
  readonly open = computed(() => this.openMap()[this.node().path] === true); // default collapsed
}
