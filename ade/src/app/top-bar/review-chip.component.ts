import { ChangeDetectionStrategy, Component, computed, inject, signal } from "@angular/core";
import { KjButtonComponent } from "@kouji-ui/components";
import { Agent } from "../models";
import { AgentRuntimeService } from "../agents/agent-runtime.service";
import { AgentReviewService } from "../agents/agent-review.service";
import { isBlock, ReviewComment, ReviewStore } from "../agents/review.store";
import { DismissDirective } from "../shared/dismiss.directive";
import { IconComponent } from "../shared/icon.component";
import { StatusDotComponent } from "../shared/status-dot.component";
import { ToolBadgeComponent } from "../shared/tool-badge.component";
import { UiStore } from "../ui/ui.store";
import { fileDir, fileName } from "../utils";
import { FileGroup, groupByFile } from "../workspace/review/send-review.component";

interface AgentGroup {
  agentId: string;
  /** Undefined when the agent was removed after its comments were queued —
   *  the queue still shows (and can be discarded), it just has no live row. */
  agent: Agent | undefined;
  comments: ReviewComment[];
  files: FileGroup[];
}

/**
 * ReviewChipComponent — top-bar "Feedback" chip (design/orrery-v2.html
 * FeedbackChip). The per-pane Send review button only sees ONE agent's queue;
 * this is the cross-agent view: every queued comment, grouped agent → file →
 * row, with a single OK that delivers each agent its own message. Hidden
 * while nothing is queued so the right cluster keeps its usual width.
 */
