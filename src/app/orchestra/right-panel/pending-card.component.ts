import { ChangeDetectionStrategy, Component, computed, input, output } from "@angular/core";
import { Agent, PendingItem } from "../models";
import { IconComponent } from "../shared/icon.component";
import { mix } from "../utils";

interface KindMeta {
  icon: string;
  color: string;
  verb: string;
}
const KIND_META: Record<string, KindMeta> = {
  permission: { icon: "bolt", color: "#f5c451", verb: "wants to run" },
  decision: { icon: "flag", color: "var(--st-blocked)", verb: "needs a decision" },
  review: { icon: "merge", color: "var(--st-done)", verb: "ready for review" },
};

@Component({
  selector: "app-pending-card",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    @let it = item();
    <div
      class="rise"
      [style.border]="'1px solid ' + mix(meta().color, 70)"
      style="margin:8px 10px;padding:11px;border-radius:var(--r-md);background:var(--panel-2)"
    >
      <div style="display:flex;align-items:center;gap:7px;margin-bottom:7px">
        <app-icon [name]="meta().icon" size="sm" [color]="meta().color" style="flex:none" />
        <span style="font-size:11.5px;font-weight:600;color:var(--ink)">{{ it.title }}</span>
        <span style="margin-left:auto;font-size:9.5px;color:var(--ink-4)">{{ it.when }}</span>
      </div>
      <div style="font-size:10.5px;color:var(--ink-3);margin-bottom:9px">
        <span [style.color]="meta().color">{{ agent().name }}</span> {{ meta().verb }}
      </div>
      <div style="font-family:var(--font-mono);font-size:11px;color:var(--ink-2);padding:7px 9px;background:var(--bg);border:1px solid var(--hair);border-radius:var(--r-sm);margin-bottom:9px;word-break:break-all">
        {{ it.kind === 'permission' ? '$ ' : '' }}{{ it.cmd }}
      </div>
      <div style="display:flex;gap:7px">
        @switch (it.kind) {
          @case ('permission') {
            <button class="btn primary" style="flex:1;justify-content:center" (click)="resolve.emit('allow')"><app-icon name="check" size="sm" />Allow</button>
            <button class="btn ghost-hair" style="flex:1;justify-content:center" (click)="resolve.emit('deny')"><app-icon name="x" size="sm" />Deny</button>
            <button class="btn ghost-hair" (click)="resolve.emit('always')" title="Always allow this command">∞</button>
          }
          @case ('decision') {
            <button class="btn primary" style="flex:1;justify-content:center" (click)="resolve.emit('open')"><app-icon name="terminal" size="sm" />Answer in terminal</button>
          }
          @case ('review') {
            <button class="btn primary" style="flex:1;justify-content:center" (click)="resolve.emit('merge')"><app-icon name="merge" size="sm" />Merge</button>
            <button class="btn ghost-hair" style="flex:1;justify-content:center" (click)="resolve.emit('diff')"><app-icon name="diff" size="sm" />Review diff</button>
          }
        }
      </div>
    </div>
  `,
})
export class PendingCardComponent {
  readonly agent = input.required<Agent>();
  readonly item = input.required<PendingItem>();
  readonly resolve = output<string>();

  readonly mix = mix;
  readonly meta = computed(() => KIND_META[this.item().kind] || KIND_META["permission"]);
}
