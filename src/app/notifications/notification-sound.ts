/**
 * WebAudio-synthesized notification cues — five short, distinct envelopes, no
 * audio assets. Names mirror the settings `SOUND_OPTIONS` (settings.store.ts);
 * a parity spec guards the two lists against drifting apart.
 *
 * Everything runs on ONE lazily-created shared AudioContext: a cue is just
 * oscillators + gain ramps, so repeated notifications cost nothing to set up.
 * No WebAudio (plain jsdom / very old WebView) → silent no-op.
 */

export type SoundName = "Ping" | "Chime" | "Pop" | "Glass" | "Submarine";

interface Tone {
  type: OscillatorType;
  /** Start frequency (Hz). */
  freq: number;
  /** Optional glide target (Hz) — the pitch ramps there over the envelope. */
  glide?: number;
  /** Delay before this tone starts, relative to the cue start (s). */
  at: number;
  /** Envelope length (s). */
  dur: number;
  /** Peak level 0..1, relative to the cue's master volume. */
  peak: number;
}

const PRESETS: Record<SoundName, Tone[]> = {
  // one clean high blip
  Ping: [{ type: "sine", freq: 880, at: 0, dur: 0.25, peak: 1 }],
  // two-note bell (E6 → G#6), the second slightly delayed and softer
  Chime: [
    { type: "sine", freq: 659.25, at: 0, dur: 0.5, peak: 0.9 },
    { type: "sine", freq: 830.61, at: 0.12, dur: 0.6, peak: 0.7 },
  ],
  // very short pitched-down thump
  Pop: [{ type: "triangle", freq: 420, glide: 130, at: 0, dur: 0.12, peak: 1 }],
  // bright high partials with a fast shimmer
  Glass: [
    { type: "sine", freq: 1567.98, at: 0, dur: 0.35, peak: 0.8 },
    { type: "sine", freq: 2349.32, at: 0.03, dur: 0.3, peak: 0.5 },
  ],
  // low slow sonar sweep
  Submarine: [{ type: "sine", freq: 220, glide: 110, at: 0, dur: 0.7, peak: 1 }],
};

let shared: AudioContext | null = null;

function context(): AudioContext | null {
  if (shared) return shared;
  const Ctor = (globalThis as { AudioContext?: typeof AudioContext }).AudioContext;
  if (!Ctor) return null; // no WebAudio — cues are silently unavailable
  try {
    shared = new Ctor();
  } catch {
    return null;
  }
  return shared;
}

/** Drop the shared context — tests swap the mocked AudioContext between cases. */
export function resetNotificationAudio(): void {
  shared = null;
  primed = false;
}

let primed = false;

/** Prime the shared context on the FIRST user gesture. WebView2's autoplay
 *  policy keeps an off-gesture AudioContext suspended — without this, the first
 *  cue (e.g. from an auto-resumed agent) is silent. One pair of once-listeners,
 *  self-removing. Idempotent. */
export function primeNotificationAudioOnGesture(doc: Document = document): void {
  if (primed) return;
  primed = true;
  const prime = () => {
    const ctx = context();
    if (ctx?.state === "suspended") void ctx.resume?.()?.catch?.(() => {});
  };
  doc.addEventListener("pointerdown", prime, { once: true, capture: true });
  doc.addEventListener("keydown", prime, { once: true, capture: true });
}

/**
 * Play one synthesized cue. `volume` is the settings 0–100 slider; 0, an
 * unknown name, or no WebAudio plays nothing. The slider is squared into the
 * gain (half slider ≈ quarter power) for a roughly perceptual ramp.
 */
export function playNotificationSound(name: string, volume: number): void {
  const tones = PRESETS[name as SoundName];
  const vol = Math.min(100, Math.max(0, volume)) / 100;
  if (!tones || vol <= 0) return;
  const ctx = context();
  if (!ctx) return;
  // An autoplay-suspended context can't play THIS cue: scheduling into it would
  // make every queued tone burst together on a later resume. Nudge it for the
  // next cue and drop this one (the gesture primer usually prevents this).
  if (ctx.state === "suspended") {
    void ctx.resume?.()?.catch?.(() => {});
    return;
  }
  const t0 = ctx.currentTime;
  const master = vol * vol;
  for (const tone of tones) {
    const osc = ctx.createOscillator();
    const gain = ctx.createGain();
    osc.type = tone.type;
    const start = t0 + tone.at;
    const end = start + tone.dur;
    osc.frequency.setValueAtTime(tone.freq, start);
    if (tone.glide !== undefined) osc.frequency.exponentialRampToValueAtTime(tone.glide, end);
    // fast attack, exponential release — short and click-free
    gain.gain.setValueAtTime(0, start);
    gain.gain.linearRampToValueAtTime(master * tone.peak, start + 0.005);
    gain.gain.exponentialRampToValueAtTime(0.0001, end);
    osc.connect(gain).connect(ctx.destination);
    osc.start(start);
    osc.stop(end + 0.05);
  }
}
