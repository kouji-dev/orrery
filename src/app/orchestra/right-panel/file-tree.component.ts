import { ChangeDetectionStrategy, Component, computed, input, signal } from "@angular/core";
import { Agent, Project } from "../models";
import { IconComponent } from "../shared/icon.component";
import { buildTree } from "./tree";
import { TreeNodeComponent } from "./tree-node.component";

@Component({
  selector: "app-file-tree",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, TreeNodeComponent],
  template: `
    @let ag = agent();
    <div style="display:flex;flex-direction:column;min-height:0;flex:1">
      <div style="display:flex;align-items:center;gap:7px;padding:8px 12px;border-bottom:1px solid var(--hair)">
        <app-icon name="folder" size="sm" [color]="project() ? project()!.color : 'var(--accent)'" />
        <span style="font-size:11.5px;color:var(--ink-2)">{{ ag.worktree }}</span>
        <span class="chip tnum" style="margin-left:auto;font-size:9px;padding:0 6px">{{ ag.files.length }} changed</span>
      </div>
      <div class="scroll-y" style="flex:1;padding:6px 4px">
        @for (n of tree(); track n.path) {
          <app-tree-node [node]="n" [depth]="0" [openMap]="openMap()" (toggle)="toggle($event)" />
        }
      </div>
    </div>
  `,
})
export class FileTreeComponent {
  readonly agent = input.required<Agent>();
  readonly project = input<Project | undefined>(undefined);
  readonly openMap = signal<Record<string, boolean>>({});

  readonly tree = computed(() => {
    const ag = this.agent();
    const proj = this.project();
    const stateMap: Record<string, "A" | "M" | "D"> = {};
    (ag.files || []).forEach((f) => (stateMap[f.path] = f.state));
    const allPaths = Array.from(
      new Set([...(proj?.files ?? []), ...(ag.files || []).map((f) => f.path)]),
    );
    return buildTree(allPaths, stateMap);
  });

  toggle(p: string) {
    this.openMap.update((m) => ({ ...m, [p]: m[p] === false ? true : false }));
  }
}
