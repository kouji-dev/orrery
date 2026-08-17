import {
  AfterViewInit,
  ChangeDetectionStrategy,
  Component,
  computed,
  ElementRef,
  inject,
  signal,
  viewChild,
} from '@angular/core';
import { Router } from '@angular/router';
import { UpdaterService } from '../updater/updater.service';
import { UpdateOutcome } from '../updater/updater';

@Component({
  selector: 'app-loading',
  changeDetection: ChangeDetectionStrategy.OnPush,
  styles: [`
    /* The one sanctioned home for the brand triad: it runs before any product
       chrome exists. Everything that is not the triad follows the tokens, so
       the splash follows the theme. */
    :host {
      position: fixed;
      inset: 0;
      z-index: 99999;
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      background: var(--bg);
      color: var(--ink);
      font-family: 'JetBrains Mono', ui-monospace, monospace;
      background-image:
        linear-gradient(var(--bg-grid) 1px, transparent 1px),
        linear-gradient(90deg, var(--bg-grid) 1px, transparent 1px),
        radial-gradient(46% 38% at 50% 43%, color-mix(in oklch, var(--ink), transparent 95%), transparent 72%);
      background-size: 34px 34px, 34px 34px, 100% 100%;
    }
    /* the halo is a STATIC sibling behind the SVG, so the 26px blur rasterises
       once instead of being recomputed on every frame the mark redraws */
    .ob-mark { position: relative; }
    .ob-halo {
      position: absolute;
      left: 50%;
      top: 50%;
      width: 62%;
      height: 62%;
      transform: translate(-50%, -50%);
      border-radius: 50%;
      pointer-events: none;
      background: color-mix(in oklch, var(--brand-2), transparent 78%);
      filter: blur(26px);
    }
    .ob-word {
      font-family: 'Space Grotesk', sans-serif;
      font-weight: 600;
      font-size: var(--fs-display);
      letter-spacing: 0.01em;
      color: var(--ink);
      margin-top: var(--sp-10);
    }
    .ob-word .o { color: var(--brand-2); }
    .ob-status {
      font-size: var(--fs-xs);
      letter-spacing: 0.2em;
      text-transform: uppercase;
      color: var(--ink-3);
      margin-top: var(--sp-7);
      height: var(--sp-6);
    }
    .ob-bar {
      margin-top: var(--sp-8);
      width: 188px;
      height: var(--sp-1);
      border-radius: 2px;
      background: var(--hair);
      overflow: hidden;
    }
    /* compositor-only progress: scaleX, never width */
    .ob-bar > i {
      display: block;
      height: 100%;
      width: 100%;
      border-radius: 2px;
      transform-origin: left center;
      will-change: transform;
      background: linear-gradient(90deg, var(--brand-1), var(--brand-2), var(--brand-3));
      transition: transform 0.15s linear;
    }
    .ob-core-pulse {
      transform-box: fill-box;
      transform-origin: 50% 50%;
      animation: ob-bootpulse 2.4s ease-in-out infinite;
    }
    @keyframes ob-bootpulse {
      0%, 100% { opacity: 0.9; }
      50% { opacity: 0.55; }
    }
    /* "Orrery × Kouji.dev" credit — synced with the loading-screen design + landing footer */
    .ob-credit {
      position: fixed;
      bottom: 22px;
      display: flex;
      align-items: center;
      gap: 8px;
      font-family: 'Space Grotesk', sans-serif;
      font-size: 13px;
      font-weight: 600;
      letter-spacing: -0.01em;
      color: var(--ink-2);
    }
    .ob-cred-mark { filter: drop-shadow(0 0 8px color-mix(in oklch, var(--brand-2), transparent 65%)); display: block; }
    .ob-cred-wm .o { color: var(--brand-2); }
    .ob-cred-x { color: var(--ink-4); font-family: 'JetBrains Mono', ui-monospace, monospace; font-size: 12px; margin: 0 1px; }
    .ob-cred-link {
      text-decoration: none;
      cursor: pointer;
      background: linear-gradient(100deg, var(--brand-1), var(--brand-2), var(--brand-3));
      -webkit-background-clip: text;
      background-clip: text;
      color: transparent;
    }
    .ob-cred-link:hover { filter: brightness(1.12); text-decoration: underline; text-underline-offset: 2px; }
  `],
  template: `
    <div class="ob-mark"><span class="ob-halo"></span><svg #svg width="168" height="168" viewBox="0 0 100 100" fill="none"></svg></div>
    <div class="ob-word"><span class="o">O</span>rrery</div>
    <div class="ob-status">{{ updater.status() || 'initializing orchestrator' }}</div>
    <div class="ob-bar"><i [style.transform]="'scaleX(' + barPct() / 100 + ')'"></i></div>
    <div class="ob-credit">
      <svg #creditMark width="18" height="18" viewBox="0 0 100 100" fill="none" class="ob-cred-mark" aria-hidden="true"></svg>
      <span class="ob-cred-wm"><span class="o">O</span>rrery</span>
      <span class="ob-cred-x">×</span>
      <a class="ob-cred-link" href="https://kouji.dev" (click)="openCredit($event)">Kouji.dev</a>
    </div>
  `,
})
export class LoadingComponent implements AfterViewInit {
  readonly updater = inject(UpdaterService);
  private readonly router = inject(Router);

