import { ChangeDetectionStrategy, Component, computed, inject, signal } from "@angular/core";
import { AgentNotification, NotificationKind } from "../models";
import { OrchestraStore } from "../orchestra.store";
import { IconComponent } from "../shared/icon.component";

const KIND_META: Record<NotificationKind, { icon: string; color: string }> = {
  permission: { icon: "flag", color: "var(--st-blocked)" },
  question: { icon: "chat", color: "var(--accent-2)" },
  done: { icon: "check", color: "var(--st-done)" },
};

/** Bell + dropdown feed of agent notifications (questions, permission, done). */
@Component({
  selector: "app-notification-center",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    <div style="position:relative">
      <button class="btn ghost-hair" (click)="open.set(!open())" title="Notifications" style="padding:5px 8px;position:relative">
        <app-icon name="bell" size="sm" />
        @if (unread()) {
          <span
            class="tnum"
            style="position:absolute;top:-3px;right:-3px;min-width:15px;height:15px;padding:0 3px;display:grid;place-items:center;font-size:9px;font-weight:600;color:#fff;background:var(--st-blocked);border-radius:8px"
          >{{ unread() }}</span>
        }
      </button>

      @if (open()) {
        <div (click)="open.set(false)" style="position:fixed;inset:0;z-index:40"></div>
        <div
          style="position:absolute;right:0;top:34px;width:344px;max-height:64vh;display:flex;flex-direction:column;background:var(--panel);border:1px solid var(--hair-2);border-radius:var(--r-md);box-shadow:0 12px 34px rgba(0,0,0,0.4);z-index:50;overflow:hidden"
        >
          <div style="display:flex;align-items:center;justify-content:space-between;padding:10px 12px;border-bottom:1px solid var(--hair)">
            <span class="up" style="font-size:10px;letter-spacing:0.08em;color:var(--ink-2)">Notifications</span>
            @if (items().length) {
              <button class="btn ghost-hair" style="padding:2px 7px;font-size:10px" (click)="store.notifications.clearResolved()">Clear read</button>
            }
          </div>

          <div class="scroll-y" style="flex:1;overflow-y:auto">
            @for (n of items(); track n.id) {
              @let pending = n.status === 'pending';
              <div [style.opacity]="pending ? 1 : 0.55" style="padding:11px 12px;border-bottom:1px solid var(--hair);display:flex;gap:9px">
                <app-icon [name]="meta(n).icon" size="sm" [color]="meta(n).color" style="flex:none;margin-top:1px" />
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
                          <button class="btn primary" style="padding:4px 9px;font-size:11px" (click)="store.acceptNotification(n)"><app-icon name="check" size="sm" />Accept</button>
                          <button class="btn ghost-hair" style="padding:4px 9px;font-size:11px" (click)="store.rejectNotification(n)"><app-icon name="x" size="sm" />Reject</button>
                          <button class="btn ghost-hair" style="padding:4px 9px;font-size:11px" (click)="open.set(false); store.openNotification(n)"><app-icon name="terminal" size="sm" />Terminal</button>
                        }
                        @case ('question') {
                          <button class="btn primary" style="padding:4px 9px;font-size:11px" (click)="open.set(false); store.openNotification(n)"><app-icon name="terminal" size="sm" />Answer in terminal</button>
                          <button class="btn ghost-hair" style="padding:4px 9px;font-size:11px" (click)="store.dismissNotification(n)">Dismiss</button>
                        }
                        @case ('done') {
                          <button class="btn primary" style="padding:4px 9px;font-size:11px" (click)="store.mergeNotification(n)"><app-icon name="merge" size="sm" />Merge</button>
                          <button class="btn ghost-hair" style="padding:4px 9px;font-size:11px" (click)="open.set(false); store.reviewNotification(n)"><app-icon name="diff" size="sm" />Review diff</button>
                          <button class="btn ghost-hair" style="padding:4px 9px;font-size:11px" (click)="store.dismissNotification(n)">Dismiss</button>
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
            } @empty {
              <div style="padding:26px 12px;text-align:center;font-size:11.5px;color:var(--ink-4)">No notifications</div>
            }
          </div>
        </div>
      }
    </div>
  `,
})
export class NotificationCenterComponent {
  readonly store = inject(OrchestraStore);
  readonly open = signal(false);

  readonly unread = this.store.notifications.unread;
  readonly items = computed(() => this.store.notifications.all().slice(0, 40));

  meta(n: AgentNotification) {
    return KIND_META[n.kind];
  }

  ago(ts: number): string {
    const s = Math.max(0, Math.round((Date.now() - ts) / 1000));
    if (s < 60) return s + "s";
    const m = Math.floor(s / 60);
    if (m < 60) return m + "m";
    return Math.floor(m / 60) + "h";
  }
}
