import { beforeEach, describe, expect, it } from "vitest";
import {
  formatBytes,
  formatMinutes,
  formatPlayed,
  roundDuration,
  runLevelText,
  serverTitle,
} from "./format";
import { i18n } from "./i18n.svelte";
import type { ServerRow } from "./api";

beforeEach(() => {
  i18n.preference = "en";
});

const row = (over: Partial<ServerRow>): ServerRow => ({
  address: "ss14s://a.test",
  name: null,
  players: 0,
  soft_max_players: 0,
  tags: [],
  map: null,
  preset: null,
  run_level: null,
  round_start_time: null,
  panic_bunker: null,
  ...over,
});

describe("serverTitle", () => {
  it("falls back to the address for missing or blank names", () => {
    expect(serverTitle(row({ name: "  Station  " }))).toBe("Station");
    expect(serverTitle(row({ name: "   " }))).toBe("ss14s://a.test");
    expect(serverTitle(row({}))).toBe("ss14s://a.test");
  });
});

describe("runLevelText", () => {
  it("names the three run levels and nothing else", () => {
    expect(runLevelText(0)).toBe("Lobby");
    expect(runLevelText(1)).toBe("Round in progress");
    expect(runLevelText(2)).toBe("Round ended");
    expect(runLevelText(7)).toBeNull();
    expect(runLevelText(null)).toBeNull();
  });
});

describe("durations", () => {
  it("formats minutes and hours", () => {
    expect(formatMinutes(0)).toBe("0 min");
    expect(formatMinutes(59)).toBe("59 min");
    expect(formatMinutes(60)).toBe("1 h 00 min");
    expect(formatMinutes(125)).toBe("2 h 05 min");
  });

  it("measures the round from its start", () => {
    const now = Date.parse("2026-10-03T12:00:00Z");
    expect(roundDuration("2026-10-03T11:13:00Z", now)).toBe("47 min");
    expect(roundDuration("2026-10-03T09:55:00Z", now)).toBe("2 h 05 min");
  });

  it("gives nothing for unknown or future starts", () => {
    const now = Date.parse("2026-10-03T12:00:00Z");
    expect(roundDuration(null, now)).toBeNull();
    expect(roundDuration("not a date", now)).toBeNull();
    expect(roundDuration("2026-10-03T12:30:00Z", now)).toBeNull();
  });

  it("never shows zero minutes of play", () => {
    expect(formatPlayed(10)).toBe("1 min");
    expect(formatPlayed(90)).toBe("2 min");
    expect(formatPlayed(3 * 3600 + 25 * 60)).toBe("3 h 25 min");
  });
});

describe("formatBytes", () => {
  it("scales the unit", () => {
    expect(formatBytes(512)).toBe("512 B");
    expect(formatBytes(1536)).toBe("1.5 KB");
    expect(formatBytes(150 * 1024 * 1024)).toBe("150 MB");
    expect(formatBytes(2.5 * 1024 ** 3)).toBe("2.5 GB");
  });
});
