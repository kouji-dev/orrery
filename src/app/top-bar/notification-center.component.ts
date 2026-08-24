import { ChangeDetectionStrategy, Component, computed, inject } from "@angular/core";
import { NotificationService } from "../notifications/notification.service";
import { NotificationCardComponent } from "../notifications/notification-card.component";
import { IconComponent } from "../shared/icon.component";
import { KjButtonComponent } from "@kouji-ui/components";
import { KjPopoverContent, KjPopoverTrigger } from "@kouji-ui/core";

/** Bell + dropdown feed of agent notifications (questions, permission, done).
 *  The panel is a kouji popover: the trigger/content pair brings outside-click
 *  dismiss, Escape, focus return, and body-portalled positioning for free —
 *  replacing the old hand-positioned absolute panel + fixed scrim. */
@Component({
  selector: "app-notification-center",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, NotificationCardComponent, KjButtonComponent, KjPopoverTrigger, KjPopoverContent],
  template: `
    <div style="position:relative">
      <kj-button kjVariant="outline" kjPopoverTrigger #np="kjPopoverTrigger" title="Notifications">
        <app-icon name="bell" size="sm" />
        @if (unread()) {
          <span
            class="tnum"
            style="position:absolute;top:-3px;right:-3px;min-width:var(--sp-7);height:var(--sp-7);padding:0 var(--sp-1);display:grid;place-items:center;font-size:var(--fs-meta);font-weight:var(--fw-medium);color:#fff;background:var(--st-blocked);border-radius:8px"
          >{{ unread() }}</span>
        }
      </kj-button>

      <kj-popover-content [kjFor]="np" kjSide="bottom" kjAlign="end" style="--kj-popover-padding-x:0;--kj-popover-padding-y:0">
        <div
          style="width: round(calc(344px * var(--density)), 1px);max-height:64vh;display:flex;flex-direction:column;background:var(--panel);border-radius:var(--r-md);overflow:hidden"
        >
          <div class="pane-head" style="justify-content:space-between">
            <span class="up" style="color:var(--ink-2)">Notifications</span>
            @if (items().length) {
              <kj-button kjVariant="outline" (click)="notifications.clearResolved()">Clear read</kj-button>
            }
          </div>

          <div class="scroll-y" style="flex:1">
            @for (n of items(); track n.id) {
              <app-notification-card [notification]="n" (navigate)="np.controller.close()" />
            } @empty {
              <div class="pane-empty pad">No notifications</div>
            }
          </div>
        </div>
      </kj-popover-content>
    </div>
  `,
})
export class NotificationCenterComponent {
  readonly notifications = inject(NotificationService);

  readonly unread = this.notifications.unread;
  readonly items = computed(() => this.notifications.all().slice(0, 40));
}
