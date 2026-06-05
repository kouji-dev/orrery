import { ChangeDetectionStrategy, Component, inject, input, output } from "@angular/core";
import { AgentNotification, NotificationKind } from "../models";
import { NotificationService } from "./notification.service";
import { IconComponent } from "../shared/icon.component";

const KIND_META: Record<NotificationKind, { icon: string; color: string }> = {
  permission: { icon: "flag", color: "var(--st-blocked)" },
  question: { icon: "chat", color: "var(--accent-2)" },
  done: { icon: "check", color: "var(--st-done)" },
};

/** One agent notification + its per-kind actions. Shared by the bell dropdown
 *  and the right-panel inbox so both render the same real feed. */
@Component({
  selector: "app-notification-card",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    @let n = notification();
    @let pending = n.status === 'pending';
    <div [style.opacity]="pending ? 1 : 0.55" style="padding:11px 12px;border-bottom:1px solid var(--hair);display:flex;gap:9px">
      <app-icon [name]="meta().icon" size="sm" [color]="meta().color" style="flex:none;margin-top:1px" />
      <div style="flex:1;min-width:0">
        <div style="display:flex;align-items:center;gap:6px">
          <span style="font-size:12px;font-weight:600;color:var(--ink)">{{ n.title }}</span>
          <span class="tnum" style="margin-left:auto;font-size:9.5px;color:var(--ink-4)">{{ ago(n.createdAt) }}</span>
        </div>
        @if (n.detail) {
          <pre
            style="margin:5px 0 0;font-family:var(--font-mono);font-size:10.5px;line-height:1.45;color:var(--ink-3);white-space:pre-wrap;word-break:break-word;max-height:54px;overflow:hidden"
          >{{ n.detail }}</pre>
        }

        @if (pending) {
          <div style="display:flex;flex-wrap:wrap;gap:6px;margin-top:8px">
            @switch (n.kind) {
              @case ('permission') {
                <button class="btn primary" style="padding:4px 9px;font-size:11px" (click)="notifications.accept(n)"><app-icon name="check" size="sm" />Accept</button>
                <button class="btn ghost-hair" style="padding:4px 9px;font-size:11px" (click)="notifications.reject(n)"><app-icon name="x" size="sm" />Reject</button>
                <button class="btn ghost-hair" style="padding:4px 9px;font-size:11px" (click)="navigate.emit(); notifications.open(n)"><app-icon name="terminal" size="sm" />Terminal</button>
              }
              @case ('question') {
                <button class="btn primary" style="padding:4px 9px;font-size:11px" (click)="navigate.emit(); notifications.open(n)"><app-icon name="terminal" size="sm" />Answer in terminal</button>
                <button class="btn ghost-hair" style="padding:4px 9px;font-size:11px" (click)="notifications.dismiss(n)">Dismiss</button>
              }
              @case ('done') {
                <button class="btn primary" style="padding:4px 9px;font-size:11px" (click)="notifications.merge(n)"><app-icon name="merge" size="sm" />Merge</button>
                <button class="btn ghost-hair" style="padding:4px 9px;font-size:11px" (click)="navigate.emit(); notifications.review(n)"><app-icon name="diff" size="sm" />Review diff</button>
                <button class="btn ghost-hair" style="padding:4px 9px;font-size:11px" (click)="notifications.dismiss(n)">Dismiss</button>
              }
            }
          </div>
        } @else {
          <div class="up" style="margin-top:6px;font-size:9.5px;letter-spacing:0.06em;color:var(--ink-4)">
            {{ n.status === 'dismissed' ? 'dismissed' : n.decision || n.status }}
          </div>
        }
      </div>
    </div>
  `,
})
export class NotificationCardComponent {
  readonly notifications = inject(NotificationService);
  readonly notification = input.required<AgentNotification>();
  /** Emitted when an action navigates to a tab (so a host dropdown can close). */
  readonly navigate = output<void>();

  meta() {
    return KIND_META[this.notification().kind];
  }
  ago(ts: number): string {
    const s = Math.max(0, Math.round((Date.now() - ts) / 1000));
    if (s < 60) return s + "s";
    const m = Math.floor(s / 60);
    if (m < 60) return m + "m";
    return Math.floor(m / 60) + "h";
  }
}
