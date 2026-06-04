import {
  afterRenderEffect,
  ChangeDetectionStrategy,
  Component,
  ElementRef,
  input,
  viewChild,
} from "@angular/core";
import { Agent, LogLine } from "../models";
import { logColor, logPrefix } from "../utils";

@Component({
  selector: "app-terminal",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div #scroll class="scroll-y" style="flex:1;background:var(--bg);padding:12px 16px;font-size:12px;line-height:1.7">
      <div style="color:var(--ink-4);margin-bottom:8px;font-size:10.5px">── session: {{ agent().worktree }} · {{ agent().branch }} ──</div>
      @for (l of lines(); track $index) {
        <div [style.color]="lc(l.t)" style="display:flex;gap:9px;white-space:pre-wrap;word-break:break-word">
          <span [style.color]="l.t === 'cmd' ? 'var(--accent)' : 'var(--ink-4)'" style="flex:none;user-select:none;width:10px;text-align:center">{{ lp(l.t) }}</span>
          <span style="flex:1">{{ l.s }}</span>
        </div>
      }
      @if (streaming()) {
        <div style="display:flex;gap:9px;color:var(--accent)">
          <span style="width:10px;text-align:center;color:var(--accent)">$</span>
          <span class="caret"></span>
        </div>
      }
    </div>
  `,
})
export class TerminalComponent {
  readonly agent = input.required<Agent>();
  readonly lines = input.required<LogLine[]>();
  readonly streaming = input<boolean>(false);

  readonly lc = logColor;
  readonly lp = logPrefix;

  private scrollEl = viewChild<ElementRef<HTMLDivElement>>("scroll");

  constructor() {
    // auto-scroll to bottom whenever new log lines stream in
    afterRenderEffect(() => {
      this.lines(); // track
      const el = this.scrollEl()?.nativeElement;
      if (el) el.scrollTop = el.scrollHeight;
    });
  }
}
