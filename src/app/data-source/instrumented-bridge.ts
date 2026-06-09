import { Bridge } from "./bridge";
import { PerfStore } from "../perf/perf.store";

/**
 * Wraps a {@link Bridge} to time every command round-trip into the
 * {@link PerfStore}. Keeps the underlying `TauriBridge` pure; provided for the
 * `BRIDGE` token in its place. `on` / `pickDirectory` pass straight through.
 */
export class InstrumentedBridge implements Bridge {
  constructor(
    private readonly inner: Bridge,
    private readonly perf: PerfStore,
  ) {}

  async invoke<R>(command: string, payload?: Record<string, unknown>): Promise<R> {
    const start = performance.now();
    try {
      const result = await this.inner.invoke<R>(command, payload);
      this.perf.record(command, performance.now() - start, true);
      return result;
    } catch (e) {
      this.perf.record(command, performance.now() - start, false);
      throw e;
    }
  }

  on<T>(event: string, handler: (payload: T) => void): Promise<() => void> {
    return this.inner.on(event, handler);
  }

  pickDirectory(): Promise<string | null> {
    return this.inner.pickDirectory();
  }
}
