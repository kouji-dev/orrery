import { ChangeDetectionStrategy, Component, computed, inject, input } from "@angular/core";
import { OrchestraStore } from "../orchestra.store";
import { logColor, logPrefix } from "../utils";

/** Last 3 log lines for an agent — shown on overview cards. */
@Component({
  selector: "app-mini-term",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div style="background:var(--bg);border:1px solid var(--hair);border-radius:var(--r-sm);padding:6px 8px;font-size:10px;line-height:1.6;overflow:hidden">
      @if (lines().length) {
        @for (l of lines(); track $index) {
          <div [style.color]="lc(l.t)" style="white-space:nowrap;overflow:hidden;text-overflow:ellipsis">
            <span style="color:var(--ink-4);margin-right:5px">{{ lp(l.t) }}</span>{{ l.s }}
          </div>
        }
      } @else {
        <span style="color:var(--ink-4)">no output yet</span>
      }
    </div>
  `,
})
export class MiniTermComponent {
  private store = inject(OrchestraStore);
  readonly agentId = input.required<string>();
  readonly lc = logColor;
  readonly lp = logPrefix;
  readonly lines = computed(() => (this.store.liveLogs()[this.agentId()] || []).slice(-3));
}
