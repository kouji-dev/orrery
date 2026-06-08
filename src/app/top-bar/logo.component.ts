import { ChangeDetectionStrategy, Component } from "@angular/core";

@Component({
  selector: "app-logo",
  changeDetection: ChangeDetectionStrategy.OnPush,
  template: `
    <svg width="20" height="20" viewBox="0 0 24 24" fill="none" aria-hidden="true" style="display:block">
      <defs>
        <linearGradient id="orrery-lg" x1="0" y1="0" x2="24" y2="24">
          <stop offset="0" stop-color="var(--accent)" />
          <stop offset="1" stop-color="var(--accent-2)" />
        </linearGradient>
      </defs>
      <path d="M12 2l8.5 4.9v9.8L12 21.6 3.5 16.7V6.9L12 2z" stroke="url(#orrery-lg)" stroke-width="1.4" fill="none" />
      <circle cx="12" cy="12" r="2.4" fill="url(#orrery-lg)" />
      <circle cx="12" cy="5.5" r="1.5" fill="var(--accent)" />
      <circle cx="18" cy="15" r="1.5" fill="var(--accent-2)" />
      <circle cx="6" cy="15" r="1.5" fill="var(--accent)" />
      <path d="M12 7.9v1.7M13.9 11l1.9 1.1M10.1 11l-1.9 1.1" stroke="url(#orrery-lg)" stroke-width="1.1" />
    </svg>
  `,
})
export class LogoComponent {}
