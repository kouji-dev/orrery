import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
  input,
  output,
  signal,
} from "@angular/core";
import { AgentNotification, PermissionQuestion } from "../models";
import { AgentsStore } from "../stores/agents.store";
import { NotificationStore } from "../stores/notifications.store";
import { UiStore } from "../ui/ui.store";
import { IconComponent } from "../shared/icon.component";
import { KjBadgeComponent, KjButtonComponent, KjCheckboxComponent, KjRadioComponent, KjRadioGroupComponent, KjTextareaComponent } from "@kouji-ui/components";

/**
 * A multi-step question answerer for an AskUserQuestion-style permission prompt,
 * rendered INLINE in the notification card. Walks the `questions` array one step
 * at a time (a stepper), collects the answers locally, then — on Submit — sends
 * them back to the agent as PACED PTY keystrokes (one question's keys, a short
 * delay so the TUI advances, then the next).
 *
 * ── RELIABILITY TIERS (honest; see HARD CONSTRAINT in the runtime) ───────────
 * Orrery drives the REAL claude CLI over a PTY (no Agent SDK). Answers go in as
 * raw keystrokes the TUI interprets, so delivery is NOT guaranteed:
 *
 *   • single-select via digit  = RELIABLE. Typing the option's number instantly
 *     selects + submits + advances. We send `decide(id, i+1)` → "{i+1}\r".
 *   • "Other" free text        = FRAGILE. We select Other (its number =
 *     concreteOptions.length + 1), then type the text + Enter. There is a known
 *     digit-in-textarea bug in claude's TUI, so a digit at the START can mis-fire.
 *   • multi-select (space-toggle) = OFFICIALLY BUGGY / unreliable. We send a
 *     BEST-EFFORT toggle sequence (navigate + space per selection, then Enter),
 *     but treat it as may-not-take. The Terminal button is the real fallback.
 *
 * The card NEVER implies guaranteed delivery — every step keeps a prominent
 * Terminal button, and multi-select shows an extra "use Terminal if this doesn't
 * take" hint. After sending, the notification is marked accepted ("answered").
 */
