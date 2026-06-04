import {
  afterRenderEffect,
  ChangeDetectionStrategy,
  Component,
  computed,
  ElementRef,
  inject,
  input,
  signal,
  viewChild,
} from "@angular/core";
import { CHATS } from "../data";
import { Agent } from "../models";
import { OrchestraStore } from "../orchestra.store";
import { IconComponent } from "../shared/icon.component";
import { mix } from "../utils";

@Component({
  selector: "app-chat",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    @let ag = agent();
    <div style="flex:1;display:flex;flex-direction:column;min-height:0;background:var(--panel-2)">
      <div #scroll class="scroll-y" style="flex:1;padding:16px 18px;display:flex;flex-direction:column;gap:14px">
        @for (m of msgs(); track $index) {
          <div [style.align-items]="m.role === 'user' ? 'flex-end' : 'flex-start'" style="display:flex;flex-direction:column;gap:4px">
            <div class="up" style="display:flex;align-items:center;gap:6px;font-size:9.5px;color:var(--ink-3)">
              @if (m.role === 'agent') { <app-icon name="agent" size="sm" [px]="11" color="var(--accent)" /> }
              {{ m.role === 'user' ? 'You' : m.role === 'agent' ? ag.name : 'system' }}
              <span style="color:var(--ink-4);letter-spacing:0;text-transform:none">{{ m.time }}</span>
            </div>
            <div
              [style.border-top-right-radius]="m.role === 'user' ? '3px' : '11px'"
              [style.border-top-left-radius]="m.role === 'user' ? '11px' : '3px'"
              [style.background]="m.role === 'user' ? mix('var(--accent)', 86) : m.role === 'sys' ? 'transparent' : 'var(--panel)'"
              [style.color]="m.role === 'sys' ? 'var(--ink-3)' : 'var(--ink)'"
              style="max-width:76%;padding:10px 13px;font-size:12.5px;line-height:1.5;text-wrap:pretty;border-radius:11px;border:1px solid var(--hair)"
            >{{ m.s }}</div>
            @if (m.decision && ag.status === 'blocked') {
              <div style="display:flex;gap:8px;margin-top:4px">
                <button class="btn primary" (click)="store.resolve(ag.id, 'Redis')">Use Redis</button>
                <button class="btn ghost-hair" (click)="store.resolve(ag.id, 'Postgres')">Use Postgres</button>
              </div>
            }
          </div>
        }
      </div>

      <!-- composer -->
      <div style="padding:12px;border-top:1px solid var(--hair);background:var(--panel)">
        <div style="display:flex;gap:8px;align-items:flex-end;padding:8px;background:var(--panel-2);border:1px solid var(--hair);border-radius:var(--r-md)">
          <textarea
            [value]="draft()"
            (input)="draft.set($any($event.target).value)"
            (keydown)="onKey($event)"
            rows="1"
            [placeholder]="'message ' + ag.name + '…'"
            style="flex:1;resize:none;background:transparent;border:none;outline:none;color:var(--ink);font-family:var(--font-mono);font-size:12.5px;line-height:1.5;max-height:120px"
          ></textarea>
          <button class="btn primary" style="padding:6px 11px" (click)="send()"><app-icon name="chevron" size="sm" /></button>
        </div>
      </div>
    </div>
  `,
})
export class ChatComponent {
  readonly store = inject(OrchestraStore);
  readonly agent = input.required<Agent>();
  readonly draft = signal("");
  readonly mix = mix;

  readonly msgs = computed(() => CHATS[this.agent().id] || []);
  private scrollEl = viewChild<ElementRef<HTMLDivElement>>("scroll");

  constructor() {
    afterRenderEffect(() => {
      this.agent();
      const el = this.scrollEl()?.nativeElement;
      if (el) el.scrollTop = el.scrollHeight;
    });
  }

  onKey(e: KeyboardEvent) {
    if (e.key === "Enter" && !e.shiftKey) {
      e.preventDefault();
      this.send();
    }
  }
  send() {
    const v = this.draft().trim();
    if (v) {
      this.store.resolve(this.agent().id, v);
      this.draft.set("");
    }
  }
}
