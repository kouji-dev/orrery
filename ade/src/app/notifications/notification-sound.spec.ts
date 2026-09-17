import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { SOUND_OPTIONS } from "../settings/settings.store";
import { playNotificationSound, resetNotificationAudio } from "./notification-sound";

// ---- WebAudio mock (jsdom has no AudioContext) ----
class MockParam {
  setValueAtTime = vi.fn();
  linearRampToValueAtTime = vi.fn();
  exponentialRampToValueAtTime = vi.fn();
}
class MockOscillator {
  type = "";
  frequency = new MockParam();
  start = vi.fn();
  stop = vi.fn();
  connect = vi.fn((node: unknown) => node);
}
class MockGain {
  gain = new MockParam();
  connect = vi.fn((node: unknown) => node);
}

let instances: MockAudioContext[] = [];
class MockAudioContext {
  currentTime = 0;
  state = "running";
  destination = {};
  oscillators: MockOscillator[] = [];
  gains: MockGain[] = [];
  resume = vi.fn(async () => {
    this.state = "running";
  });
  createOscillator = vi.fn(() => {
    const o = new MockOscillator();
    this.oscillators.push(o);
    return o;
  });
  createGain = vi.fn(() => {
    const g = new MockGain();
    this.gains.push(g);
    return g;
  });
  constructor() {
    instances.push(this);
  }
}

const g = globalThis as { AudioContext?: unknown };

beforeEach(() => {
  instances = [];
  g.AudioContext = MockAudioContext;
  resetNotificationAudio();
});
afterEach(() => {
  delete g.AudioContext;
  resetNotificationAudio();
});

const ctx = () => instances[0];

describe("playNotificationSound — synthesized presets", () => {
  it("every settings sound option has a playable preset", () => {
    for (const name of SOUND_OPTIONS) {
      playNotificationSound(name, 70);
    }
    expect(ctx().createOscillator).toHaveBeenCalled();
    // every preset produced at least one started oscillator
    expect(ctx().oscillators.length).toBeGreaterThanOrEqual(SOUND_OPTIONS.length);
    for (const o of ctx().oscillators) expect(o.start).toHaveBeenCalledTimes(1);
  });

  it("presets are DISTINCT — each starts at its own primary frequency", () => {
    const primaries = new Set<number>();
    for (const name of SOUND_OPTIONS) {
      instances = [];
      resetNotificationAudio();
      playNotificationSound(name, 70);
      const first = ctx().oscillators[0];
      primaries.add(first.frequency.setValueAtTime.mock.calls[0][0] as number);
    }
    expect(primaries.size).toBe(SOUND_OPTIONS.length);
  });

  it("Ping is a single tone; Chime layers two", () => {
    playNotificationSound("Ping", 70);
    expect(ctx().oscillators).toHaveLength(1);
    playNotificationSound("Chime", 70);
    expect(ctx().oscillators).toHaveLength(3); // 1 (Ping) + 2 (Chime)
  });

  it("scales the envelope peak from the settings volume (0–100)", () => {
    playNotificationSound("Ping", 100);
    playNotificationSound("Ping", 50);
    const [loud, quiet] = ctx().gains;
    const peakOf = (gain: MockGain) => gain.gain.linearRampToValueAtTime.mock.calls[0][0] as number;
    expect(peakOf(loud)).toBeGreaterThan(0);
    expect(peakOf(quiet)).toBeGreaterThan(0);
    expect(peakOf(quiet)).toBeLessThan(peakOf(loud));
  });

  it("volume 0 plays nothing (no context, no oscillators)", () => {
    playNotificationSound("Ping", 0);
    expect(instances).toHaveLength(0);
  });

  it("unknown sound name is a silent no-op", () => {
    playNotificationSound("Airhorn", 70);
    expect(instances).toHaveLength(0);
  });

  it("reuses ONE shared AudioContext across plays", () => {
    playNotificationSound("Ping", 70);
    playNotificationSound("Pop", 70);
    expect(instances).toHaveLength(1);
  });

  it("resumes a suspended context before playing", () => {
    playNotificationSound("Ping", 70); // create the shared context
    ctx().state = "suspended";
    playNotificationSound("Ping", 70);
    expect(ctx().resume).toHaveBeenCalled();
  });

  it("no AudioContext available (plain jsdom) — silent no-op, no throw", () => {
    delete g.AudioContext;
    resetNotificationAudio();
    expect(() => playNotificationSound("Ping", 70)).not.toThrow();
  });
});
