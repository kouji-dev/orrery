import { ChangeDetectionStrategy, Component, computed, inject, input, output } from "@angular/core";
import { AgentNotification } from "../models";
import { NotificationService } from "./notification.service";
import { QuestionStepperComponent } from "./question-stepper.component";
import { IconComponent } from "../shared/icon.component";
import { KjButtonComponent } from "@kouji-ui/components";

// The adaptive presentation chosen for a card from its content + kind. The icon
// and accent color are picked per-layout so each notification reads with the
// best context (a "?" for questions, a shield for raw commands, a check when
// done, a bell otherwise).
type Layout = "question" | "command" | "done" | "info";
interface Presentation {
  layout: Layout;
  icon: string;
  color: string;
}

/** One agent notification + its per-kind actions. Shared by the bell dropdown
 *  and the right-panel inbox so both render the same real feed.
 *
 *  Rendering is ADAPTIVE — the layout is derived from the notification's content
 *  (not just its kind) so each gives the best context:
 *    • questions present        → QUESTION layout ("?" icon, question text,
 *                                 options as label chips with desc-on-hover)
 *    • permission with command  → COMMAND layout (shield icon, mono command,
 *                                 mode label, suggested-rule chips)
 *    • kind 'done'              → check icon + summary/detail
 *    • else                     → bell/info icon + detail
 *  Options/chips are DISPLAY-ONLY — returning the chosen option / persisting a
 *  rule is DEFERRED (Claude resolves the prompt in its own TUI). */
