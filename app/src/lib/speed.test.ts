import { describe, expect, it } from "vitest";
import { SpeedMeter } from "./speed";

describe("SpeedMeter", () => {
  it("measures the rate over the sample window, in bits per second", () => {
    const meter = new SpeedMeter();
    meter.sample(0, 0);
    meter.sample(1_000_000, 1000); // 1 MB in 1 s = 8 Mbit/s
    expect(meter.rate(1000)).toBeCloseTo(8_000_000, -1);
  });

  it("needs a window before it claims a speed", () => {
    const meter = new SpeedMeter();
    expect(meter.sample(0, 0)).toBe(0);
    expect(meter.sample(10_000, 100)).toBe(0);
  });

  it("drops to zero when the transfer stalls", () => {
    const meter = new SpeedMeter();
    meter.sample(0, 0);
    meter.sample(1_000_000, 1000);
    expect(meter.rate(1500)).toBeGreaterThan(0);
    expect(meter.rate(5000)).toBe(0);
  });

  it("starts over when the counter goes backwards", () => {
    const meter = new SpeedMeter();
    meter.sample(0, 0);
    meter.sample(1_000_000, 1000);
    expect(meter.sample(0, 1100)).toBe(0);
    expect(meter.sample(1_000_000, 2100)).toBeCloseTo(8_000_000, -1);
  });

  it("forgets the history on reset", () => {
    const meter = new SpeedMeter();
    meter.sample(0, 0);
    meter.sample(1_000_000, 1000);
    meter.reset();
    expect(meter.rate(1000)).toBe(0);
  });

  it("ignores garbage counters instead of showing a made-up speed", () => {
    const meter = new SpeedMeter();
    meter.sample(0, 0);
    meter.sample(Number.NaN, 1000);
    expect(meter.rate(1000)).toBe(0);
  });
});