  /** Cosmetic minimum the splash stays up (ms). */
  minMs = 1600;
  /** Hard backstop: navigate onward even if the updater hangs (ms). */
  safetyMs = 12_000;

  private readonly svg = viewChild<ElementRef<SVGSVGElement>>('svg');
  private readonly creditMark = viewChild<ElementRef<SVGSVGElement>>('creditMark');

  /** Intro epicycle sweep, 0..1, advanced over ~2.2s by the draw loop. */
  private readonly introFrac = signal(0);

  /** Bar width %. Reactive so it tracks the download for its full duration:
   *  once a download is underway (progress > 0) the bar follows it; otherwise it
   *  shows the intro sweep. This is bound in the template — NOT written from the
   *  intro rAF — so it never freezes when the intro animation ends. */
  readonly barPct = computed(() => {
    const dl = this.updater.progress();
    return (dl > 0 ? dl : this.introFrac()) * 100;
  });

  ngAfterViewInit() {
    this.draw();
    void this.boot();
  }

  /** Testable boot flow: run the updater (with a cosmetic floor), and unless an
   *  update is installing, route into the app. A safety timeout guarantees we
   *  always move on. */
  async boot(): Promise<void> {
    const outcome = await Promise.race<UpdateOutcome>([
      this.runWithFloor(),
      this.delay(this.safetyMs).then(() => 'no-update' as const),
    ]);
    if (outcome !== 'updating') {
      await this.router.navigateByUrl('/app');
    }
  }

  private async runWithFloor(): Promise<UpdateOutcome> {
    const [outcome] = await Promise.all([this.updater.run(), this.delay(this.minMs)]);
    return outcome;
  }

  private delay(ms: number): Promise<void> {
    return new Promise((r) => setTimeout(r, ms));
  }

