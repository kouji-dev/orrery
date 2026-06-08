import { Injector, runInInjectionContext } from '@angular/core';
import { Router } from '@angular/router';
import { describe, expect, it, vi } from 'vitest';
import { UpdateOutcome } from '../updater/updater';
import { UpdaterService } from '../updater/updater.service';
import { LoadingComponent } from './loading.component';

function make(outcome: UpdateOutcome | 'pending') {
  const navigateByUrl = vi.fn(async () => true);
  const run = vi.fn(
    (): Promise<UpdateOutcome> =>
      outcome === 'pending' ? new Promise<UpdateOutcome>(() => {}) : Promise.resolve(outcome),
  );
  const injector = Injector.create({
    providers: [
      { provide: Router, useValue: { navigateByUrl } },
      { provide: UpdaterService, useValue: { run, status: () => '', progress: () => 0 } },
    ],
  });
  const cmp = runInInjectionContext(injector, () => new LoadingComponent());
  cmp.minMs = 0;
  cmp.safetyMs = 50_000;
  return { cmp, navigateByUrl };
}

describe('LoadingComponent.boot', () => {
  it('navigates to /app when there is no update', async () => {
    const { cmp, navigateByUrl } = make('no-update');
    await cmp.boot();
    expect(navigateByUrl).toHaveBeenCalledWith('/app');
  });

  it('does NOT navigate when an update is installing (it will relaunch)', async () => {
    const { cmp, navigateByUrl } = make('updating');
    await cmp.boot();
    expect(navigateByUrl).not.toHaveBeenCalled();
  });

  it('navigates anyway if the check hangs past the safety timeout', async () => {
    const { cmp, navigateByUrl } = make('pending');
    cmp.safetyMs = 0; // timeout fires immediately
    await cmp.boot();
    expect(navigateByUrl).toHaveBeenCalledWith('/app');
  });
});
