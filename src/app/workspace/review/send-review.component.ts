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
  standalone: true,
  imports: [IconComponent],
  template: `
    <!-- backdrop -->
    <div
      (click)="close.emit()"
      style="position:fixed;inset:0;z-index:70;display:grid;place-items:center;padding:24px;background:rgba(0,0,0,0.5);backdrop-filter:blur(3px)"
    >
      <!-- card -->
      <div
        class="surface rise"
        (click)="$event.stopPropagation()"
        style="width:600px;max-height:88vh;display:flex;flex-direction:column;padding:0;overflow:hidden;box-shadow:var(--shadow)"
      >
        <!-- header -->
        <div style="padding:14px 18px;border-bottom:1px solid var(--hair);display:flex;align-items:center;gap:9px;flex:none">
          <app-icon name="chat" style="color:var(--accent)" />
          <span class="disp" style="font-size:14px;font-weight:600">Send review</span>
          <span class="tnum" style="font-size:11px;color:var(--ink-3)">
            {{ comments().length }} comment{{ comments().length !== 1 ? 's' : '' }} · {{ groups().length }} file{{ groups().length !== 1 ? 's' : '' }}
          </span>
          <span class="chip" style="margin-left:auto;font-size:9.5px">→ {{ agentName() || agent() }}</span>
          <button
            (click)="close.emit()"
            style="background:transparent;border:none;color:var(--ink-4);cursor:pointer;display:flex;padding:3px;border-radius:4px"
          >
            <app-icon name="x" size="sm" />
          </button>
        </div>

        <!-- grouped comment list -->
        <div class="scroll-y" style="flex:1;min-height:0;padding:6px 0">
          @if (groups().length === 0) {
            <div style="display:grid;place-items:center;color:var(--ink-4);font-size:12px;padding:40px">
              no comments yet — hover a line and click the + to add one
            </div>
          } @else {
            @for (g of groups(); track g.file) {
              <div style="margin-bottom:4px">
                <!-- file header -->
                <div style="display:flex;align-items:center;gap:7px;padding:7px 18px;position:sticky;top:0;background:var(--panel);z-index:1">
                  <app-icon name="file" size="sm" style="color:var(--ink-3)" />
                  <span style="font-size:11px;color:var(--ink-4)">{{ dirOf(g.file) }}</span>
                  <span style="font-size:11.5px;color:var(--ink);margin-left:-3px">{{ nameOf(g.file) }}</span>
                  <span class="tnum" style="margin-left:auto;font-size:9.5px;color:var(--ink-4)">{{ g.items.length }}</span>
                </div>
                <!-- comment rows -->
                @for (c of g.items; track c.id) {
                  <div style="display:flex;gap:10px;padding:8px 18px 10px;margin:0 10px;border-radius:var(--r-sm)">
                    <span class="tnum" style="flex:none;width:54px;padding-top:2px;font-size:10.5px;color:var(--accent-2);text-align:right">:{{ linesOf(c) }}</span>
                    <div style="flex:1;min-width:0">
                      <div class="tnum" style="font-size:11px;color:var(--ink-3);background:var(--panel-2);border:1px solid var(--hair);border-radius:4px;padding:4px 8px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">
                        @if (blockComment(c)) {
                          <span style="color:var(--ink-4)">{{ c.lines.length }} lines · </span>
                        }
                        {{ c.snippet || '(blank line)' }}
                      </div>
                      <div style="display:flex;gap:6px;margin-top:6px">
                        <span style="color:var(--accent);flex:none">→</span>
                        <span style="font-size:12px;color:var(--ink);line-height:1.5;white-space:pre-wrap;word-break:break-word">{{ c.note }}</span>
                      </div>
                    </div>
                    <button
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
            <span class="up" style="font-size:9px;color:var(--ink-3)">Global note</span>
            <span style="font-size:9.5px;color:var(--ink-4)">· applies to the whole review</span>
          </div>
          <textarea
            [value]="global()"
            (input)="onGlobalInput($event)"
            rows="2"
            placeholder="Overall direction for this pass — e.g. tighten error handling and keep the public API stable…"
            style="width:100%;resize:none;background:var(--panel);border:1px solid var(--hair);border-radius:var(--r-sm);padding:9px 11px;color:var(--ink);font-family:var(--font-mono);font-size:12px;line-height:1.5;outline:none"
          ></textarea>
        </div>

        <!-- footer -->
        <div style="padding:12px 18px;border-top:1px solid var(--hair);display:flex;align-items:center;gap:8px;flex:none">
          <span style="font-size:10px;color:var(--ink-4)">delivered to the agent as one structured message</span>
          <div style="margin-left:auto;display:flex;gap:8px">
            <button class="btn ghost-hair" (click)="close.emit()">Cancel</button>
            <button
              class="btn primary"
              (click)="send()"
              [disabled]="!comments().length"
            >
              <app-icon name="enter" size="sm" />Send to agent
            </button>
          </div>
        </div>
      </div>
    </div>
  `,
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
  blockComment(c: { fromIdx: number; toIdx: number }): boolean { return isBlock(c); }

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
  standalone: true,
  imports: [IconComponent, SendReviewModalComponent],
  template: `
    @if (count() > 0) {
      <button
        class="btn"
        (click)="open.set(true)"
        title="Review all pending comments and send them to this agent"
        style="padding:5px 11px;color:var(--ink);border:1px solid color-mix(in oklch,var(--accent),transparent 55%);background:color-mix(in oklch,var(--accent),transparent 86%)"
      >
        <app-icon name="chat" size="sm" style="color:var(--accent)" />Send review
        <span
          class="tnum"
          style="font-size:10px;font-weight:700;color:#06070b;background:var(--accent);border-radius:999px;padding:1px 6px;margin-left:2px"
        >{{ count() }}</span>
      </button>
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
