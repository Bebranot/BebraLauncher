// A joke for the launcher's owner: a "sale" in the settings whose timer starts by itself a while
// after launch and runs for a random time. Nothing here touches the network or the disk.

const rand = (min: number, max: number) => min + Math.floor(Math.random() * (max - min + 1));

class Promo {
  /** Seconds left of the sale; null until the timer has started. */
  left = $state<number | null>(null);
  /** The "age check" window is shown. */
  open = $state(false);

  private started = false;

  /** Starts the countdown once: after 3-20 s from launch, for 2-59 minutes. Safe to call again. */
  start() {
    if (this.started) return;
    this.started = true;
    setTimeout(() => this.run(), rand(3, 20) * 1000);
  }

  private run() {
    this.left = rand(2 * 60, 59 * 60 + 59);
    const timer = setInterval(() => {
      if (this.left === null) return;
      this.left -= 1;
      if (this.left <= 0) {
        clearInterval(timer);
        // The sale "ends", then another one starts, as sales do.
        setTimeout(() => this.run(), 4000);
      }
    }, 1000);
  }
}

export const promo = new Promo();

/** `75` -> `00:01:15`. */
export function clock(seconds: number): string {
  const s = Math.max(0, seconds);
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(Math.floor(s / 3600))}:${p(Math.floor((s % 3600) / 60))}:${p(s % 60)}`;
}