@Component({
  selector: "app-question-stepper",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, KjBadgeComponent, KjButtonComponent, KjCheckboxComponent, KjRadioComponent, KjRadioGroupComponent, KjTextareaComponent],
  styles: [
    `
      /* the option chip is the shared .opt / .opt:hover / .opt.sel recipe */
      .nav {
        display: flex;
        align-items: center;
        gap: var(--sp-3);
        margin-top: var(--sp-5);
        flex-wrap: wrap;
      }
      .hint {
        margin-top: var(--sp-3);
        font-size: var(--fs-meta);
        line-height: 1.4;
        color: var(--ink-4);
      }
    `,
  ],
  template: `
    @let qs = questions();
    @let i = step();
    @let q = qs[i];
    @if (q) {
      <div (click)="$event.stopPropagation()" style="margin-top:var(--sp-4)">
        <!-- progress + header -->
        <div style="display:flex;align-items:center;gap:var(--sp-3);flex-wrap:wrap">
          <span class="up tnum" style="color:var(--ink-4)"
            >Question {{ i + 1 }} of {{ qs.length }}</span
          >
          @if (q.header) {
            <span class="up" style="color:var(--ui-ink)">{{ q.header }}</span>
          }
          @if (q.multiSelect) {
            <kj-badge class="up" bg="var(--ui-sel)" fg="var(--ui-ink)"
              style="--kj-badge-border-color:var(--ui-sel-2);--kj-badge-"
              title="Multi-select is experimental: the claude TUI's space-toggle is unreliable. Use Terminal if it doesn't take."
              >experimental</kj-badge>
          }
        </div>

        <!-- the question text -->
        <div style="line-height:1.45;color:var(--ink-2);margin-top:var(--sp-2)">{{ q.question }}</div>

        <!-- options (radio for single, checkbox for multi) + the synthesized
             "Other" free-text choice claude auto-appends -->
        <!-- the option rows own the interaction; the kj-radio / kj-checkbox
             marks are presentational mirrors (pointer-events off), with the
             radio group carrying the current single-select value -->
        <kj-radio-group
          [value]="q.multiSelect === true ? undefined : singleSel(i)"
          orientation="vertical"
          ariaLabel="Answer options"
          style="display:flex;flex-direction:column;gap:var(--sp-2);margin-top:var(--sp-3)"
        >
          @for (opt of q.options; track $index) {
            <button kjButton
              type="button"
              class="opt"
              [class.sel]="isSelected(i, $index)"
              [title]="opt.description || opt.label"
              (click)="toggle(i, $index, q.multiSelect === true)"
            >
              @if (q.multiSelect === true) {
                <kj-checkbox size="sm" [checked]="isSelected(i, $index)" style="pointer-events:none;flex:none;margin-top:1px" />
              } @else {
                <kj-radio [value]="$index" style="pointer-events:none;flex:none;margin-top:1px" />
              }
              <span style="min-width:0">{{ opt.label }}</span>
            </button>
          }

          <!-- "Other" — always available (claude auto-appends a free-text choice
               AFTER the concrete options). Selecting it reveals the auto-growing
               textarea. -->
          <button kjButton
            type="button"
            class="opt"
            [class.sel]="isOther(i)"
            title="Type a custom answer (claude's auto-appended free-text choice)"
            (click)="chooseOther(i)"
          >
            @if (q.multiSelect === true) {
              <kj-checkbox size="sm" [checked]="isOther(i)" style="pointer-events:none;flex:none;margin-top:1px" />
            } @else {
              <kj-radio value="other" style="pointer-events:none;flex:none;margin-top:1px" />
            }
            <span>Other…</span>
          </button>
          @if (isOther(i)) {
            <kj-textarea
              kjAutoresize="auto"
              [kjMinRows]="1"
              [kjMaxRows]="6"
              kjPlaceholder="Type your answer…"
              [kjValue]="otherText(i)"
              (input)="onOtherInput(i, $event)"
              (click)="$event.stopPropagation()"
              style="--kj-textarea-font:var(--font-ui);--kj-textarea---kj-textarea-padding-x:var(--sp-4);--kj-textarea-border-color:var(--ui-focus)"
            />
          }
        </kj-radio-group>

        @if (q.multiSelect) {
          <div class="hint">
            Multi-select is best-effort — the claude TUI's space-toggle is buggy. If it doesn't take, answer in the Terminal.
          </div>
        }

        <!-- stepper nav + actions -->
        <div class="nav">
          @if (i > 0) {
            <kj-button kjVariant="outline" (click)="back()">
              <app-icon name="chevron" size="sm" style="transform:rotate(180deg)" />Back
            </kj-button>
          }
          @if (i < qs.length - 1) {
            <kj-button kjVariant="outline" (click)="next()">
              Next<app-icon name="chevron" size="sm" />
            </kj-button>
          } @else {
            <kj-button kjVariant="default" [kjDisabled]="sending()" (click)="submit()">
              <app-icon name="check" size="sm" />{{ sending() ? "Sending…" : "Submit" }}
            </kj-button>
          }
          <!-- Terminal: the RELIABLE fallback for every question -->
          <kj-button kjVariant="outline" (click)="terminal()">
            <app-icon name="terminal" size="sm" />Terminal
          </kj-button>
          <!-- Reject: Esc cancels the numbered select -->
          <kj-button kjVariant="outline" (click)="reject()">
            <app-icon name="x" size="sm" />Reject
          </kj-button>
        </div>
      </div>
    }
  `,
})
export class QuestionStepperComponent {
  private agentsStore = inject(AgentsStore);
  private notificationsStore = inject(NotificationStore);
  private ui = inject(UiStore);

  readonly notification = input.required<AgentNotification>();
  /** Emitted when an action should navigate to the agent tab (host dropdown closes). */
  readonly navigate = output<void>();

  readonly questions = computed<PermissionQuestion[]>(() => this.notification().questions ?? []);

  readonly step = signal(0);
  readonly sending = signal(false);

  // Per-question answer state, keyed by question index:
  //  • selected: the chosen CONCRETE option indices (0-based). Single-select keeps
  //    at most one; multi-select keeps a set.
  //  • other:    when the user picked the synthesized "Other" choice (free text).
  //  • otherText: the typed free-text answer for that question.
  private readonly selected = signal<Record<number, Set<number>>>({});
  private readonly other = signal<Record<number, boolean>>({});
  private readonly otherTexts = signal<Record<number, string>>({});

  // ~120ms between questions so the TUI has time to advance to the next prompt.
  private static readonly STEP_DELAY = 120;

  isSelected(q: number, opt: number): boolean {
    return this.selected()[q]?.has(opt) ?? false;
  }
  /** Single-select value for the presentational radio group: the chosen
   *  concrete option index, `"other"` for the free-text choice, else nothing. */
  singleSel(q: number): number | "other" | undefined {
    if (this.isOther(q)) return "other";
    const set = this.selected()[q];
    if (!set?.size) return undefined;
    return [...set][0];
  }
  isOther(q: number): boolean {
    return this.other()[q] ?? false;
  }
  otherText(q: number): string {
    return this.otherTexts()[q] ?? "";
  }

