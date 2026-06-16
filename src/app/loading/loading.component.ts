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
    :host {
      position: fixed;
      inset: 0;
      z-index: 99999;
      display: flex;
      flex-direction: column;
      align-items: center;
      justify-content: center;
      background: #07080d;
      color: #e8ebf2;
      font-family: 'JetBrains Mono', ui-monospace, monospace;
      background-image:
        linear-gradient(rgba(255, 255, 255, 0.022) 1px, transparent 1px),
        linear-gradient(90deg, rgba(255, 255, 255, 0.022) 1px, transparent 1px),
        radial-gradient(46% 38% at 50% 43%, rgba(168, 85, 247, 0.16), transparent 72%);
      background-size: 34px 34px, 34px 34px, 100% 100%, 100% 100%;
    }
    .ob-mark { filter: drop-shadow(0 0 26px rgba(168, 85, 247, 0.4)); }
    .ob-word {
      font-family: 'Space Grotesk', sans-serif;
      font-weight: 600;
      font-size: var(--fs-display);
      letter-spacing: 0.01em;
      margin-top: var(--ctl-h);
    }
    .ob-word .o { color: #a855f7; }
    .ob-status {
      font-size: var(--fs-xs);
      letter-spacing: 0.2em;
      text-transform: uppercase;
      color: #6b7488;
      margin-top: var(--sp-7);
      height: var(--sp-6);
    }
    .ob-bar {
      margin-top: var(--sp-8);
      width: 188px;
      height: var(--sp-1);
      border-radius: 2px;
      background: rgba(255, 255, 255, 0.08);
      overflow: hidden;
    }
    .ob-bar > i {
      display: block;
      height: 100%;
      border-radius: 2px;
      background: linear-gradient(90deg, #ff5d9e, #a855f7, #22d3ee);
      transition: width 0.15s linear;
    }
  `],
  template: `
    <div class="ob-mark"><svg #svg width="168" height="168" viewBox="0 0 100 100" fill="none"></svg></div>
    <div class="ob-word"><span class="o">O</span>rrery</div>
    <div class="ob-status">{{ updater.status() || 'initializing orchestrator' }}</div>
    <div class="ob-bar"><i [style.width.%]="barPct()"></i></div>
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
    svg.innerHTML =
      '<defs>' +
        '<linearGradient id="ob-g" x1="0" y1="0" x2="1" y2="1"><stop offset="0" stop-color="#ff5d9e"/><stop offset=".5" stop-color="#a855f7"/><stop offset="1" stop-color="#22d3ee"/></linearGradient>' +
      '</defs>' +
      '<circle cx="50" cy="50" r="22" fill="none" stroke="#6b7488" stroke-width=".6" stroke-opacity=".3"/>' +
      '<path id="ob-path" d="' + d + '" fill="none" stroke="url(#ob-g)" stroke-width="2.4" stroke-linejoin="round" stroke-linecap="round"/>' +
      '<circle cx="50" cy="50" r="8.5" fill="#a855f7" fill-opacity=".25"/>';
    const path = svg.querySelector('#ob-path') as SVGPathElement;
    const reduce = window.matchMedia?.('(prefers-reduced-motion:reduce)').matches;
    const L = path.getTotalLength();
    path.style.strokeDasharray = String(L);
    if (reduce) {
      path.style.strokeDashoffset = '0';
      this.introFrac.set(1);
      return;
    }
    path.style.strokeDashoffset = String(L);
    const DUR = 2200;
    const ease = (x: number) => 1 - Math.pow(1 - x, 3);
    let start: number | null = null;
    const frame = (now: number) => {
      if (start === null) start = now;
      const p = Math.min(1, (now - start) / DUR), e = ease(p);
      path.style.strokeDashoffset = String(L * (1 - e));
      this.introFrac.set(e);
      if (p < 1) requestAnimationFrame(frame);
    };
    requestAnimationFrame(frame);
  }
}
