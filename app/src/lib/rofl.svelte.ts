// A joke for the launcher's owner: a "sale" in the settings whose timer starts by itself a while
// after launch and runs for a random time. Nothing here touches the network or the disk.

const rand = (min: number, max: number) => min + Math.floor(Math.random() * (max - min + 1));

class Promo {
  /** Seconds left of the sale; null until the timer has started. */
  left = $state<number | null>(null);
  /** The "age check" window is shown. */
  open = $state(false);
  /** Hidden for the rest of the application session once purchased. */
  dismissedUntilRestart = $state(false);
  /** Whether the promo rolled to show during the current Settings view. */
  visible = $state(false);

  /** Called when opening settings: 1-in-5 (20%) roll if not already purchased. */
  checkSettingsVisit() {
    if (this.dismissedUntilRestart) {
      this.visible = false;
      return;
    }
    // 1 in 5 chance
    if (Math.random() < 0.2) {
      this.visible = true;
      this.start();
    } else {
      this.visible = false;
    }
  }

  /** Complete the joke purchase and hide promo until application restart. */
  markBought() {
    this.dismissedUntilRestart = true;
    this.visible = false;
    this.open = false;
    this.stop();
  }

  private started = false;
  /** The one-shot start/restart delay and the countdown, so `stop` can clear them (P2-62). */
  private timeout: ReturnType<typeof setTimeout> | null = null;
  private interval: ReturnType<typeof setInterval> | null = null;

  /** Starts the countdown once: after 3-20 s from launch, for 2-59 minutes. Safe to call again. */
  start() {
    if (this.started) return;
    this.started = true;
    this.timeout = setTimeout(() => this.run(), rand(3, 20) * 1000);
  }

  private run() {
    this.clear();
    this.left = rand(2 * 60, 59 * 60 + 59);
    this.interval = setInterval(() => {
      if (this.left === null) {
        this.clear();
        return;
      }
      this.left -= 1;
      if (this.left <= 0) {
        this.clear();
        // The sale "ends", then another one starts, as sales do.
        this.timeout = setTimeout(() => this.run(), 4000);
      }
    }, 1000);
  }

  private clear() {
    if (this.interval !== null) {
      clearInterval(this.interval);
      this.interval = null;
    }
    if (this.timeout !== null) {
      clearTimeout(this.timeout);
      this.timeout = null;
    }
  }

  /** Stops every timer; a later `start` begins again. */
  stop() {
    this.clear();
    this.started = false;
    this.left = null;
  }
}

export const promo = new Promo();

/** `75` -> `00:01:15`. */
export function clock(seconds: number): string {
  const s = Math.max(0, Math.floor(Number.isFinite(seconds) ? seconds : 0));
  const p = (n: number) => String(n).padStart(2, "0");
  return `${p(Math.floor(s / 3600))}:${p(Math.floor((s % 3600) / 60))}:${p(s % 60)}`;
}

/** `1536` -> `1,50 КБ`; used by the joke "upload" so the numbers look like a real client. */
export function formatBytes(bytes: number): string {
  const n = Math.max(0, Number.isFinite(bytes) ? bytes : 0);
  const units = ["Б", "КБ", "МБ", "ГБ"];
  let value = n;
  let unit = 0;
  while (value >= 1024 && unit < units.length - 1) {
    value /= 1024;
    unit += 1;
  }
  const digits = unit === 0 || value >= 100 ? 0 : value >= 10 ? 1 : 2;
  return `${value.toFixed(digits).replace(".", ",")} ${units[unit]}`;
}

/**
 * How long the joke "upload" should pretend to take. Bigger files take longer, but the wait is
 * clamped so a tiny scan still shows a progress bar and a huge one does not stall the joke.
 */
export function uploadSeconds(bytes: number): number {
  const size = Math.max(0, Number.isFinite(bytes) ? bytes : 0);
  return Math.min(15, Math.max(2.8, size / 600_000));
}