  /** Pick / unpick a concrete option. Single-select replaces; multi-select toggles.
   *  Picking any concrete option clears the "Other" selection for that question
   *  (and vice-versa) for single-select — for multi-select, Other stays mutually
   *  exclusive too, to keep the answer unambiguous to send. */
  toggle(q: number, opt: number, multi: boolean) {
    this.other.update((m) => ({ ...m, [q]: false }));
    this.selected.update((m) => {
      const set = new Set(m[q] ?? []);
      if (multi) {
        if (set.has(opt)) set.delete(opt);
        else set.add(opt);
      } else {
        set.clear();
        set.add(opt);
      }
      return { ...m, [q]: set };
    });
  }

  /** Select the synthesized "Other" free-text choice; reveals the textarea and
   *  clears concrete selections for this question. */
  chooseOther(q: number) {
    this.selected.update((m) => ({ ...m, [q]: new Set<number>() }));
    this.other.update((m) => ({ ...m, [q]: true }));
  }

  /** Store the typed text — kj-textarea's kjAutoresize handles the auto-grow
   *  (min 1 row, max 6) that used to be hand-rolled here. */
  onOtherInput(q: number, ev: Event) {
    this.otherTexts.update((m) => ({ ...m, [q]: (ev.target as HTMLTextAreaElement).value }));
  }

  back() {
    this.step.update((s) => Math.max(0, s - 1));
  }
  next() {
    this.step.update((s) => Math.min(this.questions().length - 1, s + 1));
  }

  /**
   * Submit: send every question's answer in order as PACED PTY keystrokes. Each
   * question is sent, then we await ~120ms so the TUI advances to the next prompt
   * before sending the next answer.
   *
   * Per-type sequence (see reliability tiers in the class doc):
   *  • single-select concrete option i (0-based) → decide(id, i+1)  [RELIABLE].
   *  • "Other" free text → decide(id, concreteOptions.length + 1) then
   *    input(id, text + "\r")  [FRAGILE: digit-in-textarea bug].
   *  • multi-select set → BEST-EFFORT toggle sequence then Enter  [UNRELIABLE].
   */
  async submit() {
    if (this.sending()) return;
    this.sending.set(true);
    const n = this.notification();
    const id = n.agentId;
    const qs = this.questions();

    try {
      for (let qi = 0; qi < qs.length; qi++) {
        const q = qs[qi];
        const concrete = q.options?.length ?? 0;

        if (this.isOther(qi)) {
          // ── "Other" free text (FRAGILE) ─────────────────────────────────
          // Select the auto-appended Other choice (its number is AFTER the
          // concrete options), then type the answer + Enter. Known issue: a
          // leading digit can mis-fire in claude's textarea.
          await this.agentsStore.decide(id, concrete + 1).catch(() => {});
          await this.delay(QuestionStepperComponent.STEP_DELAY);
          await this.agentsStore.input(id, this.otherText(qi) + "\r").catch(() => {});
        } else if (q.multiSelect) {
          // ── multi-select (UNRELIABLE / OFFICIALLY BUGGY) ────────────────
          // Best-effort: for each selected option, send its 1-based number to
          // move to it then a space to toggle it (claude's space-toggle), and a
          // final Enter to confirm. This may NOT take — the Terminal button is
          // the real path, surfaced in the hint above.
          const picks = [...(this.selected()[qi] ?? [])].sort((a, b) => a - b);
          for (const opt of picks) {
            await this.agentsStore.input(id, `${opt + 1} `).catch(() => {});
            await this.delay(40);
          }
          await this.agentsStore.input(id, "\r").catch(() => {});
        } else {
          // ── single-select via digit (RELIABLE) ──────────────────────────
          // Typing the option's number instantly selects + submits + advances.
          const pick = [...(this.selected()[qi] ?? [])][0];
          if (pick != null) await this.agentsStore.decide(id, pick + 1).catch(() => {});
        }

        // Pace: let the TUI advance to the next question before the next answer.
        if (qi < qs.length - 1) await this.delay(QuestionStepperComponent.STEP_DELAY);
      }

      // Mark answered + flash. Not a guaranteed delivery — best-effort keystrokes.
      this.notificationsStore.decide(n.id, "accepted", "answered");
      this.ui.flash("answered · " + n.agentName);
      this.navigate.emit();
    } finally {
      this.sending.set(false);
    }
  }

  /** Reliable fallback: open the agent terminal to answer there. */
  terminal() {
    this.navigate.emit();
    this.ui.openAgent(this.notification().agentId, "terminal");
  }

  /** Reject: Esc cancels the numbered select in the agent's TUI. */
  reject() {
    const n = this.notification();
    void this.agentsStore.deny(n.agentId).catch(() => {});
    this.notificationsStore.decide(n.id, "rejected", "rejected");
    this.ui.flash("rejected · " + n.agentName);
  }

  private delay(ms: number): Promise<void> {
    return new Promise((r) => setTimeout(r, ms));
  }
}