  /** Ported epicycle draw from the old index.html splash. Feeds the intro sweep
   *  into `introFrac`; the bar is rendered reactively via `barPct`. */
  private draw(): void {
    const svg = this.svg()?.nativeElement;
    if (!svg) return;
    const A = 22, B = 13, K = -4, TAU = Math.PI * 2;
    const epi = (t: number): [number, number] => [
      50 + A * Math.cos(t) + B * Math.cos(K * t),
      50 + A * Math.sin(t) + B * Math.sin(K * t),
    ];
    const N = 260;
    let d = '';
    for (let i = 0; i <= N; i++) {
      const t = (i / N) * TAU, p = epi(t);
      d += (i ? 'L' : 'M') + p[0].toFixed(2) + ' ' + p[1].toFixed(2) + ' ';
    }
    d += 'Z';
    // small static credit mark (bare epicycle, no tile) for the "Orrery × Kouji.dev" footer
    const cm = this.creditMark()?.nativeElement;
    if (cm) {
      cm.innerHTML =
        '<defs>' +
          '<linearGradient id="cm-g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="var(--brand-1)"/><stop offset=".5" stop-color="var(--brand-2)"/><stop offset="1" stop-color="var(--brand-3)"/></linearGradient>' +
          '<radialGradient id="cm-c"><stop offset="0" stop-color="var(--brand-core)"/><stop offset=".45" stop-color="var(--brand-2)"/><stop offset="1" stop-color="var(--brand-2)" stop-opacity="0"/></radialGradient>' +
        '</defs>' +
        '<path d="' + d + '" fill="none" stroke="url(#cm-g)" stroke-width="6" stroke-linejoin="round"/>' +
        '<circle cx="50" cy="50" r="13" fill="url(#cm-c)"/>';
    }
    svg.innerHTML =
      '<defs>' +
        '<linearGradient id="ob-g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="var(--brand-1)"/><stop offset=".5" stop-color="var(--brand-2)"/><stop offset="1" stop-color="var(--brand-3)"/></linearGradient>' +
        '<radialGradient id="ob-c"><stop offset="0" stop-color="var(--brand-core)"/><stop offset=".42" stop-color="var(--brand-2)"/><stop offset="1" stop-color="var(--brand-2)" stop-opacity="0"/></radialGradient>' +
      '</defs>' +
      '<circle cx="50" cy="50" r="22" fill="none" stroke="var(--ink-4)" stroke-width=".6" stroke-opacity=".3"/>' +
      '<path id="ob-path" d="' + d + '" fill="none" stroke="url(#ob-g)" stroke-width="2.4" stroke-linejoin="round" stroke-linecap="round"/>' +
      '<g id="ob-epi-g"><circle cx="50" cy="50" r="13" fill="none" stroke="var(--ink-4)" stroke-width=".6" stroke-opacity=".4"/></g>' +
      '<line id="ob-arm" stroke="var(--brand-3)" stroke-width="1" stroke-opacity=".6"/>' +
      '<circle class="ob-core-pulse" cx="50" cy="50" r="8.5" fill="url(#ob-c)"/>' +
      '<g id="ob-pen-g"><circle cx="50" cy="50" r="4" fill="var(--brand-3)"/></g>';
    const path = svg.querySelector('#ob-path') as SVGPathElement;
    const epiG = svg.querySelector('#ob-epi-g') as SVGGElement;
    const penG = svg.querySelector('#ob-pen-g') as SVGGElement;
    const arm = svg.querySelector('#ob-arm') as SVGLineElement;
    const reduce = window.matchMedia?.('(prefers-reduced-motion:reduce)').matches;
    const L = path.getTotalLength();
    path.style.strokeDasharray = String(L);
    const finish = () => {
      // the construction scaffolding fades once the curve is drawn
      epiG.style.transition = arm.style.transition = 'opacity .3s';
      epiG.style.opacity = '0';
      arm.style.opacity = '0';
      const s = epi(0);
      penG.setAttribute('transform', `translate(${(s[0] - 50).toFixed(2)},${(s[1] - 50).toFixed(2)})`);
    };
    if (reduce) {
      path.style.strokeDashoffset = '0';
      this.introFrac.set(1);
      finish();
      return;
    }
    path.style.strokeDashoffset = String(L);
    const DUR = 2200;
    const ease = (x: number) => 1 - Math.pow(1 - x, 3);
    let start: number | null = null;
    const frame = (now: number) => {
      // read first, then one batched write pass — the pen and the epicycle each
      // move as a <g> transform, nothing triggers layout
      if (start === null) start = now;
      const p = Math.min(1, (now - start) / DUR), e = ease(p);
      const t = e * TAU, dx = A * Math.cos(t), dy = A * Math.sin(t), pp = epi(t);
      path.style.strokeDashoffset = String(L * (1 - e));
      epiG.setAttribute('transform', `translate(${dx.toFixed(2)},${dy.toFixed(2)})`);
      penG.setAttribute('transform', `translate(${(pp[0] - 50).toFixed(2)},${(pp[1] - 50).toFixed(2)})`);
      arm.setAttribute('x1', (50 + dx).toFixed(2));
      arm.setAttribute('y1', (50 + dy).toFixed(2));
      arm.setAttribute('x2', pp[0].toFixed(2));
      arm.setAttribute('y2', pp[1].toFixed(2));
      this.introFrac.set(e);
      if (p < 1) requestAnimationFrame(frame);
      else finish();
    };
    requestAnimationFrame(frame);
  }

  /** Open the Kouji.dev credit in the user's browser (window.open is blocked in
   *  the Tauri webview, so route through the opener plugin). */
  openCredit(e: Event): void {
    e.preventDefault();
    import('@tauri-apps/plugin-opener')
      .then((m) => m.openUrl('https://kouji.dev'))
      .catch(() => window.open('https://kouji.dev', '_blank'));
  }
}
