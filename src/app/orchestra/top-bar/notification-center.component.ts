import { ChangeDetectionStrategy, Component, computed, inject, signal } from "@angular/core";
import { NotificationService } from "../notifications/notification.service";
import { NotificationCardComponent } from "../notifications/notification-card.component";
import { IconComponent } from "../shared/icon.component";

/** Bell + dropdown feed of agent notifications (questions, permission, done). */
@Component({
  selector: "app-notification-center",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, NotificationCardComponent],
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
              <button class="btn ghost-hair" style="padding:2px 7px;font-size:10px" (click)="notifications.clearResolved()">Clear read</button>
            }
          </div>

          <div class="scroll-y" style="flex:1;overflow-y:auto">
            @for (n of items(); track n.id) {
              <app-notification-card [notification]="n" (navigate)="open.set(false)" />
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
  readonly notifications = inject(NotificationService);
  readonly open = signal(false);

  readonly unread = this.notifications.unread;
  readonly items = computed(() => this.notifications.all().slice(0, 40));
}