@Component({
  selector: "app-notification-card",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, QuestionStepperComponent, KjButtonComponent],
  template: `
    @let n = notification();
    @let p = presentation();
    @let pending = n.status === 'pending';
    <div [style.opacity]="pending ? 1 : 0.55" style="padding:var(--sp-5) var(--sp-6);border-bottom:1px solid var(--hair);display:flex;gap:var(--sp-4)">
      <app-icon [name]="p.icon" size="sm" [color]="p.color" style="flex:none;margin-top:1px" />
      <div style="flex:1;min-width:0">
        <!-- body: click anywhere on the text region to open the agent terminal -->
        <div style="cursor:pointer" title="Open agent terminal" (click)="openTerminal(n)">
          <div style="display:flex;align-items:center;gap:var(--sp-3)">
            <span style="font-weight:var(--fw-medium);color:var(--ink)">{{ n.title }}</span>
            <span class="tnum" style="margin-left:auto;font-size:var(--fs-meta);color:var(--ink-4)">{{ ago(n.createdAt) }}</span>
          </div>

          @switch (p.layout) {
            @case ('question') {
              <!-- QUESTION layout: the multi-step stepper is shown DIRECTLY (no
                   "Answer" gate) — one question at a time, single/multi-select +
                   an "Other" auto-growing textarea, with Back/Next/Submit. The
                   stepper shows the question text + progress itself. -->
              <app-question-stepper [notification]="n" (navigate)="navigate.emit()" />
            }
            @case ('command') {
              <!-- COMMAND layout: the concrete command in a mono block, the
                   permission mode, then suggested-rule chips (allow=accent /
                   deny=blocked). Display-only — persisting a rule is deferred. -->
              @if (n.mode) {
                <span class="up" style="display:block;margin-top:var(--sp-3);color:var(--ink-4)">{{ n.mode }}</span>
              }
              @let body = n.command || n.description || n.filePath;
              @if (body) {
                <pre
                  style="margin:var(--sp-2) 0 0;line-height:1.45;color:var(--ink-3);white-space:pre-wrap;word-break:break-word;max-height: round(calc(72px * var(--density)), 1px);overflow:hidden"
                >{{ body }}</pre>
              }
              @if (n.suggestions && n.suggestions.length) {
                <div class="up" style="margin:var(--sp-4) 0 var(--sp-2);color:var(--ink-4)">suggested rules</div>
                <div style="display:flex;flex-wrap:wrap;gap:var(--sp-2)">
                  @for (s of n.suggestions; track s.rule) {
                    <span
                      [title]="s.description"
                      [style.color]="s.behavior === 'deny' ? 'var(--sem-del)' : 'var(--ui-ink)'"
                      [style.border-color]="s.behavior === 'deny' ? 'var(--sem-del)' : 'var(--ui-sel-2)'"
                      style="display:inline-flex;align-items:center;gap:var(--sp-2);font-family:var(--font-mono);font-size:var(--fs-meta);padding:var(--sp-1) var(--sp-3);border-radius:5px;border:1px solid;background:var(--panel)"
                    >
                      <span class="up" style="opacity:0.8">{{ s.behavior }}</span>
                      <span>{{ s.rule }}</span>
                    </span>
                  }
                </div>
              }
            }
            @default {
              <!-- DONE / INFO layout: the summary/detail in a compact mono block. -->
              @let body = n.summary || n.detail;
              @if (body) {
                <pre
                  style="margin:var(--sp-2) 0 0;line-height:1.45;color:var(--ink-3);white-space:pre-wrap;word-break:break-word;max-height: round(calc(54px * var(--density)), 1px);overflow:hidden"
                >{{ body }}</pre>
              }
            }
          }
        </div>

        <!-- stop button clicks from bubbling to the body terminal navigation.
             Question-bearing notifications render the stepper's OWN nav
             (Back/Next/Submit + Terminal + Reject), so the card's button row is
             suppressed for any pending notification with questions. -->
        @if (pending && !n.questions?.length) {
          <div style="display:flex;flex-wrap:wrap;gap:var(--sp-3);margin-top:var(--sp-4)" (click)="$event.stopPropagation()">
            @switch (n.kind) {
              @case ('permission') {
                <kj-button kjVariant="default" (click)="notifications.accept(n)"><app-icon name="check" size="sm" />Accept</kj-button>
                <kj-button kjVariant="outline" (click)="notifications.reject(n)"><app-icon name="x" size="sm" />Reject</kj-button>
                <kj-button kjVariant="outline" (click)="navigate.emit(); notifications.open(n)"><app-icon name="terminal" size="sm" />Terminal</kj-button>
              }
              @case ('question') {
                <!-- question with NO options to render → primary Terminal answer +
                     Dismiss. (Option-bearing questions show the stepper inline.) -->
                <kj-button kjVariant="default" (click)="navigate.emit(); notifications.open(n)"><app-icon name="terminal" size="sm" />Answer in terminal</kj-button>
                <kj-button kjVariant="outline" (click)="notifications.dismiss(n)">Dismiss</kj-button>
              }
              @case ('done') {
                <kj-button kjVariant="default" (click)="notifications.push(n)"><app-icon name="push" size="sm" />Push</kj-button>
                <kj-button kjVariant="outline" (click)="navigate.emit(); notifications.review(n)"><app-icon name="diff" size="sm" />Review diff</kj-button>
                <kj-button kjVariant="outline" (click)="notifications.dismiss(n)">Dismiss</kj-button>
              }
            }
          </div>
        } @else if (!pending) {
          <div class="up" style="margin-top:var(--sp-3);color:var(--ink-4)">
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

  /** Adaptive presentation — derived from the notification's content first, then
   *  its kind. Questions win over a raw command; a permission with a command but
   *  no questions gets the shield/command layout; 'done' gets a check; everything
   *  else falls back to the bell/info layout. */
  readonly presentation = computed<Presentation>(() => {
    const n = this.notification();
    if (n.questions?.length) {
      return { layout: "question", icon: "question", color: "var(--sem-attn)" };
    }
    if (n.kind === "permission" && (n.command || n.description || n.filePath)) {
      return { layout: "command", icon: "shield", color: "var(--sem-attn)" };
    }
    if (n.kind === "done") {
      return { layout: "done", icon: "check", color: "var(--st-done)" };
    }
    return { layout: "info", icon: "bell", color: "var(--ink-2)" };
  });

  /** Body click: open the agent's terminal and let a host (dropdown) close.
   *  Never removes the notification — history stays. */
  openTerminal(n: AgentNotification) {
    this.navigate.emit();
    this.notifications.open(n);
  }

  ago(ts: number): string {
    const s = Math.max(0, Math.round((Date.now() - ts) / 1000));
    if (s < 60) return s + "s";
    const m = Math.floor(s / 60);
    if (m < 60) return m + "m";
    return Math.floor(m / 60) + "h";
  }
}
