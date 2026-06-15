import {
  ChangeDetectionStrategy,
  Component,
  input,
  ViewEncapsulation,
} from "@angular/core";

/**
 * Read-only render of stored rich HTML (ticket notes preview / posted comments).
 *
 * Renders via Angular's `[innerHTML]`, which runs the Dom sanitizer
 * automatically — we intentionally do NOT bypass it, so any script/handler
 * markup in stored HTML is stripped.
 *
 *   <app-rich-view [html]="ticket.notes" />
 */
@Component({
  selector: "app-rich-view",
  changeDetection: ChangeDetectionStrategy.OnPush,
  // Content comes from [innerHTML] (no Angular scoping attribute), so emulated
  // encapsulation would never style it. None keeps the host rule global; the
  // `.rte-view` content typography lives in src/styles.css, shared with the
  // editor body (.rte-content) so both render stored markup identically.
  encapsulation: ViewEncapsulation.None,
  template: `<div class="rte-view" [class.compact]="compact()" [innerHTML]="html()"></div>`,
  styles: [
    `
    app-rich-view { display: block; }
    `,
  ],
})
export class RichViewComponent {
  readonly html = input<string>("");
  readonly compact = input<boolean>(false);
}
