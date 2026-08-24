import {
  ChangeDetectionStrategy,
  Component,
  computed,
  EventEmitter,
  inject,
  Input,
  Output,
  signal,
} from "@angular/core";
import { IconComponent } from "../../shared/icon.component";
import { ReviewStore } from "../../agents/review.store";
import { isBlock } from "../../agents/review.store";
import { AgentReviewService } from "../../agents/agent-review.service";
import { fileName, fileDir } from "../../utils";
import { KjBadgeComponent, KjButtonComponent, KjTextareaComponent } from "@kouji-ui/components";

function refLines(c: { fromLine: number; toLine: number }): string {
  return c.fromLine === c.toLine ? `${c.fromLine}` : `${c.fromLine}-${c.toLine}`;
}

// ─── Modal ───────────────────────────────────────────────────────────────────

/**
 * SendReviewModalComponent — pending comments grouped by file + global note + send.
 * Inputs use decorator @Input backed by signals (vitest JIT / NG0950 workaround).
 */
@Component({
  selector: "app-send-review-modal",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, KjButtonComponent, KjBadgeComponent, KjTextareaComponent],
  template: `
    <!-- backdrop -->
    <div
      (click)="close.emit()"
      class="scrim"
      style="z-index:70;padding:24px;-webkit-backdrop-filter:blur(3px);backdrop-filter:blur(3px)"
    >
      <!-- card -->
      <div
        class="surface rise"
        (click)="$event.stopPropagation()"
        style="width: round(calc(600px * var(--density)), 1px);max-height:88vh;display:flex;flex-direction:column;padding:0;overflow:hidden;box-shadow:var(--shadow)"
      >
        <!-- header -->
        <div style="padding:14px 18px;border-bottom:1px solid var(--hair);display:flex;align-items:center;gap:9px;flex:none">
          <app-icon name="chat" style="color:var(--ui-ink)" />
          <h3>Send review</h3>
          <span class="tnum" style="font-size:var(--fs-badge);color:var(--ink-3)">
            {{ comments().length }} comment{{ comments().length !== 1 ? 's' : '' }} · {{ groups().length }} file{{ groups().length !== 1 ? 's' : '' }}
          </span>
          <kj-badge style="margin-left:auto;font-size:var(--fs-micro)">→ {{ agentName() || agent() }}</kj-badge>
          <button kjButton
            (click)="close.emit()"
            style="background:transparent;border:none;color:var(--ink-4);cursor:pointer;display:flex;padding:3px;border-radius:4px"
          >
            <app-icon name="x" size="sm" />
          </button>
        </div>

        <!-- grouped comment list -->
        <div class="scroll-y" style="flex:1;min-height:0;padding:6px 0">
          @if (groups().length === 0) {
            <div class="pane-empty pad">no comments yet — hover a line and click the + to add one</div>
          } @else {
            @for (g of groups(); track g.file) {
              <div style="margin-bottom:4px">
                <!-- file header -->
                <div style="display:flex;align-items:center;gap:7px;padding:7px 18px;position:sticky;top:0;background:var(--panel);z-index:1">
                  <app-icon name="file" size="sm" style="color:var(--ink-3)" />
                  <span style="font-size:var(--fs-badge);color:var(--ink-4)">{{ dirOf(g.file) }}</span>
                  <span style="font-size:var(--fs-badge);color:var(--ink);margin-left:-3px">{{ nameOf(g.file) }}</span>
                  <span class="tnum" style="margin-left:auto;font-size:var(--fs-micro);color:var(--ink-4)">{{ g.items.length }}</span>
                </div>
                <!-- comment rows -->
                @for (c of g.items; track c.id) {
                  <div style="display:flex;gap:10px;padding:8px 18px 10px;margin:0 10px;border-radius:var(--r-sm)">
                    <span class="tnum" style="flex:none;width:54px;padding-top:2px;font-size:var(--fs-micro);color:var(--ink-3);text-align:right">:{{ linesOf(c) }}</span>
                    <div style="flex:1;min-width:0">
                      <div class="tnum trunc" style="font-size:var(--fs-badge);color:var(--ink-3);background:var(--panel-2);border:1px solid var(--hair);border-radius:4px;padding:4px 8px">
                        @if (blockComment(c)) {
                          <span style="color:var(--ink-4)">{{ c.lines.length }} lines · </span>
                        }
                        {{ c.snippet || '(blank line)' }}
                      </div>
                      <div style="display:flex;gap:6px;margin-top:6px">
                        <span style="color:var(--ui-ink);flex:none">→</span>
                        <span style="font-size:var(--fs-badge);color:var(--ink);line-height:1.5;white-space:pre-wrap;word-break:break-word">{{ c.note }}</span>
                      </div>
                    </div>
                    <button kjButton
                      (click)="removeComment(c.id)"
                      title="Delete comment"
                      style="flex:none;background:transparent;border:none;color:var(--ink-4);cursor:pointer;display:flex;padding:3px;border-radius:3px;align-self:flex-start"
                    >
                      <app-icon name="trash" size="sm" />
                    </button>
                  </div>
                }
              </div>
            }
          }
        </div>

        <!-- global note -->
        <div style="padding:12px 18px;border-top:1px solid var(--hair);flex:none;background:var(--panel-2)">
          <div style="display:flex;align-items:center;gap:6px;margin-bottom:7px">
            <span class="up" style="font-size:var(--fs-micro);color:var(--ink-3)">Global note</span>
            <span style="font-size:var(--fs-micro);color:var(--ink-4)">· applies to the whole review</span>
          </div>
          <kj-textarea
            class="gr-note"
            [kjValue]="global()"
            (input)="onGlobalInput($event)"
            kjRows="2"
            kjResize="none"
            kjPlaceholder="Overall direction for this pass — e.g. tighten error handling and keep the public API stable…"
          />
        </div>

        <!-- footer -->
        <div style="padding:12px 18px;border-top:1px solid var(--hair);display:flex;align-items:center;gap:8px;flex:none">
          <span style="font-size:var(--fs-micro);color:var(--ink-4)">delivered to the agent as one structured message</span>
          <div style="margin-left:auto;display:flex;gap:8px">
            <kj-button kjVariant="outline" (click)="close.emit()">Cancel</kj-button>
            <kj-button kjVariant="default" (click)="send()" [kjDisabled]="!comments().length">
              <app-icon name="enter" size="sm" />Send to agent
            </kj-button>
          </div>
        </div>
      </div>
    </div>
  `,
  styles: [
    `
      /* Global-note metrics — kouji declares the --kj-textarea-* knobs ON the
         inner .kj-textarea (layered), so this unlayered ::ng-deep rule
         retargets them there; host-level custom properties would lose. */
      :host ::ng-deep kj-textarea.gr-note .kj-textarea {
        --kj-textarea-bg: var(--panel);
        --kj-textarea-fg: var(--ink);
        --kj-textarea-border-color: var(--hair);
        --kj-textarea-radius: var(--r-sm);
        --kj-textarea-font: var(--font-mono);
        --kj-textarea-font-size: var(--fs-badge);
        --kj-textarea-line-height: 1.5;
        --kj-textarea-padding-x: 11px;
        --kj-textarea-padding-y: 9px;
        --kj-textarea-min-height: 0;
      }
    `,
  ],
})
export class SendReviewModalComponent {
  readonly review = inject(ReviewStore);
  readonly agentReview = inject(AgentReviewService);

