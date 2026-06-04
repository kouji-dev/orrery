import { ChangeDetectionStrategy, Component, computed, input, signal } from "@angular/core";
import { DIFFS } from "../data";
import { Agent, Diff } from "../models";
import { IconComponent } from "../shared/icon.component";
import { fileDir, fileName, mix } from "../utils";

@Component({
  selector: "app-diff-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    @let ag = agent();
    <div style="flex:1;display:grid;grid-template-columns:236px 1fr;min-height:0">
      <!-- file list -->
      <div class="scroll-y" style="border-right:1px solid var(--hair);background:var(--panel);padding:6px 0">
        <div class="up" style="font-size:9px;color:var(--ink-3);padding:6px 14px 4px">Changed files · {{ ag.files.length }}</div>
        @for (f of ag.files; track f.path; let i = $index) {
          <div
            class="diff-file"
            [class.sel]="sel() === i"
            (click)="sel.set(i)"
            style="display:flex;align-items:center;gap:8px;padding:6px 12px;cursor:pointer;margin:1px 6px;border-radius:var(--r-sm)"
          >
            <span
              [style.color]="stateInk(f.state)"
              [style.background]="stateBg(f.state)"
              style="flex:none;width:14px;height:14px;border-radius:3px;display:grid;place-items:center;font-size:9px;font-weight:700"
            >{{ f.state }}</span>
            <div style="flex:1;min-width:0">
              <div style="font-size:11.5px;overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ fname(f.path) }}</div>
              @if (fdir(f.path)) {
                <div style="font-size:9.5px;color:var(--ink-4);overflow:hidden;text-overflow:ellipsis;white-space:nowrap">{{ fdir(f.path) }}</div>
              }
            </div>
            <span class="tnum" style="font-size:9.5px;display:flex;gap:4px;flex:none">
              <span style="color:var(--code-add-ink)">+{{ f.add }}</span>
              @if (f.del > 0) { <span style="color:var(--code-del-ink)">−{{ f.del }}</span> }
            </span>
          </div>
        }
      </div>

      <!-- diff body -->
      <div class="scroll-y" style="background:var(--bg)">
        <div style="position:sticky;top:0;display:flex;align-items:center;gap:8px;padding:8px 14px;background:var(--panel);border-bottom:1px solid var(--hair);font-size:11.5px">
          <app-icon name="file" size="sm" color="var(--ink-3)" />
          <span>{{ diff().file }}</span>
          <span class="chip" style="margin-left:auto;font-size:9.5px">{{ diff().lang }}</span>
        </div>
        @if (diff().hunks.length) {
          @for (h of diff().hunks; track $index) {
            <div style="padding:5px 14px;font-size:11px;color:var(--accent-2);background:color-mix(in oklch,var(--accent-2),transparent 93%)">{{ h.meta }}</div>
            @for (ln of h.lines; track $index) {
              <div
                [style.background]="ln.k === '+' ? 'var(--code-add-bg)' : ln.k === '-' ? 'var(--code-del-bg)' : 'transparent'"
                style="display:flex;font-size:12px;line-height:1.7"
              >
                <span class="tnum" style="width:44px;flex:none;text-align:right;padding:0 10px 0 0;color:var(--ink-4);user-select:none">{{ ln.n }}</span>
                <span [style.color]="ln.k === '+' ? 'var(--code-add-ink)' : ln.k === '-' ? 'var(--code-del-ink)' : 'var(--ink-4)'" style="width:16px;flex:none;text-align:center;user-select:none">{{ ln.k }}</span>
                <span [style.color]="ln.k === '+' ? 'var(--code-add-ink)' : ln.k === '-' ? 'var(--code-del-ink)' : 'var(--ink-2)'" style="flex:1;white-space:pre-wrap;word-break:break-word;padding-right:14px">{{ ln.s || ' ' }}</span>
              </div>
            }
          }
        } @else {
          <div style="padding:30px;text-align:center;color:var(--ink-4);font-size:12px">no diff preview for this file</div>
        }
      </div>
    </div>
  `,
  styles: [
    `
      .diff-file:hover:not(.sel) {
        background: var(--panel-2);
      }
      .diff-file.sel {
        background: var(--panel-3);
      }
    `,
  ],
})
export class DiffViewComponent {
  readonly agent = input.required<Agent>();
  readonly sel = signal(0);

  readonly fname = fileName;
  readonly fdir = fileDir;

  readonly diff = computed<Diff>(() => {
    const ag = this.agent();
    return (
      DIFFS[ag.id] || {
        file: ag.files[0]?.path ?? "",
        lang: "",
        hunks: [],
      }
    );
  });

  stateInk(state: string): string {
    return state === "A" ? "var(--code-add-ink)" : state === "D" ? "var(--code-del-ink)" : "var(--accent-2)";
  }
  stateBg(state: string): string {
    return state === "A" ? "var(--code-add-bg)" : state === "D" ? "var(--code-del-bg)" : mix("var(--accent-2)", 86);
  }
}
