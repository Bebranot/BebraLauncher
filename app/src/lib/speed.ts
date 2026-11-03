/**
 * Download speed from a growing byte counter, in bits per second.
 *
 * Progress arrives in bursts (one event per file, or a few a second), so a single pair of samples
 * would jump around. The meter keeps the last few seconds of samples and measures the rate over
 * that window instead.
 */

/** How much history the rate is measured over. */
const WINDOW_MS = 3000;
/** No new sample for this long means the transfer has stalled: the speed drops to zero. */
const STALE_MS = 2500;
/** Too short a window divides by a near-zero time and produces nonsense. */
const MIN_WINDOW_MS = 200;

interface Sample {
  at: number;
  bytes: number;
}

export class SpeedMeter {
  private samples: Sample[] = [];

  /** Forgets the history, e.g. when a new download or stage starts. */
  reset(): void {
    this.samples = [];
  }

  /** Records the byte counter and returns the current speed in bits per second. */
  sample(bytes: number, at = Date.now()): number {
    if (!Number.isFinite(bytes)) return this.rate(at);
    const last = this.samples[this.samples.length - 1];
    // A counter that went backwards is a new transfer, not a negative speed.
    if (last && bytes < last.bytes) this.samples = [];
    this.samples.push({ at, bytes });
    while (this.samples.length > 2 && at - this.samples[0].at > WINDOW_MS) this.samples.shift();
    return this.rate(at);
  }

  /** The speed for the samples already collected, without adding a new one. */
  rate(at = Date.now()): number {
    const last = this.samples[this.samples.length - 1];
    if (!last || at - last.at > STALE_MS) return 0;
    const first = this.samples[0];
    const window = last.at - first.at;
    if (window < MIN_WINDOW_MS) return 0;
    const gained = last.bytes - first.bytes;
    if (gained <= 0) return 0;
    return (gained * 8 * 1000) / window;
  }
}
