import {
  ChangeDetectionStrategy,
  Component,
  computed,
  ElementRef,
  HostListener,
  inject,
  input,
  output,
  signal,
} from "@angular/core";
import { CostEstimate } from "../../models";
import { EstimateService, EstimateInput, fmtTok, fmtUsd } from "../../cost/estimate.service";
import { IconComponent } from "../icon.component";

/** One AI variant row in the dropdown. `op`/`verbose` override the base
 *  estimate input for this row; the estimate renders ON the row (A4.1 —
 *  never hover-only). */
export interface GitActionVariant {
  id: string;
  label: string;
  op?: EstimateInput["op"];
  verbose?: boolean;
  icon?: string;
}

/** What a chosen AI variant hands back to the call site. */
export interface GitActionAiEvent {
  variantId: string;
  estimate: CostEstimate;
}

/**
 * The dual-path git action control (A4.1): a split button whose primary press
 * is the NATIVE operation (instant, free, deterministic) and whose dropdown
 * offers the AI variants, each labelled with its token/$ estimate on the row.
 * Owns the disclosure contract — budget-cap disabling and the confirm-above
 * second click — so no call site reimplements it (A4.5).
 *
 * With no variants it renders a plain native button. `aiOnly` inverts the
 * split for ops with no native path yet (e.g. rebase until A3.4): the primary
 * press runs the FIRST variant (estimate shown inline), still cap/confirm
 * guarded.
 */
