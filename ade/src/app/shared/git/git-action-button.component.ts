import {
  ChangeDetectionStrategy,
  Component,
  computed,
  inject,
  input,
  output,
  signal,
} from "@angular/core";
import { CostEstimate } from "../../models";
import { COST_FEATURES_ENABLED } from "../../cost/cost-flags";
import { EstimateService, EstimateInput, fmtTok, fmtUsd } from "../../cost/estimate.service";
import { IconComponent } from "../icon.component";
import { KjButtonComponent, KjButtonGroupComponent } from "@kouji-ui/components";
import { KjDropdownMenuContent, KjDropdownMenuItem, KjDropdownMenuTrigger } from "@kouji-ui/core";

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
  imports: [IconComponent, KjButtonComponent, KjButtonGroupComponent, KjDropdownMenuItem, KjDropdownMenuTrigger, KjDropdownMenuContent],
  template: `
    @if (!variants().length) {
      <kj-button [kjVariant]="variant()" [kjSize]="size()"
        [kjDisabled]="disabled()"
        [title]="title() || label() + (costEnabled ? ' · native, 0 tokens' : ' · native')"
        (click)="pressNative()"
        style="--kj-button-min-width: 0; --kj-button-justify: flex-start"
      >
        @if (icon()) { <app-icon [name]="icon()!" size="sm" /> }
        <span class="trunc">{{ label() }}</span>
      </kj-button>
    } @else {
      <kj-button-group class="split" kjAriaLabel="Git action">
        <kj-button [kjVariant]="variant()" [kjSize]="size()" class="main"
          [kjDisabled]="disabled() || (aiOnly() && primaryAiBlocked())"
          [title]="primaryTitle()"
          (click)="pressPrimary()"
        >
          @if (icon()) { <app-icon [name]="icon()!" size="sm" [color]="aiOnly() ? 'var(--ui-ink)' : null" /> }
          <span class="trunc">{{ primaryConfirming() && primaryEstimate() ? 'Confirm ≈' + usd(primaryEstimate()!.usdHigh) + ' — click again' : label() }}</span>
          @if (costEnabled && aiOnly() && primaryEstimate(); as pe) {
            <span class="tnum est-inline">~{{ tok(pe.tokensHigh) }} tok · ≈{{ usd(pe.usdHigh) }}</span>
          }
        </kj-button>
        <kj-button [kjVariant]="variant()" [kjSize]="size()" class="caret"
          kjDropdownMenuTrigger #aiTrigger="kjDropdownMenuTrigger"
          [kjDisabled]="disabled()"
          [title]="costEnabled ? 'AI path — shows its price first' : 'AI path'"
                  >
          <app-icon size="md" name="chevronD" />
        </kj-button>

        <kj-dropdown-menu-content
          class="popover menu rise"
          [kjFor]="aiTrigger"
          kjMount="portal"
          kjSide="bottom"
          kjAlign="end"
        >
          <div class="menu-head">
            <app-icon size="md" name="sparkles" color="var(--ui-ink)" />
            <span class="up" style="color:var(--ink-3)">{{ costEnabled ? 'AI path · spends tokens' : 'AI path' }}</span>
            @if (costEnabled && est.capUsd() > 0) {
              <span class="tnum" style="margin-left:auto;font-size:var(--fs-meta);color:var(--ink-4)">
                {{ usd(est.remainingUsd()) }} left of {{ usd(est.capUsd()) }}
              </span>
            }
          </div>
          @for (v of rows(); track v.variant.id) {
            <kj-button kjVariant="ghost" kjDropdownMenuItem class="row"
              [kjDisabled]="v.overCap"
              [title]="v.overCap ? 'would exceed the budget cap — raise it in Settings' : v.estimate.confidence"
              (click)="pressVariant(v.variant, v.estimate)"
              [style.opacity]="v.overCap ? 0.45 : 1"
            >
              <app-icon size="md" [name]="v.variant.icon || 'sparkles'" color="var(--ink-3)" />
              <span class="row-lbl trunc">
                {{ confirming() === v.variant.id ? 'Confirm ≈' + usd(v.estimate.usdHigh) + ' — click again' : v.variant.label }}
              </span>
              @if (costEnabled) {
                <span class="tnum row-est">
                  <span>~{{ tok(v.estimate.tokensHigh) }} tok</span>
                  <span style="color:var(--ink)">≈{{ usd(v.estimate.usdHigh) }}</span>
                </span>
              }
            </kj-button>
          }
          <div class="menu-foot">{{ costEnabled ? 'native path is instant, deterministic and costs 0 tokens' : 'native path is instant and deterministic' }}</div>
        </kj-dropdown-menu-content>
      </kj-button-group>
    }
  `,
  styles: [
    `
      /* Content-sized by default: the action bar's message input takes the
         slack, so stretching every split button just made them all huge. A
         call site that wants fill can still set flex on the element. */
      :host {
        display: flex;
        min-width: 0;
        flex: none;
      }
      .split {
        display: flex;
        flex: 1;
        min-width: 0;
      }
      .est-inline {
        margin-left: auto;
        flex: none;
        font-size: var(--fs-meta);
        color: var(--ink-3);
      }
      /* elevated card surface comes from the shared .popover */
      .menu {
        z-index: 40;
        min-width: round(calc(316px * var(--density)), 1px);
        padding: var(--sp-2);
      }
      .menu-head {
        display: flex;
        align-items: center;
        gap: var(--sp-3);
        padding: var(--sp-2) var(--sp-4) var(--sp-3);
      }
      .row-lbl {
        color: var(--ink);
      }
      .row-est {
        margin-left: auto;
        flex: none;
        display: flex;
        gap: var(--sp-3);
        align-items: baseline;
        color: var(--ink-3);
      }
      .menu-foot {
        padding: var(--sp-2) var(--sp-4) var(--sp-1);
        font-size: var(--fs-badge);
        color: var(--ink-4);
        border-top: 1px solid var(--hair);
        margin-top: var(--sp-2);
      }
    `,
  ],
})
export class GitActionButtonComponent {
  /** Cost kill switch — when off, all token/$ chrome disappears but the
   *  native/AI action paths keep working. */
  readonly costEnabled = COST_FEATURES_ENABLED;
  readonly est = inject(EstimateService);

