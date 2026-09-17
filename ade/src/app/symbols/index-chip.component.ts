import { ChangeDetectionStrategy, Component, computed, inject } from "@angular/core";
import { LibSrcStore } from "../extensions/libsrc.store";
import { IconComponent } from "../shared/icon.component";
import { fmtN } from "../utils";
import { IndexStatusStore } from "./index-status.store";

/** What the chip is counting: project roots, library sources, or both. */
export type IndexChipMode = "symbols" | "library" | "both";

/**
 * Footer chip for the symbol index (design orrery-v2 `IndexChip`), covering
 * both index jobs:
 *  - roots only: "symbols · indexing 2,341 / 10,020", or "indexing 3 roots"
 *    when several run at once;
 *  - library sources only (M4): "library · indexing JDK 21 · 2,341 / 23,010",
 *    or "indexing 2 sources";
 *  - both: "indexing 2 roots · JDK 21" (the library half names the one
 *    source, or counts them);
 * with one mini meter over the summed progress, blocked-tinted when the last
 * root run failed and nothing runs. Renders nothing when there is nothing to
 * say — the footer only carries it while it is true. Recipe `.lsp-chip.idx`
 * in styles.css (shared chrome).
 */
@Component({
  selector: "app-index-chip",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  host: { style: "display:contents" },
  template: `
    @if (visible()) {
      <span class="lsp-chip idx" [class.error]="error()" [title]="title()" [attr.data-mode]="mode()" data-testid="index-chip">
        <app-icon name="search" size="sm" />
        @if (error()) {
          symbols · index failed
        } @else {
          @switch (mode()) {
            @case ("symbols") {
              symbols · indexing
              @if (roots() > 1) {
                <span class="mono">{{ roots() }} roots</span>
              } @else {
                <span class="mono">{{ done() }} / {{ total() }}</span>
              }
            }
            @case ("library") {
              library · indexing
              @if (libs() > 1) {
                <span class="mono">{{ libs() }} sources</span>
              } @else {
                {{ libLabel() }}@if (totalN() > 0) { · <span class="mono">{{ done() }} / {{ total() }}</span> }
              }
            }
            @default {
              indexing <span class="mono">{{ summary() }}</span>
            }
          }
          <span class="meter"><i [style.width.%]="pct()"></i></span>
        }
      </span>
    }
  `,
})
export class IndexChipComponent {
  readonly store = inject(IndexStatusStore);
  readonly libsrc = inject(LibSrcStore);

  readonly roots = computed(() => this.store.indexing().length);
  readonly libs = computed(() => this.libsrc.indexing().length);
  readonly visible = computed(() => this.store.visible() || this.libs() > 0);
  readonly mode = computed<IndexChipMode>(() => {
    if (this.roots() > 0 && this.libs() > 0) return "both";
    return this.libs() > 0 ? "library" : "symbols";
  });
  readonly error = computed(() => this.roots() === 0 && this.libs() === 0 && this.store.errored().length > 0);
  /** The one indexing source's label ("JDK 21"). */
  readonly libLabel = computed(() => this.libsrc.indexing()[0]?.label ?? "");
  readonly doneN = computed(() => this.store.done() + this.libsrc.done());
  /** Zero before the first status of a run (the figures are withheld). */
  readonly totalN = computed(() => this.store.total() + this.libsrc.total());
  readonly done = computed(() => fmtN(this.doneN()));
  readonly total = computed(() => fmtN(this.totalN()));
  readonly pct = computed(() => {
    const t = this.totalN();
    return t > 0 ? Math.min(100, Math.round((100 * this.doneN()) / t)) : 0;
  });
  /** "2 roots · JDK 21" / "1 root · 2 sources" — the both-at-once copy. */
  readonly summary = computed(() => {
    const r = this.roots();
    const l = this.libs();
    return `${r} root${r > 1 ? "s" : ""} · ${l > 1 ? `${l} sources` : this.libLabel()}`;
  });
  readonly title = computed(() => {
    if (this.error()) return this.store.errored().map((s) => s.error || "index failed").join("\n");
    const lines: string[] = [];
    if (this.roots() > 0) lines.push("Building the symbol index");
    for (const s of this.libsrc.indexing()) lines.push(`Indexing library source ${s.label}${s.path ? ` (${s.path})` : ""}`);
    return lines.join("\n");
  });
}
