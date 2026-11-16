/**
 * A short two-note chime for "your image is ready", synthesised with Web Audio
 * so it ships no audio asset and works the same on desktop and in browser mode.
 */

let ctx: AudioContext | null = null;
let lastPlayedAt = 0;

export function playFinishChime(): void {
  // A batch or a burst of finishes should ring once, not stack up.
  const now = Date.now();
  if (now - lastPlayedAt < 800) return;
  lastPlayedAt = now;

  try {
    const AudioCtor =
      window.AudioContext ??
      (window as unknown as { webkitAudioContext?: typeof AudioContext }).webkitAudioContext;
    if (!AudioCtor) return;
    ctx ??= new AudioCtor();
    if (ctx.state === "suspended") void ctx.resume();

    const start = ctx.currentTime + 0.01;
    // E6 then A6: bright, short, and not alarming.
    const notes: Array<[number, number]> = [
      [1318.5, 0],
      [1760, 0.12],
    ];
    for (const [freq, offset] of notes) {
      const osc = ctx.createOscillator();
      const gain = ctx.createGain();
      osc.type = "sine";
      osc.frequency.value = freq;
      const t = start + offset;
      gain.gain.setValueAtTime(0.0001, t);
      gain.gain.exponentialRampToValueAtTime(0.18, t + 0.015);
      gain.gain.exponentialRampToValueAtTime(0.0001, t + 0.45);
      osc.connect(gain).connect(ctx.destination);
      osc.start(t);
      osc.stop(t + 0.5);
    }
  } catch (e) {
    console.warn("[finishChime] could not play:", e);
  }
}
