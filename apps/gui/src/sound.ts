// Procedural sound engine for Dremes buttons.
//
// Every sound is synthesized at runtime with the Web Audio API — no asset
// files, no licenses, works offline. The master volume is controlled by the
// "Sound volume" setting (0.0 muted .. 1.0 max).

export type SoundName =
  | "jet_start"
  | "jet_stop"
  | "super"
  | "stream_start"
  | "stream_stop"
  | "mode"
  | "power_down"
  | "reconnect"
  | "glow"
  | "close"
  | "boost_on"
  | "boost_off"
  | "toggle"
  | "hover"
  | "tick"
  | "page"
  | "save"
  | "reset"
  | "error";

let ctx: AudioContext | null = null;
let master: GainNode | null = null;
let noiseBuf: AudioBuffer | null = null;
let volume = 0.7;

export function soundVolume(): number {
  return volume;
}

export function setSoundVolume(v: number): void {
  volume = Math.min(1, Math.max(0, v));
}

function ac(): AudioContext | null {
  if (typeof window === "undefined") return null;
  const Ctor: typeof AudioContext | undefined =
    window.AudioContext || (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
  if (!Ctor) return null;
  if (!ctx) {
    try {
      ctx = new Ctor();
    } catch {
      return null;
    }
  }
  if (ctx.state === "suspended") void ctx.resume();
  ensureUnlocked();
  return ctx;
}

// Hover events aren't reliable "user gestures" for WebKit's autoplay policy, so
// the context can stay suspended and hover sounds never play. Unlock it on the
// first real click/key/scroll interaction so hovers always sound afterwards.
let unlockBound = false;
function ensureUnlocked(): void {
  if (unlockBound || typeof document === "undefined") return;
  unlockBound = true;
  const resume = () => {
    if (ctx && ctx.state === "suspended") void ctx.resume();
  };
  for (const ev of ["pointerdown", "pointermove", "pointerover", "keydown", "click", "wheel"]) {
    document.addEventListener(ev, resume, { passive: true });
  }
}

function out(c: AudioContext): GainNode {
  if (!master) {
    master = c.createGain();
    master.gain.value = 1;
    master.connect(c.destination);
  }
  return master;
}

function noise(c: AudioContext): AudioBuffer {
  if (noiseBuf) return noiseBuf;
  noiseBuf = c.createBuffer(1, c.sampleRate * 2, c.sampleRate);
  const data = noiseBuf.getChannelData(0);
  let last = 0;
  for (let i = 0; i < data.length; i++) {
    const white = Math.random() * 2 - 1;
    last = (last + 0.06 * white) / 1.06;
    data[i] = last * 3.5;
  }
  return noiseBuf;
}

function note(c: AudioContext, m: AudioNode, type: OscillatorType, freq: number, at: number, dur: number, peak: number) {
  const o = c.createOscillator();
  o.type = type;
  o.frequency.setValueAtTime(freq, at);
  const g = c.createGain();
  g.gain.setValueAtTime(0.0001, at);
  g.gain.linearRampToValueAtTime(peak, at + 0.02);
  g.gain.exponentialRampToValueAtTime(0.0001, at + dur);
  o.connect(g).connect(m);
  o.start(at);
  o.stop(at + dur + 0.03);
}

function sweep(c: AudioContext, m: AudioNode, type: OscillatorType, f0: number, f1: number, at: number, dur: number, peak: number) {
  const o = c.createOscillator();
  o.type = type;
  o.frequency.setValueAtTime(Math.max(1, f0), at);
  o.frequency.exponentialRampToValueAtTime(Math.max(1, f1), at + dur);
  const g = c.createGain();
  g.gain.setValueAtTime(0.0001, at);
  g.gain.linearRampToValueAtTime(peak, at + dur * 0.6);
  g.gain.exponentialRampToValueAtTime(0.0001, at + dur);
  o.connect(g).connect(m);
  o.start(at);
  o.stop(at + dur + 0.03);
}

function whoosh(c: AudioContext, m: AudioNode, f0: number, f1: number, at: number, dur: number, peak: number) {
  const n = c.createBufferSource();
  n.buffer = noise(c);
  n.loop = true;
  const f = c.createBiquadFilter();
  f.type = "lowpass";
  f.frequency.setValueAtTime(Math.max(30, f0), at);
  f.frequency.exponentialRampToValueAtTime(Math.max(30, f1), at + dur);
  const g = c.createGain();
  g.gain.setValueAtTime(0.0001, at);
  g.gain.linearRampToValueAtTime(peak, at + dur * 0.7);
  g.gain.exponentialRampToValueAtTime(0.0001, at + dur);
  n.connect(f).connect(g).connect(m);
  n.start(at);
  n.stop(at + dur + 0.05);
}

const V = () => volume;

function play(name: SoundName): void {
  const c = ac();
  if (!c || volume <= 0) return;
  const m = out(c);
  const v = V();
  const t = c.currentTime;

  switch (name) {
    case "jet_start":
      sweep(c, m, "sawtooth", 50, 170, t, 1.0, 0.3 * v);
      {
        const o = c.createOscillator();
        o.type = "sawtooth";
        o.frequency.setValueAtTime(170, t + 1.0);
        o.frequency.exponentialRampToValueAtTime(120, t + 1.7);
        const g = c.createGain();
        g.gain.setValueAtTime(0.2 * v, t + 1.0);
        g.gain.exponentialRampToValueAtTime(0.0001, t + 2.2);
        o.connect(g).connect(m);
        o.start(t + 1.0);
        o.stop(t + 2.3);
      }
      whoosh(c, m, 90, 2900, t, 1.6, 0.5 * v);
      note(c, m, "triangle", 340, t + 1.65, 0.35, 0.16 * v);
      break;

    case "jet_stop":
      sweep(c, m, "sawtooth", 150, 48, t, 1.3, 0.24 * v);
      whoosh(c, m, 2400, 100, t, 1.3, 0.4 * v);
      sweep(c, m, "sine", 90, 36, t + 0.9, 0.6, 0.14 * v);
      break;

    case "super":
      // Deep boom
      sweep(c, m, "sine", 160, 45, t, 0.9, 0.7 * v);
      // Rising power sweep
      sweep(c, m, "sawtooth", 120, 1900, t, 0.7, 0.22 * v);
      sweep(c, m, "sine", 300, 2600, t, 0.75, 0.18 * v);
      // Tearing noise
      whoosh(c, m, 400, 6200, t, 0.9, 0.5 * v);
      // Shimmering finale arpeggio
      [660, 880, 1320, 1760].forEach((f, i) => note(c, m, "triangle", f, t + 0.6 + i * 0.09, 0.4, 0.18 * v));
      break;

    case "stream_start":
      note(c, m, "triangle", 523, t, 0.5, 0.25 * v);
      note(c, m, "triangle", 784, t + 0.16, 0.6, 0.25 * v);
      whoosh(c, m, 300, 1800, t, 0.4, 0.18 * v);
      break;

    case "stream_stop":
      note(c, m, "triangle", 784, t, 0.5, 0.22 * v);
      note(c, m, "triangle", 523, t + 0.16, 0.6, 0.22 * v);
      whoosh(c, m, 1600, 250, t, 0.35, 0.14 * v);
      break;

    case "mode":
      note(c, m, "triangle", 392, t, 0.6, 0.26 * v);
      note(c, m, "sine", 587, t + 0.12, 0.5, 0.2 * v);
      break;

    case "power_down":
      sweep(c, m, "sine", 220, 48, t, 1.2, 0.24 * v);
      break;

    case "reconnect":
      whoosh(c, m, 200, 2800, t, 0.55, 0.4 * v);
      note(c, m, "triangle", 440, t + 0.4, 0.3, 0.2 * v);
      break;

    case "glow":
      note(c, m, "triangle", 440, t, 0.5, 0.2 * v);
      note(c, m, "triangle", 554, t + 0.05, 0.6, 0.2 * v);
      break;

    case "close":
      sweep(c, m, "sine", 120, 55, t, 0.18, 0.22 * v);
      break;

    case "boost_on":
      sweep(c, m, "square", 420, 1180, t, 0.16, 0.2 * v);
      note(c, m, "triangle", 990, t + 0.12, 0.18, 0.14 * v);
      break;

    case "boost_off":
      sweep(c, m, "square", 880, 420, t, 0.14, 0.16 * v);
      break;

    case "toggle":
      note(c, m, "triangle", 900, t, 0.07, 0.12 * v);
      break;

    case "hover":
      // iPhone keyboard tap: a single, dry, muted wooden-block / plastic
      // click — high transient attack, near-instantaneous decay, no tail,
      // no reverb, no echo. Synthesized from one short damped tone.
      {
        const o = c.createOscillator();
        o.type = "sine";
        o.frequency.setValueAtTime(1050, t);
        o.frequency.exponentialRampToValueAtTime(760, t + 0.018);
        const g = c.createGain();
        g.gain.setValueAtTime(0.10 * v, t);
        g.gain.exponentialRampToValueAtTime(0.0001, t + 0.03);
        o.connect(g).connect(m);
        o.start(t);
        o.stop(t + 0.04);
        // Microscopic contact noise for the plastic/maple character.
        const n = c.createBufferSource();
        n.buffer = noise(c);
        const f = c.createBiquadFilter();
        f.type = "bandpass";
        f.frequency.value = 1500;
        f.Q.value = 1.1;
        const ng = c.createGain();
        ng.gain.setValueAtTime(0.045 * v, t);
        ng.gain.exponentialRampToValueAtTime(0.0001, t + 0.022);
        n.connect(f).connect(ng).connect(m);
        n.start(t);
        n.stop(t + 0.033);
      }
      break;

    case "tick":
      note(c, m, "triangle", 1100, t, 0.05, 0.09 * v);
      break;

    case "page":
      note(c, m, "triangle", 660, t, 0.06, 0.08 * v);
      note(c, m, "triangle", 990, t + 0.05, 0.06, 0.07 * v);
      break;

    case "save":
      note(c, m, "triangle", 523, t, 0.4, 0.2 * v);
      note(c, m, "triangle", 659, t + 0.12, 0.5, 0.2 * v);
      note(c, m, "sine", 784, t + 0.26, 0.5, 0.12 * v);
      break;

    case "reset":
      sweep(c, m, "square", 240, 60, t, 0.35, 0.18 * v);
      note(c, m, "square", 110, t + 0.05, 0.3, 0.12 * v);
      break;

    case "error":
      sweep(c, m, "sawtooth", 150, 120, t, 0.3, 0.2 * v);
      note(c, m, "square", 100, t + 0.05, 0.25, 0.1 * v);
      break;
  }
}

/// Map an engine command + result to the right sound. Failed commands buzz.
export function playCommandSound(command: string, ok: boolean): void {
  if (!ok) {
    play("error");
    return;
  }
  switch (command) {
    case "EngineOn": play("jet_start"); break;
    case "EngineOff": play("jet_stop"); break;
    case "Super": play("super"); break;
    case "StartStreaming": play("stream_start"); break;
    case "StopStreaming": play("stream_stop"); break;
    case "GamingMode": play("mode"); break;
    case "StopWaydroid": play("power_down"); break;
    case "ReconnectToObs": play("reconnect"); break;
    case "OpenObs": play("glow"); break;
    case "CloseObs": play("close"); break;
    case "StreamBoost":
    case "GameBoost":
    case "WifiBoost": play("boost_on"); break;
    default: break;
  }
}

/// Play a named preset at the current volume (used for previews).
export function playSound(name: SoundName): void {
  play(name);
}

/// A small preview tick used by the volume slider in Settings.
export function previewTick(): void {
  play("toggle");
}