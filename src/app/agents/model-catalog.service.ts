import { inject, Injectable, signal } from "@angular/core";
import { Bridge, BRIDGE, Commands } from "../data-source/bridge";

/**
 * Runtime model catalog for the tools that can enumerate their own `--model`
 * vocabulary (backend `list_tool_models`): **pi** (`pi --list-models`, BYOK
 * across ~20 providers, ids in the `provider/model` spelling its `--model`
 * documents) and **cursor** (`cursor-agent models`, the signed-in account's
 * pool, one bare slug per line). claude / codex / gemini have no listing
 * command and never reach this service — see `AgentTool.dynamicModels`.
 *
 * STALE-WHILE-REVALIDATE. The spawn modal re-probes on EVERY open (the account
 * pool and the configured API keys change under us), but the last known list
 * keeps rendering until the new one lands: no empty picker, no blocking, no
 * flicker. `load()` is the once-per-session form; `refresh()` forces the
 * re-probe. Concurrent calls are de-duped — one shell-out per tool in flight.
 *
 * A FAILED probe never clobbers a good list: the previous entry stands and the
 * caller falls back to the tool's curated catalog when there is none. An
 * EMPTY-but-successful probe is truth (signed out, no keys) and is recorded as
 * such, which is what sends the picker to the curated fallback.
 */
@Injectable({ providedIn: "root" })
export class ModelCatalogService {
  private readonly bridge = inject<Bridge>(BRIDGE);

  private readonly catalog = signal<Record<string, string[]>>({});
  /** Tools whose probe has resolved (empty list included) — stops a re-probe. */
  private readonly probed = new Set<string>();
  /** Tools with a probe in flight — de-dupes concurrent `load` calls. */
  private readonly inflight = new Set<string>();

  /** The discovered `--model` ids for `tool`; `[]` until the probe resolves.
   *  Reads a signal, so a caller's computed re-runs when the probe lands. */
  models(tool: string): string[] {
    return this.catalog()[tool] ?? [];
  }

  /** Has `tool`'s CURRENT probe resolved? False while a first probe or a
   *  refresh is in flight — drives the picker's "discovering…" copy, while
   *  `models()` keeps returning the previous list. */
  isProbed(tool: string): boolean {
    return this.probed.has(tool);
  }

  /** Probe `tool` once per session. No-op for a tool already probed or in
   *  flight; safe to call on every spawn-modal open / tool switch. */
  load(tool: string): void {
    if (this.probed.has(tool) || this.inflight.has(tool)) return;
    void this.probe(tool);
  }

  /** Force a re-probe — the spawn modal calls this on every open so the list is
   *  never stale. The cached list stays readable throughout. */
  refresh(tool: string): void {
    this.probed.delete(tool);
    this.load(tool);
  }

  private async probe(tool: string): Promise<void> {
    this.inflight.add(tool);
    try {
      const models = (await this.bridge.invoke<string[]>(Commands.ListToolModels, { id: tool })) ?? [];
      // success — including a legitimately EMPTY pool, which is what routes the
      // picker to the tool's curated fallback.
      this.catalog.update((m) => ({ ...m, [tool]: models }));
    } catch {
      // not installed / no listing command / the CLI failed. Leave whatever we
      // already knew in place; absent an entry, `models()` reports [] and the
      // caller falls back to the curated catalog (or stays free-text).
    } finally {
      this.inflight.delete(tool);
      this.probed.add(tool);
    }
  }
}
