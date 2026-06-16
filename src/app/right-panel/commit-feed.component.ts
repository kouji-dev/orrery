import { ChangeDetectionStrategy, Component, inject, input } from "@angular/core";
import { Commit } from "../models";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { UiStore } from "../ui/ui.store";
import { STATUS_META } from "../utils";

@Component({
  selector: "app-commit-feed",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div style="position:relative;padding-left:var(--sp-9)">
      <div style="position:absolute;left:18px;top:8px;bottom:8px;width:1px;background:var(--hair)"></div>
      @for (c of commits(); track $index) {
        @let ag = agentFor(c.agent);
        <div
          class="commit-row"
          (click)="open(c.agent)"
          style="position:relative;padding:var(--sp-3) var(--sp-6) var(--sp-3) var(--sp-4);cursor:pointer;border-radius:var(--r-sm);margin:0 var(--sp-3)"
        >
          <span [style.border]="'2px solid ' + color(c.agent)" style="position:absolute;left:-12px;top:11px;width:9px;height:var(--sp-4);border-radius:50%;background:var(--panel)"></span>
          <div
            [title]="c.msg"
            style="font-size:var(--fs-sm);color:var(--ink);line-height:1.4;display:-webkit-box;-webkit-line-clamp:2;-webkit-box-orient:vertical;overflow:hidden"
          >{{ c.msg }}</div>
          <div class="tnum" style="display:flex;align-items:center;gap:var(--sp-3);margin-top:var(--sp-1);font-size:var(--fs-2xs);color:var(--ink-4)">
            @if (!compact()) { <span [style.color]="color(c.agent)">{{ ag ? ag.name : c.agent }}</span> }
            <span class="chip" style="font-size:var(--fs-2xs);padding:0 var(--sp-2)">{{ c.sha }}</span>
            <span>{{ c.files }} files</span>
            <span style="margin-left:auto">{{ c.when }} ago</span>
          </div>
        </div>
      }
    </div>
  `,
  styles: [`.commit-row:hover { background: var(--panel-2); }`],
})
export class CommitFeedComponent {
  private runtime = inject(AgentRuntimeService);
  private ui = inject(UiStore);
  readonly commits = input.required<Commit[]>();
  readonly compact = input<boolean>(false);

  agentFor(id: string) {
    return this.runtime.agents().find((a) => a.id === id);
  }
  color(id: string): string {
    const ag = this.agentFor(id);
    return ag ? STATUS_META[ag.status].color : "var(--ink-3)";
  }
  open(id: string) {
    if (this.agentFor(id)) this.ui.openAgent(id);
  }
}
