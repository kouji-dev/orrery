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
  imports: [IconComponent],
  styles: [
    `
      .opt {
        display: flex;
        align-items: flex-start;
        gap: var(--sp-3);
        width: 100%;
        font-family: var(--font-mono);
        font-size: var(--fs-xs);
        line-height: 1.4;
        padding: var(--sp-3) var(--sp-4);
        border-radius: var(--r-sm, 5px);
        border: 1px solid var(--hair);
        background: var(--panel);
        color: var(--ink-3);
        cursor: pointer;
        text-align: left;
        transition:
          border-color 0.12s ease,
          color 0.12s ease,
          background 0.12s ease;
      }
      .opt:hover {
        border-color: var(--accent);
        color: var(--ink-2);
        background: color-mix(in oklch, var(--accent), transparent 90%);
      }
      .opt.sel {
        border-color: var(--accent);
        color: var(--ink);
        background: color-mix(in oklch, var(--accent), transparent 84%);
      }
      /* the radio/checkbox mark — square for multi, round for single */
      .mark {
        flex: none;
        width: 13px;
        height: var(--sp-6);
        margin-top: 1px;
        border: 1px solid var(--ink-4);
        border-radius: 50%;
        display: flex;
        align-items: center;
        justify-content: center;
        color: var(--accent);
      }
      .mark.box {
        border-radius: 3px;
      }
      .opt.sel .mark {
        border-color: var(--accent);
      }
      .otherText {
        width: 100%;
        margin-top: var(--sp-3);
        box-sizing: border-box;
        resize: none;
        overflow: hidden;
        font-family: var(--font-mono);
        font-size: var(--fs-xs);
        line-height: 1.5;
        padding: var(--sp-3) var(--sp-4);
        border-radius: var(--r-sm, 5px);
        border: 1px solid var(--accent);
        background: var(--panel-2, var(--panel));
        color: var(--ink);
        outline: none;
      }
      .nav {
        display: flex;
        align-items: center;
        gap: var(--sp-3);
        margin-top: var(--sp-5);
        flex-wrap: wrap;
      }
      .badge {
        display: inline-flex;
        align-items: center;
        gap: var(--sp-2);
        font-size: var(--fs-3xs);
        letter-spacing: 0.06em;
        text-transform: uppercase;
        padding: 1px var(--sp-3);
        border-radius: 999px;
        border: 1px solid var(--accent-2);
        color: var(--accent-2);
        background: color-mix(in oklch, var(--accent-2), transparent 88%);
      }
      .hint {
        margin-top: var(--sp-3);
        font-size: var(--fs-2xs);
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
          <span class="up tnum" style="font-size:var(--fs-3xs);letter-spacing:0.06em;color:var(--ink-4)"
            >Question {{ i + 1 }} of {{ qs.length }}</span
          >
          @if (q.header) {
            <span class="up" style="font-size:var(--fs-3xs);letter-spacing:0.06em;color:var(--accent)">{{ q.header }}</span>
          }
          @if (q.multiSelect) {
            <span class="badge" title="Multi-select is experimental: the claude TUI's space-toggle is unreliable. Use Terminal if it doesn't take."
              >experimental</span
            >
          }
        </div>

        <!-- the question text -->
        <div style="font-size:var(--fs-sm);line-height:1.45;color:var(--ink-2);margin-top:var(--sp-2)">{{ q.question }}</div>

        <!-- options (radio for single, checkbox for multi) + the synthesized
             "Other" free-text choice claude auto-appends -->
        <div style="display:flex;flex-direction:column;gap:var(--sp-2);margin-top:var(--sp-3)">
          @for (opt of q.options; track $index) {
            <button
              type="button"
              class="opt"
              [class.sel]="isSelected(i, $index)"
              [title]="opt.description || opt.label"
              (click)="toggle(i, $index, q.multiSelect === true)"
            >
              <span class="mark" [class.box]="q.multiSelect === true">
                @if (isSelected(i, $index)) {
                  <app-icon name="check" [px]="9" />
                }
              </span>
              <span style="min-width:0">{{ opt.label }}</span>
            </button>
          }

          <!-- "Other" — always available (claude auto-appends a free-text choice
               AFTER the concrete options). Selecting it reveals the auto-growing
               textarea. -->
          <button
            type="button"
            class="opt"
            [class.sel]="isOther(i)"
            title="Type a custom answer (claude's auto-appended free-text choice)"
            (click)="chooseOther(i)"
          >
            <span class="mark" [class.box]="q.multiSelect === true">
              @if (isOther(i)) {
                <app-icon name="check" [px]="9" />
              }
            </span>
            <span>Other…</span>
          </button>
          @if (isOther(i)) {
            <textarea
              class="otherText"
              rows="1"
              placeholder="Type your answer…"
              [value]="otherText(i)"
              (input)="onOtherInput(i, $event)"
              (click)="$event.stopPropagation()"
            ></textarea>
          }
        </div>

        @if (q.multiSelect) {
          <div class="hint">
            Multi-select is best-effort — the claude TUI's space-toggle is buggy. If it doesn't take, answer in the Terminal.
          </div>
        }

        <!-- stepper nav + actions -->
        <div class="nav">
          @if (i > 0) {
            <button class="btn ghost-hair" style="padding:var(--sp-2) var(--sp-4);font-size:var(--fs-sm)" (click)="back()">
              <app-icon name="chevron" size="sm" style="transform:rotate(180deg)" />Back
            </button>
          }
          @if (i < qs.length - 1) {
            <button class="btn ghost-hair" style="padding:var(--sp-2) var(--sp-4);font-size:var(--fs-sm)" (click)="next()">
              Next<app-icon name="chevron" size="sm" />
            </button>
          } @else {
            <button class="btn primary" style="padding:var(--sp-2) var(--sp-4);font-size:var(--fs-sm)" [disabled]="sending()" (click)="submit()">
              <app-icon name="check" size="sm" />{{ sending() ? "Sending…" : "Submit" }}
            </button>
          }
          <!-- Terminal: the RELIABLE fallback for every question -->
          <button class="btn ghost-hair" style="padding:var(--sp-2) var(--sp-4);font-size:var(--fs-sm)" (click)="terminal()">
            <app-icon name="terminal" size="sm" />Terminal
          </button>
          <!-- Reject: Esc cancels the numbered select -->
          <button class="btn ghost-hair" style="padding:var(--sp-2) var(--sp-4);font-size:var(--fs-sm)" (click)="reject()">
            <app-icon name="x" size="sm" />Reject
          </button>
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

  /** Auto-grow: store the text + resize the textarea to fit content (min 1 row,
   *  max ~6). Height tracks scrollHeight on every input. */
  onOtherInput(q: number, ev: Event) {
    const el = ev.target as HTMLTextAreaElement;
    this.otherTexts.update((m) => ({ ...m, [q]: el.value }));
    this.autoGrow(el);
  }
  private autoGrow(el: HTMLTextAreaElement) {
    el.style.height = "auto";
    const line = parseFloat(getComputedStyle(el).lineHeight) || 16;
    const max = line * 6 + 14; // ~6 rows + vertical padding
    el.style.height = Math.min(el.scrollHeight, max) + "px";
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
