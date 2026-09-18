import { ChangeDetectionStrategy, Component, inject, input, output } from "@angular/core";
import { Agent, FileDiff } from "../../models";
import { DIFF_LIST_MAX, DIFF_LIST_MIN, UiStore } from "../../ui/ui.store";
import { PaneResizerComponent } from "../../shared/pane-resizer.component";
import { DiffFileListComponent } from "./diff-file-list.component";
import { DiffOrBlameComponent } from "./diff-or-blame.component";
import { DiffEntry } from "./diff-tree";

/**
 * The body every git-inspection view shares: the changed-file list, the drag
 * handle, and the per-file diff/blame pane. The two callers (a single commit,
 * a multi-commit compare) differ only in what they put in the header and which
 * revisions the diff is taken between — so the body is one component, on the
 * same resizable column as the working-tree diff.
 */
@Component({
  selector: "app-diff-split",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [DiffFileListComponent, PaneResizerComponent, DiffOrBlameComponent],
  template: `
    <div class="diff-grid" [style.grid-template-columns]="listW() + 'px 6px 1fr'">
      <app-diff-file-list
        style="min-height:0;min-width:0"
        [agent]="agent()"
        [files]="files()"
        [selPath]="selPath()"
        [title]="title()"
        emptyLabel="no files"
        (select)="select.emit($event)"
      />

      <app-pane-resizer
        [width]="listW()"
        [min]="LIST_MIN"
        [max]="LIST_MAX"
        (widthChange)="ui.diffListWidth.set($event)"
        (reset)="ui.diffListWidth.set(null)"
      />

      @if (selPath(); as path) {
        <app-diff-or-blame
          [agent]="agent().id"
          [path]="path"
          [diff]="diff()"
          [add]="add()"
          [del]="del()"
          [oldRev]="oldRev()"
          [newRev]="newRev()"
        />
      } @else {
        <div class="pane-empty" style="background:var(--bg)">
          {{ loading() ? 'loading…' : 'select a file' }}
        </div>
      }
    </div>
  `,
})
export class DiffSplitComponent {
  readonly ui = inject(UiStore);
  readonly LIST_MIN = DIFF_LIST_MIN;
  readonly LIST_MAX = DIFF_LIST_MAX;

  readonly agent = input.required<Agent>();
  readonly files = input<readonly DiffEntry[]>([]);
  readonly title = input("");
  readonly selPath = input<string | null>(null);
  readonly diff = input<FileDiff | null>(null);
  readonly add = input<number | null>(null);
  readonly del = input<number | null>(null);
  readonly oldRev = input<string | null>(null);
  readonly newRev = input<string | null>(null);
  readonly loading = input(false);

  readonly select = output<string>();

  readonly listW = this.ui.diffListW;
}
