import { inject, Injectable, signal } from "@angular/core";
import { Bridge, BRIDGE, Commands } from "../data-source/bridge";
import { ToolDetection } from "../models";

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
  /** Last probe FAILURE per tool ("" entry cleared on success). Discarding it —
   *  as this service used to — collapsed three unrelated situations (CLI absent,
   *  probe blew up, CLI fine but signed out) into one silent empty picker, which
   *  is exactly what left the pi model list unexplainable. */
  private readonly failures = signal<Record<string, string>>({});
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

  /** Why `tool`'s last probe failed, or null when it succeeded (an empty-but-
   *  successful list is NOT a failure). Reads a signal, so the hint re-renders
   *  when a re-probe changes the answer. */
  error(tool: string): string | null {
    return this.failures()[tool] ?? null;
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
      this.failures.update((m) => {
        if (!(tool in m)) return m;
        const { [tool]: _fixed, ...rest } = m;
        return rest;
      });
    } catch (e) {
      // not installed / no listing command / the CLI failed. Leave whatever we
      // already knew in place; absent an entry, `models()` reports [] and the
      // caller falls back to the curated catalog (or stays free-text). The
      // MESSAGE is kept: it is the only thing that tells a signed-out CLI apart
      // from a missing one once the picker comes back empty.
      this.failures.update((m) => ({ ...m, [tool]: errorText(e) }));
    } finally {
      this.inflight.delete(tool);
      this.probed.add(tool);
    }
  }
}

/** Tauri rejects with a plain string, a real `Error`, or a `{message}` bag —
 *  flatten all three to one line, since this text lands in a picker footer. */
function errorText(e: unknown): string {
  const raw =
    typeof e === "string" ? e : ((e as { message?: string } | null)?.message ?? String(e));
  return raw.replace(/\s+/g, " ").trim() || "the probe failed";
}

/** Trailing sentence every hint below ends with — the picker stays free-text
 *  in ALL of these cases, and that escape hatch is the actionable part. */
const FREE_TEXT = "Enter applies the typed id.";

/**
 * One line explaining why a `dynamicModels` picker has nothing to offer. The
 * four outcomes were indistinguishable before — same empty combobox whether the
 * CLI was missing, the probe exploded, or the user simply wasn't signed in —
 * and the user read every one of them as "Orrery is broken".
 *
 * Shared by the spawn modal and Settings → Agent defaults so the two can't
 * drift into telling different stories about the same probe.
 */
export function modelDiscoveryHint(
  tool: { id: string; name: string },
  state: {
    probed: boolean;
    error: string | null;
    empty: boolean;
    detection: ToolDetection | null;
  },
): string {
  if (!state.probed) return `Asking ${tool.name} for its models… ${FREE_TEXT}`;
  if (state.error) {
    // A missing binary is not a malfunction — name it as an install gap, or the
    // raw "program not found" spew gets read as a bug in Orrery.
    if (state.detection?.status === "missing") {
      return `${tool.name} isn’t installed, so it has no models to list. ${FREE_TEXT}`;
    }
    return `Couldn’t read ${tool.name}’s model list — ${state.error}. ${FREE_TEXT}`;
  }
  if (state.empty) {
    // VERIFIED against @earendil-works/pi-coding-agent 0.85.1: `pi --list-models`
    // EXITS 0 and prints "No models available. Use /login to log into a provider
    // via OAuth or API key." when nothing is authenticated. Exit 0 + empty is pi
    // saying "signed out", never "broken" — say so in pi's own words.
    if (tool.id === "pi") {
      return `pi ran, but no provider is signed in — run /login inside pi (OAuth or an API key) to get a model list. ${FREE_TEXT}`;
    }
    return `${tool.name} reported no models — check its sign-in or API keys. ${FREE_TEXT}`;
  }
  return `No match — ${FREE_TEXT}`;
}
