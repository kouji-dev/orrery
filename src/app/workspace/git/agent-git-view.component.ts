import { ChangeDetectionStrategy, Component, input, output } from "@angular/core";
import { Agent, GitView } from "../../models";
import { IconComponent } from "../../shared/icon.component";
import { CommitDiffViewComponent } from "./commit-diff-view.component";
import { RangeDiffViewComponent } from "./range-diff-view.component";
import { FileHistoryViewComponent } from "./file-history-view.component";
import { ConflictViewComponent } from "./conflict-view.component";
import { KjButtonComponent } from "@kouji-ui/components";

/**
 * Center "Diff tab" dispatcher: when a {@link GitView} is active for an agent
 * (the user picked a commit / range / file-history from the right-panel commit
 * history), this renders the matching read-only inspection view in place of the
 * working-tree diff. A back button clears the view (→ working changes).
 */
@Component({
  selector: "app-agent-git-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, CommitDiffViewComponent, RangeDiffViewComponent, FileHistoryViewComponent, ConflictViewComponent, KjButtonComponent],
  template: `
    @let ag = agent();
    @let gv = gitView();
    <div style="flex:1;display:flex;flex-direction:column;min-height:0">
      <div class="pane-head" style="gap:var(--sp-3);padding:var(--sp-2) var(--sp-4);background:var(--panel)">
        <kj-button kjVariant="outline" (click)="close.emit()" title="Back to working changes">
          <app-icon size="md" name="chevron" />Working changes
        </kj-button>
        <app-icon size="md" name="branch" color="var(--ink-3)" />
        <span class="trunc" style="color:var(--ink-4)">{{ ag.branch }}</span>
      </div>
      @switch (gv.kind) {
        @case ('commit') {
          <app-commit-diff-view [agent]="ag" [sha]="gv.sha" [initPath]="gv.path" />
        }
        @case ('range') {
          <app-range-diff-view [agent]="ag" [shas]="gv.shas" />
        }
        @case ('filehistory') {
          <app-file-history-view [agent]="ag" [path]="gv.path" />
        }
        @case ('conflict') {
          <app-conflict-view [agent]="ag" (close)="close.emit()" />
        }
      }
    </div>
  `,
})
export class AgentGitViewComponent {
  readonly agent = input.required<Agent>();
  readonly gitView = input.required<GitView>();
  readonly close = output<void>();
}