@Component({
  selector: "app-review-chip",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [KjButtonComponent, IconComponent, StatusDotComponent, ToolBadgeComponent, DismissDirective],
  template: `
    @if (total() > 0) {
      <div style="position:relative;display:flex;align-items:center">
        <!-- data-dismiss-ignore: the chip toggles the popover, so its own
             mousedown must not count as an outside click (it would close and
             the click that follows would instantly reopen). -->
        <kj-button
          kjVariant="ghost"
          class="fb-chip"
          [class.open]="open()"
          data-dismiss-ignore
          (click)="open.set(!open())"
          [title]="'Feedback · ' + total() + ' comment' + (total() !== 1 ? 's' : '') + ' queued'"
          kjAriaLabel="Feedback"
          style="display:flex"
        >
          <app-icon name="chat" size="sm" style="color:var(--ui-ink)" />
          <span
            class="tnum"
            style="font-size:var(--fs-badge);font-weight:var(--fw-strong);color:var(--ui-on-fill);background:var(--ui-fill);border-radius:999px;padding:0 var(--sp-3);line-height:16px"
          >{{ total() }}</span>
        </kj-button>

        @if (open()) {
          <div class="fb-pop rise" role="dialog" aria-label="Feedback" (appDismiss)="open.set(false)">
            <div class="hd">
              <app-icon name="chat" style="color:var(--ui-ink)" />
              <span class="disp" style="font-size:var(--fs-body);font-weight:var(--fw-strong)">Feedback</span>
              <span class="tnum" style="font-size:var(--fs-meta);color:var(--ink-3)">
                {{ total() }} comment{{ total() !== 1 ? 's' : '' }} · {{ groups().length }} agent{{ groups().length !== 1 ? 's' : '' }}
              </span>
              <button class="pane-btn" style="margin-left:auto" title="Close" (click)="open.set(false)">
                <app-icon name="x" size="sm" />
              </button>
            </div>

            <div class="scroll-y" style="max-height: round(calc(440px * var(--density)), 1px);padding:var(--sp-2) 0 var(--sp-3)">
              @for (g of groups(); track g.agentId) {
                <div>
                  <div class="agent">
                    <app-status-dot [status]="g.agent?.status ?? 'idle'" />
                    <span class="name">{{ g.agent?.name ?? g.agentId }}</span>
                    @if (g.agent) { <app-tool-badge [tool]="g.agent.tool" [size]="14" /> }
                    <span class="tnum" style="font-size:var(--fs-badge);color:var(--ink-4)">{{ g.comments.length }}</span>
                    <kj-button
                      kjVariant="ghost"
                      kjSize="xs"
                      class="btn"
                      style="display:flex;margin-left:auto"
                      title="Discard this agent's comments"
                      (click)="review.clear(g.agentId)"
                    >
                      <app-icon name="trash" size="sm" />Discard
                    </kj-button>
                  </div>
                  @for (f of g.files; track f.file) {
                    <div>
                      <div class="file">
                        <app-icon name="file" size="sm" style="width:round(calc(12px * var(--density)), 1px);height:round(calc(12px * var(--density)), 1px);color:var(--ink-4)" />
                        <span>{{ dirOf(f.file) }}</span>
                        <span class="name" style="margin-left:calc(-1 * var(--sp-3))">{{ nameOf(f.file) }}</span>
                      </div>
                      @for (c of f.items; track c.id) {
                        <div class="row">
                          <span class="ln">:{{ linesOf(c) }}</span>
                          <span class="q">{{ quoteOf(c) }}</span>
                          <span class="note"><span class="arr">→</span><span>{{ c.note }}</span></span>
                        </div>
                      }
                    </div>
                  }
                </div>
              }
            </div>

            <div class="ft">
              <span class="hint">OK sends each agent its comments as one message</span>
              <div style="margin-left:auto;display:flex;gap:var(--sp-4)">
                <kj-button kjVariant="outline" kjSize="sm" class="btn" (click)="open.set(false)" title="Close — comments stay queued">
                  <app-icon name="x" size="sm" />Cancel
                </kj-button>
                <kj-button kjVariant="default" kjSize="sm" class="btn" (click)="sendAll()">
                  <app-icon name="enter" size="sm" />OK
                </kj-button>
              </div>
            </div>
          </div>
        }
      </div>
    }
  `,
  styles: [
    `
      /* Chip skin from the design's inline FeedbackChip styles. kouji declares
         --kj-button-* ON its inner .kj-button (layered), so a value on the
         <kj-button> host only inherits and loses — this unlayered ::ng-deep
         rule retargets the inner node instead. */
      :host ::ng-deep kj-button.fb-chip .kj-button {
        --kj-button-fg: var(--ink);
        --kj-button-bg: transparent;
        --kj-button-border-color: var(--hair);
        --kj-button-padding-x: 7px;
        --kj-button-padding-y: 4px;
        --kj-button-height: auto;
        border: 1px solid var(--hair);
        gap: var(--sp-3);
      }
      :host ::ng-deep kj-button.fb-chip.open .kj-button {
        --kj-button-bg: var(--ui-sel);
        --kj-button-border-color: var(--ui-line);
        background: var(--ui-sel);
        border-color: var(--ui-line);
      }
    `,
  ],
})
export class ReviewChipComponent {
  readonly review = inject(ReviewStore);
  private readonly runtime = inject(AgentRuntimeService);
  private readonly agentReview = inject(AgentReviewService);
  private readonly ui = inject(UiStore);

  readonly open = signal(false);

  readonly total = computed(() => this.review.totalCount());

  readonly groups = computed<AgentGroup[]>(() => {
    const agents = this.runtime.agents();
    return this.review.allByAgent().map((g) => ({
      agentId: g.agentId,
      agent: agents.find((a) => a.id === g.agentId),
      comments: g.comments,
      files: groupByFile(g.comments),
    }));
  });

  nameOf(path: string): string { return fileName(path); }
  dirOf(path: string): string { return fileDir(path); }
  linesOf(c: { fromLine: number; toLine: number }): string {
    return c.fromLine === c.toLine ? `${c.fromLine}` : `${c.fromLine}–${c.toLine}`;
  }
  /** Block comments summarise as a line count; single-line ones quote the text. */
  quoteOf(c: ReviewComment): string {
    if (isBlock(c)) return `${c.lines.length} lines`;
    return `“${c.quote || c.snippet || "(blank)"}”`;
  }

  /** One message per agent, then the queue empties and the popover closes. */
  sendAll(): void {
    const groups = this.groups();
    for (const g of groups) {
      this.agentReview.sendReview(g.agentId, this.review.buildPayload(g.agentId, ""));
      this.review.clear(g.agentId);
    }
    this.open.set(false);
    this.ui.flash(`sent feedback to ${groups.length} agent${groups.length !== 1 ? "s" : ""}`);
  }
}
