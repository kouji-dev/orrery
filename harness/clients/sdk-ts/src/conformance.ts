/**
 * The fixture runner, shared by every TypeScript client.
 *
 * `sdk-ts` runs these and so does Ink (plan 09c). The **same files** the Rust
 * SDK runs: a scenario only one language checks is a scenario that has stopped
 * being a contract.
 */

import { readFileSync, readdirSync } from "node:fs";
import { basename, dirname, join } from "node:path";
import { fileURLToPath } from "node:url";

import type { Frame } from "./events.js";
import { type StoreState, SurfaceStore } from "./store.js";

/** One line of a scenario. */
export type Step =
  | { kind: "event"; frame: Frame }
  /** A frame this build cannot even parse. It still consumes its `seq`. */
  | { kind: "unknown"; seq: number; what: string }
  | { kind: "expect"; state: StoreState };

/** One fixture file. */
export interface Scenario {
  name: string;
  path: string;
  steps: Step[];
}

/** Every event `type` this build understands. */
const KNOWN = new Set([
  "RUN_STARTED",
  "RUN_FINISHED",
  "RUN_ERROR",
  "STEP_STARTED",
  "STEP_FINISHED",
  "TEXT_MESSAGE_START",
  "TEXT_MESSAGE_CONTENT",
  "TEXT_MESSAGE_END",
  "TOOL_CALL_START",
  "TOOL_CALL_ARGS",
  "TOOL_CALL_END",
  "TOOL_CALL_RESULT",
  "STATE_SNAPSHOT",
  "STATE_DELTA",
  "CUSTOM",
]);

/** Where the fixtures live, relative to this package. */
export function fixturesDir(): string {
  return join(dirname(fileURLToPath(import.meta.url)), "../../conformance");
}

/** Load one scenario file. Throws naming the file and the line. */
export function load(path: string): Scenario {
  const steps: Step[] = [];
  const text = readFileSync(path, "utf8");
  text.split(/\r?\n/).forEach((raw, index) => {
    const line = raw.trim();
    if (!line || line.startsWith("#")) return;
    let value: Record<string, unknown>;
    try {
      value = JSON.parse(line) as Record<string, unknown>;
    } catch (e) {
      throw new Error(`${path}:${index + 1}: ${(e as Error).message}`);
    }
    if ("ev" in value) {
      const ev = value["ev"] as Frame & { type?: string };
      if (typeof ev.seq !== "number") {
        throw new Error(`${path}:${index + 1}: an event with no seq`);
      }
      steps.push(
        KNOWN.has(ev.type ?? "")
          ? { kind: "event", frame: ev }
          : { kind: "unknown", seq: ev.seq, what: ev.type ?? "?" },
      );
    } else if ("expect" in value) {
      steps.push({ kind: "expect", state: value["expect"] as StoreState });
    } else {
      throw new Error(`${path}:${index + 1}: expected \`ev\` or \`expect\``);
    }
  });
  const name = basename(path).replace(/\.jsonl$/, "");
  return { name, path, steps };
}

/** Load every `*.jsonl` scenario in a directory, sorted by name. */
export function loadAll(dir: string = fixturesDir()): Scenario[] {
  return readdirSync(dir)
    .filter((f) => f.endsWith(".jsonl"))
    .sort()
    .map((f) => load(join(dir, f)));
}

/** Every checkpoint in a scenario, with the store as it stood at each. */
export function checkpoints(
  scenario: Scenario,
): Array<{ index: number; actual: StoreState; expected: StoreState }> {
  const store = new SurfaceStore();
  const out: Array<{ index: number; actual: StoreState; expected: StoreState }> = [];
  for (const step of scenario.steps) {
    if (step.kind === "event") store.apply(step.frame);
    else if (step.kind === "unknown") store.applyUnknown(step.seq, step.what);
    else {
      out.push({
        index: out.length + 1,
        actual: structuredClone(store.state()),
        expected: step.state,
      });
    }
  }
  return out;
}

/** Replay a scenario and hand back the store it produced. */
export function replay(scenario: Scenario): SurfaceStore {
  const store = new SurfaceStore();
  for (const step of scenario.steps) {
    if (step.kind === "event") store.apply(step.frame);
    else if (step.kind === "unknown") store.applyUnknown(step.seq, step.what);
  }
  return store;
}
