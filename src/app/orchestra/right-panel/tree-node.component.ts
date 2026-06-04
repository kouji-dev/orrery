import { ChangeDetectionStrategy, Component, computed, input, output } from "@angular/core";
import { IconComponent } from "../shared/icon.component";
import { countChanged, STATE_COLOR, TreeItem } from "./tree";

@Component({
  selector: "app-tree-node",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    @let n = node();
    @if (n.dir) {
      <div>
        <div
          class="tree-row"
          (click)="toggle.emit(n.path)"
          [style.padding-left.px]="pad()"
          style="display:flex;align-items:center;gap:6px;padding:3px 8px;cursor:pointer;border-radius:5px"
        >
          <app-icon [name]="open() ? 'chevronD' : 'chevron'" size="sm" [px]="11" color="var(--ink-4)" />
          <app-icon [name]="open() ? 'folderOpen' : 'folder'" size="sm" [px]="13" [color]="n.state ? color(n.state) : 'var(--accent)'" />
          <span style="font-size:11.5px;color:var(--ink-2)">{{ n.name }}</span>
          @if (n.state) { <span [style.color]="color(n.state)" style="font-size:9px;font-weight:700">{{ n.state }}</span> }
          @if (changed() > 0) { <span class="tnum" style="margin-left:auto;font-size:9px;color:var(--ink-4)">{{ changed() }}</span> }
        </div>
        @if (open()) {
          @for (c of n.children; track c.path) {
            <app-tree-node [node]="c" [depth]="depth() + 1" [openMap]="openMap()" (toggle)="toggle.emit($event)" />
          }
        }
      </div>
    } @else {
      <div
        class="tree-row"
        [style.padding-left.px]="pad() + 17"
        style="display:flex;align-items:center;gap:6px;padding:3px 8px;cursor:pointer;border-radius:5px"
      >
        <app-icon name="file" size="sm" [px]="12" [color]="n.state ? color(n.state) : 'var(--ink-4)'" />
        <span [style.color]="n.state ? 'var(--ink)' : 'var(--ink-3)'" style="font-size:11.5px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ n.name }}</span>
        @if (n.state) { <span [style.color]="color(n.state)" style="margin-left:auto;font-size:9px;font-weight:700">{{ n.state }}</span> }
      </div>
    }
  `,
})
export class TreeNodeComponent {
  readonly node = input.required<TreeItem>();
  readonly depth = input<number>(0);
  readonly openMap = input<Record<string, boolean>>({});
  readonly toggle = output<string>();

  readonly pad = computed(() => 8 + this.depth() * 13);
  readonly open = computed(() => this.openMap()[this.node().path] !== false); // default open
  readonly changed = computed(() => countChanged(this.node()));

  color(state: string): string {
    return STATE_COLOR[state];
  }
}
