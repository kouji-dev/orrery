import { ChangeDetectionStrategy, Component, input, output } from "@angular/core";
import { Agent, GitView } from "../../models";
import { IconComponent } from "../../shared/icon.component";
import { CommitDiffViewComponent } from "./commit-diff-view.component";
import { RangeDiffViewComponent } from "./range-diff-view.component";
import { FileHistoryViewComponent } from "./file-history-view.component";

/**
 * Center "Diff tab" dispatcher: when a {@link GitView} is active for an agent
 * (the user picked a commit / range / file-history from the right-panel commit
 * history), this renders the matching read-only inspection view in place of the
 * working-tree diff. A back button clears the view (→ working changes).
 */
@Component({
  selector: "app-agent-git-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, CommitDiffViewComponent, RangeDiffViewComponent, FileHistoryViewComponent],
  template: `
    @let ag = agent();
    @let gv = gitView();
    <div style="flex:1;display:flex;flex-direction:column;min-height:0">
      <div
        style="flex:none;display:flex;align-items:center;gap:var(--sp-3);padding:var(--sp-2) var(--sp-4);border-bottom:1px solid var(--hair);background:var(--panel)"
      >
        <button class="btn ghost-hair" (click)="close.emit()" title="Back to working changes" style="padding:var(--sp-1) var(--sp-3)">
          <app-icon name="chevron" size="sm" [px]="12" />Working changes
        </button>
        <app-icon name="branch" size="sm" color="var(--accent-2)" [px]="12" />
        <span style="font-size:var(--fs-xs);color:var(--ink-4);overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ ag.branch }}</span>
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
      }
    </div>
  `,
  styles: [
    `
      /* Fill the pane body (a flex column). Without an explicit :host flex rule
         the host collapses to content height, so the view only looked full when
         a file's content was tall enough to stretch it. */
      :host {
        display: flex;
        flex-direction: column;
        flex: 1;
        min-height: 0;
        min-width: 0;
      }
    `,
  ],
})
export class AgentGitViewComponent {
  readonly agent = input.required<Agent>();
  readonly gitView = input.required<GitView>();
  readonly close = output<void>();
}
