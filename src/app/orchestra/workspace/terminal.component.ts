import {
  afterRenderEffect,
  ChangeDetectionStrategy,
  Component,
  DestroyRef,
  ElementRef,
  inject,
  input,
  viewChild,
} from "@angular/core";
import { Agent } from "../models";
import { TerminalService } from "../terminal.service";

/**
 * Hosts the agent's persistent xterm terminal. The Terminal instance lives in
 * TerminalService (survives tab switches); this component just attaches it to
 * the visible host and re-attaches when the shown agent changes.
 */
@Component({
  selector: "app-terminal",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <div style="flex:1;display:flex;flex-direction:column;min-height:0;background:var(--bg)">
      <div style="color:var(--ink-4);padding:8px 14px 4px;font-size:10.5px">── session: {{ agent().worktree }} · {{ agent().branch }} ──</div>
      <div #host style="flex:1;min-height:0;padding:2px 10px 8px"></div>
    </div>
  `,
})
export class TerminalComponent {
  private terminals = inject(TerminalService);
  readonly agent = input.required<Agent>();

  private host = viewChild.required<ElementRef<HTMLDivElement>>("host");
  private detach: (() => void) | null = null;
  private shownId: string | null = null;

  constructor() {
    inject(DestroyRef).onDestroy(() => this.detach?.());
    // (re)attach the persistent terminal after render, whenever the agent changes
    afterRenderEffect(() => {
      const el = this.host().nativeElement;
      const id = this.agent().id;
      if (id === this.shownId) return;
      this.shownId = id;
      this.detach?.();
      this.detach = this.terminals.attach(id, el);
    });
  }
}