@Component({
  selector: "app-git-action-button",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    @if (!variants().length) {
      <button
        class="btn {{ kind() }}"
        [disabled]="disabled()"
        [title]="title() || label() + ' · native, 0 tokens'"
        (click)="pressNative()"
        [style.padding]="pad()"
        [style.font-size]="fs()"
        style="flex:1;min-width:0;justify-content:flex-start"
      >
        @if (icon()) { <app-icon [name]="icon()!" size="sm" /> }
        <span class="lbl">{{ label() }}</span>
      </button>
    } @else {
      <div class="split">
        <button
          class="btn {{ kind() }} main"
          [disabled]="disabled() || (aiOnly() && primaryAiBlocked())"
          [title]="primaryTitle()"
          (click)="pressPrimary()"
          [style.padding]="pad()"
          [style.font-size]="fs()"
        >
          @if (icon()) { <app-icon [name]="icon()!" size="sm" [color]="aiOnly() ? 'var(--accent)' : null" /> }
          <span class="lbl">{{ primaryConfirming() && primaryEstimate() ? 'Confirm ≈' + usd(primaryEstimate()!.usdHigh) + ' — click again' : label() }}</span>
          @if (aiOnly() && primaryEstimate(); as pe) {
            <span class="tnum est-inline">~{{ tok(pe.tokensHigh) }} tok · ≈{{ usd(pe.usdHigh) }}</span>
          }
        </button>
        <button
          class="btn {{ kind() }} caret"
          [disabled]="disabled()"
          title="AI path — shows its price first"
          (click)="open.set(!open())"
          [style.font-size]="fs()"
        >
          <app-icon name="chevronD" size="sm" [px]="12" />
        </button>

        @if (open()) {
          <div class="menu rise" [style.left]="menuAlign() === 'left' ? '0' : 'auto'" [style.right]="menuAlign() === 'left' ? 'auto' : '0'">
            <div class="menu-head">
              <app-icon name="sparkles" size="sm" [px]="12" color="var(--accent)" />
              <span class="up" style="font-size:var(--fs-3xs);color:var(--ink-3)">AI path · spends tokens</span>
              @if (est.capUsd() > 0) {
                <span class="tnum" style="margin-left:auto;font-size:var(--fs-2xs);color:var(--ink-4)">
                  {{ usd(est.remainingUsd()) }} left of {{ usd(est.capUsd()) }}
                </span>
              }
            </div>
            @for (v of rows(); track v.variant.id) {
              <button
                class="btn row"
                [disabled]="v.overCap"
                [title]="v.overCap ? 'would exceed the budget cap — raise it in Settings' : v.estimate.confidence"
                (click)="pressVariant(v.variant, v.estimate)"
                [style.opacity]="v.overCap ? 0.45 : 1"
              >
                <app-icon [name]="v.variant.icon || 'sparkles'" size="sm" [px]="12" color="var(--accent-2)" />
                <span class="row-lbl">
                  {{ confirming() === v.variant.id ? 'Confirm ≈' + usd(v.estimate.usdHigh) + ' — click again' : v.variant.label }}
                </span>
                <span class="tnum row-est">
                  <span>~{{ tok(v.estimate.tokensHigh) }} tok</span>
                  <span style="color:var(--accent-2)">≈{{ usd(v.estimate.usdHigh) }}</span>
                </span>
              </button>
            }
            <div class="menu-foot">native path is instant, deterministic and costs 0 tokens</div>
          </div>
        }
      </div>
    }
  `,
  styles: [
    `
      :host {
        display: flex;
        min-width: 0;
        flex: 1;
      }
      .split {
        position: relative;
        display: flex;
        flex: 1;
        min-width: 0;
      }
      .btn.main {
        flex: 1;
        min-width: 0;
        justify-content: flex-start;
        border-top-right-radius: 0;
        border-bottom-right-radius: 0;
        border-right: none;
      }
      .btn.caret {
        flex: none;
        padding: 0 var(--sp-2);
        border-top-left-radius: 0;
        border-bottom-left-radius: 0;
        border-left: 1px solid var(--hair);
      }
      .btn.primary.caret {
        border-left: 1px solid rgba(0, 0, 0, 0.25);
      }
      .lbl {
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
      .est-inline {
        margin-left: auto;
        flex: none;
        font-size: var(--fs-2xs);
        color: var(--ink-3);
      }
      .menu {
        position: absolute;
        top: calc(100% + 5px);
        z-index: 40;
        min-width: 316px;
        padding: var(--sp-2);
        background: var(--elev);
        border: 1px solid var(--hair-2);
        border-radius: var(--r-md);
        box-shadow: var(--shadow);
      }
      .menu-head {
        display: flex;
        align-items: center;
        gap: var(--sp-3);
        padding: var(--sp-2) var(--sp-4) var(--sp-3);
      }
      .btn.row {
        width: 100%;
        justify-content: flex-start;
        gap: var(--sp-3);
        padding: var(--sp-3) var(--sp-4);
        border-radius: var(--r-sm);
        border: none;
        background: transparent;
      }
      .btn.row:hover:not(:disabled) {
        background: var(--panel-3);
      }
      .row-lbl {
        font-size: var(--fs-sm);
        color: var(--ink);
        min-width: 0;
        overflow: hidden;
        text-overflow: ellipsis;
        white-space: nowrap;
      }
      .row-est {
        margin-left: auto;
        flex: none;
        display: flex;
        gap: var(--sp-3);
        align-items: baseline;
        font-size: var(--fs-xs);
        color: var(--ink-3);
      }
      .menu-foot {
        padding: var(--sp-2) var(--sp-4) var(--sp-1);
        font-size: var(--fs-3xs);
        color: var(--ink-4);
        border-top: 1px solid var(--hair);
        margin-top: var(--sp-2);
      }
    `,
  ],
})
export class GitActionButtonComponent {
  readonly est = inject(EstimateService);
  private readonly host = inject(ElementRef<HTMLElement>);

  readonly label = input.required<string>();
  readonly icon = input<string | null>(null);
  /** Button style class ("ghost-hair" | "primary" | ""). */
  readonly kind = input("ghost-hair");
  readonly disabled = input(false);
  readonly small = input(false);
  readonly title = input("");
  readonly menuAlign = input<"left" | "right">("right");
  /** Base estimate input; per-variant `op`/`verbose` override it. */
  readonly estimateInput = input.required<EstimateInput>();
  readonly variants = input<GitActionVariant[]>([]);
  /** No native path yet: primary press runs the FIRST variant (estimate
   *  inline on the button), remaining variants stay in the dropdown. */
  readonly aiOnly = input(false);

  readonly native = output<void>();
  readonly ai = output<GitActionAiEvent>();

  readonly open = signal(false);
  /** Variant id awaiting its confirming second click (confirm-above guard). */
  readonly confirming = signal<string | null>(null);

  readonly tok = fmtTok;
  readonly usd = fmtUsd;
  readonly pad = computed(() => (this.small() ? "var(--sp-1) var(--sp-4)" : "var(--sp-2) var(--sp-5)"));
  readonly fs = computed(() => (this.small() ? "var(--fs-xs)" : "var(--fs-sm)"));

  readonly rows = computed(() =>
    this.variants().map((variant) => {
      const base = this.estimateInput();
      const estimate = this.est.estimate({
        ...base,
        op: variant.op ?? base.op,
        verbose: variant.verbose ?? base.verbose,
      });
      return { variant, estimate, overCap: this.est.overCap(estimate) };
    }),
  );
  readonly primaryEstimate = computed<CostEstimate | null>(() =>
    this.aiOnly() ? (this.rows()[0]?.estimate ?? null) : null,
  );
  readonly primaryAiBlocked = computed(() => {
    const r = this.rows()[0];
    return !r || r.overCap;
  });
  readonly primaryConfirming = computed(() => this.confirming()?.startsWith("primary:") ?? false);
  readonly primaryTitle = computed(() => {
    if (this.title()) return this.title();
    if (this.aiOnly()) return this.label() + " · AI path — price shown on the button";
    return this.label() + " · native, 0 tokens";
  });

  pressNative(): void {
    this.native.emit();
  }
  pressPrimary(): void {
    if (!this.aiOnly()) {
      this.native.emit();
      return;
    }
    const r = this.rows()[0];
    if (r) this.runVariant(r.variant.id, r.estimate, "primary:" + r.variant.id);
  }
  pressVariant(v: GitActionVariant, estimate: CostEstimate): void {
    this.runVariant(v.id, estimate, v.id);
  }
  /** Cap is enforced by the disabled row; here we apply the confirm-above
   *  second-click, record the (estimated) spend, and emit. */
  private runVariant(variantId: string, estimate: CostEstimate, confirmKey: string): void {
    if (this.est.needsConfirm(estimate) && this.confirming() !== confirmKey) {
      this.confirming.set(confirmKey);
      return;
    }
    this.confirming.set(null);
    this.open.set(false);
    this.est.recordSpend(estimate);
    this.ai.emit({ variantId, estimate });
  }

  @HostListener("document:mousedown", ["$event"])
  onDocDown(e: MouseEvent): void {
    if (!this.open()) return;
    if (!this.host.nativeElement.contains(e.target as Node)) {
      this.open.set(false);
      this.confirming.set(null);
    }
  }
  @HostListener("document:keydown.escape")
  onEsc(): void {
    this.open.set(false);
    this.confirming.set(null);
  }
}
