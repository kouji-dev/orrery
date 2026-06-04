import {
  afterRenderEffect,
  ChangeDetectionStrategy,
  Component,
  effect,
  ElementRef,
  HostListener,
  inject,
  signal,
  viewChild,
} from "@angular/core";
import { OrchestraStore } from "../orchestra.store";
import { IconComponent } from "../shared/icon.component";
import { MenuItem } from "../models";

@Component({
  selector: "app-context-menu",
  changeDetection: ChangeDetectionStrategy.OnPush,
  imports: [IconComponent],
  template: `
    @let menu = store.contextMenu();
    @if (menu) {
      <div
        #box
        class="rise"
        [style.left.px]="pos().x"
        [style.top.px]="pos().y"
        style="position:fixed;z-index:80;min-width:196px;background:var(--elev);border:1px solid var(--hair-2);border-radius:var(--r-md);box-shadow:var(--shadow);padding:5px"
      >
        @for (it of menu.items; track $index) {
          @if (it.sep) {
            <div style="height:1px;background:var(--hair);margin:5px 6px"></div>
          } @else {
            <button
              class="menu-item"
              [class.danger]="it.danger"
              [disabled]="it.disabled"
              (click)="run(it)"
              [style.color]="it.disabled ? 'var(--ink-4)' : it.danger ? 'var(--st-blocked)' : 'var(--ink-2)'"
              [style.cursor]="it.disabled ? 'default' : 'pointer'"
              style="display:flex;align-items:center;gap:9px;width:100%;text-align:left;padding:6px 9px;border-radius:6px;border:none;background:transparent;font-family:var(--font-mono);font-size:12px"
            >
              @if (it.icon) {
                <app-icon [name]="it.icon" size="sm" [color]="it.danger ? 'var(--st-blocked)' : (it.accent || 'var(--ink-3)')" style="flex:none" />
              }
              <span style="flex:1">{{ it.label }}</span>
              @if (it.kbd) { <span class="kbd">{{ it.kbd }}</span> }
            </button>
          }
        }
      </div>
    }
  `,
  styles: [
    `
      .menu-item:not(:disabled):hover {
        background: var(--panel-3);
      }
      .menu-item.danger:not(:disabled):hover {
        background: color-mix(in oklch, var(--st-blocked), transparent 88%);
      }
    `,
  ],
})
export class ContextMenuComponent {
  readonly store = inject(OrchestraStore);
  readonly pos = signal<{ x: number; y: number }>({ x: 0, y: 0 });
  private box = viewChild<ElementRef<HTMLDivElement>>("box");

  constructor() {
    // seed position from the requested click point whenever a menu opens
    effect(() => {
      const m = this.store.contextMenu();
      if (m) this.pos.set({ x: m.x, y: m.y });
    });
    // clamp into the viewport after the menu is laid out
    afterRenderEffect(() => {
      const m = this.store.contextMenu();
      const el = this.box()?.nativeElement;
      if (!m || !el) return;
      const r = el.getBoundingClientRect();
      let nx = m.x,
        ny = m.y;
      if (m.x + r.width > window.innerWidth - 8) nx = window.innerWidth - r.width - 8;
      if (m.y + r.height > window.innerHeight - 8) ny = window.innerHeight - r.height - 8;
      const cur = this.pos();
      if (cur.x !== nx || cur.y !== ny) this.pos.set({ x: nx, y: ny });
    });
  }

  run(it: MenuItem) {
    if (it.disabled) return;
    it.onClick?.();
    this.store.closeMenu();
  }

  @HostListener("document:mousedown", ["$event"])
  onDown(e: MouseEvent) {
    if (!this.store.contextMenu()) return;
    const el = this.box()?.nativeElement;
    if (el && !el.contains(e.target as Node)) this.store.closeMenu();
  }

  @HostListener("document:keydown.escape")
  onEsc() {
    this.store.closeMenu();
  }
}
