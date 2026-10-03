import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import { clock, promo } from "./rofl.svelte";

describe("clock", () => {
  it("shows seconds as hours:minutes:seconds", () => {
    expect(clock(75)).toBe("00:01:15");
    expect(clock(3599)).toBe("00:59:59");
    expect(clock(3600 + 61)).toBe("01:01:01");
    expect(clock(-5)).toBe("00:00:00");
  });
});

describe("the joke sale's timer", () => {
  beforeEach(() => vi.useFakeTimers());
  afterEach(() => vi.useRealTimers());

  it("starts by itself shortly after launch, counts down, and starts over when it ends", () => {
    expect(promo.left).toBeNull();
    promo.start();
    promo.start(); // a second call changes nothing
    expect(promo.left).toBeNull();

    vi.advanceTimersByTime(21_000); // the longest wait before the start
    const first = promo.left;
    expect(first).not.toBeNull();
    expect(first!).toBeGreaterThanOrEqual(120);
    expect(first!).toBeLessThanOrEqual(3599);

    vi.advanceTimersByTime(5_000);
    expect(promo.left).toBe(first! - 5);

    // when it runs out it says so, then a new sale begins
    vi.advanceTimersByTime((first! - 5) * 1000);
    expect(promo.left).toBe(0);
    vi.advanceTimersByTime(4_100);
    expect(promo.left!).toBeGreaterThan(0);
  });
});