  readonly label = input.required<string>();
  readonly icon = input<string | null>(null);
  /** Button style class ("ghost-hair" | "primary" | ""). */
  readonly kind = input("ghost-hair");

  /** kouji variant for the caller-supplied `kind`. Callers still pass Orrery's
   *  historical names ("ghost-hair" / "primary"), so the mapping lives here
   *  rather than at every call site. */
  readonly variant = computed(() => {
    const k = this.kind();
    return k === "primary" ? "default" : k === "ghost-hair" ? "outline" : k;
  });
  readonly disabled = input(false);
  readonly small = input(false);
  readonly title = input("");
  /** Base estimate input; per-variant `op`/`verbose` override it. */
  readonly estimateInput = input.required<EstimateInput>();
  readonly variants = input<GitActionVariant[]>([]);
  /** No native path yet: primary press runs the FIRST variant (estimate
   *  inline on the button), remaining variants stay in the dropdown. */
  readonly aiOnly = input(false);

  readonly native = output<void>();
  readonly ai = output<GitActionAiEvent>();

  /** Variant id awaiting its confirming second click (confirm-above guard). */
  readonly confirming = signal<string | null>(null);

  readonly tok = fmtTok;
  readonly usd = fmtUsd;
  /** kouji size step. `small` call sites want the dense chrome size. */
  readonly size = computed(() => (this.small() ? "xs" : "sm"));

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
    if (this.aiOnly()) return this.label() + (this.costEnabled ? " · AI path — price shown on the button" : " · AI path");
    return this.label() + (this.costEnabled ? " · native, 0 tokens" : " · native");
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
    this.est.recordSpend(estimate);
    this.ai.emit({ variantId, estimate });
  }

}