  // Decorator @Input backed by signals — vitest JIT / NG0950 workaround.
  readonly agent = signal("");
  readonly agentName = signal("");

  @Input("agent") set agentInput(v: string) { this.agent.set(v); }
  @Input("agentName") set agentNameInput(v: string) { this.agentName.set(v); }

  @Output() close = new EventEmitter<void>();

  readonly global = signal("");

  readonly comments = computed(() => this.review.list(this.agent()));

  readonly groups = computed(() => {
    const byFile: Record<string, { file: string; items: ReturnType<ReviewStore["list"]> }> = {};
    const order: string[] = [];
    for (const c of this.comments()) {
      if (!byFile[c.file]) {
        byFile[c.file] = { file: c.file, items: [] };
        order.push(c.file);
      }
      byFile[c.file].items.push(c);
    }
    return order.map((f) => byFile[f]);
  });

  nameOf(path: string): string { return fileName(path); }
  dirOf(path: string): string { return fileDir(path); }
  linesOf(c: { fromLine: number; toLine: number }): string { return refLines(c); }
  blockComment(c: { fromLine: number; toLine: number }): boolean { return isBlock(c); }

  onGlobalInput(e: Event): void {
    this.global.set((e.target as HTMLTextAreaElement).value);
  }

  removeComment(id: string): void {
    this.review.remove(this.agent(), id);
  }

  send(): void {
    const payload = this.review.buildPayload(this.agent(), this.global());
    this.agentReview.sendReview(this.agent(), payload);
    this.review.clear(this.agent());
    this.close.emit();
  }
}

// ─── Button ──────────────────────────────────────────────────────────────────

/**
 * SendReviewButtonComponent — toolbar button; hidden when count===0.
 * Opens SendReviewModalComponent via an internal `open` signal.
 */
@Component({
  selector: "app-send-review-button",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent, SendReviewModalComponent, KjButtonComponent],
  template: `
    @if (count() > 0) {
      <kj-button kjVariant="ghost" (click)="open.set(true)" title="Review all pending comments and send them to this agent" style="--kj-button-fg: var(--ink); --kj-button-bg: var(--ui-sel)">
        <app-icon name="chat" size="sm" style="color:var(--ui-ink)" />Send review
        <span
          class="tnum"
          style="font-size:var(--fs-micro);font-weight:var(--fw-strong);color:var(--ui-on-fill);background:var(--ui-fill);border-radius:999px;padding:1px 6px;margin-left:2px"
        >{{ count() }}</span>
      </kj-button>
    }
    @if (open()) {
      <app-send-review-modal
        [agent]="agent()"
        [agentName]="agentName()"
        (close)="open.set(false)"
      />
    }
  `,
})
export class SendReviewButtonComponent {
  readonly review = inject(ReviewStore);

  // Decorator @Input backed by signals — vitest JIT / NG0950 workaround.
  readonly agent = signal("");
  readonly agentName = signal("");

  @Input("agent") set agentInput(v: string) { this.agent.set(v); }
  @Input("agentName") set agentNameInput(v: string) { this.agentName.set(v); }

  readonly open = signal(false);

  readonly count = computed(() => this.review.count(this.agent()));
}
