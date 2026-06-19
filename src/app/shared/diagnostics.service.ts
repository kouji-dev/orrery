import { inject, Injectable } from "@angular/core";
import { BRIDGE, Commands } from "../data-source/bridge";
import { UiStore } from "../ui/ui.store";

/** App diagnostics actions shared across surfaces (Settings + the status bar). */
@Injectable({ providedIn: "root" })
export class DiagnosticsService {
  private readonly bridge = inject(BRIDGE);
  private readonly ui = inject(UiStore);

  /** Reveal the rolling diagnostics log (app_log_dir/orrery.log) in the OS handler. */
  openLog(): void {
    void this.bridge.invoke(Commands.OpenLog).catch(() => this.ui.flash("couldn't open log file"));
  }
}
