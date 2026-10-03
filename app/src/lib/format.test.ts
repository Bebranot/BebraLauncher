import { beforeEach, describe, expect, it } from "vitest";
import type { ServerRow } from "./api";
import {
  formatBytes,
  formatClock,
  formatMinutes,
  formatPlayed,
  gridTemplate,
  pingClass,
  playersText,
  roundText,
  serverTitle,
} from "./format";
import { i18n } from "./i18n.svelte";

beforeEach(() => {
  i18n.preference = "en";
});

const row = (over: Partial<ServerRow> = {}): ServerRow => ({
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
    expect(serverTitle(row())).toBe("ss14s://a.test");
  });
});

describe("round column", () => {
  const now = Date.parse("2026-10-03T12:00:00Z");

  it("shows how long a running round has been going", () => {
    const s = row({ run_level: 1, round_start_time: "2026-10-03T10:54:55Z" });
    expect(roundText(s, now)).toBe("1:05:05");
  });

  it("names the lobby and the end of the round", () => {
    expect(roundText(row({ run_level: 0 }), now)).toBe("Lobby");
    expect(roundText(row({ run_level: 2 }), now)).toBe("Round ended");
  });

  it("says nothing when the server does not tell", () => {
    expect(roundText(row(), now)).toBeNull();
    expect(roundText(row({ run_level: 1 }), now)).toBeNull();
    expect(roundText(row({ run_level: 1, round_start_time: "nonsense" }), now)).toBeNull();
    expect(roundText(row({ run_level: 1, round_start_time: "2026-10-03T13:00:00Z" }), now)).toBeNull();
  });

  it("clock format", () => {
    expect(formatClock(0)).toBe("0:00:00");
    expect(formatClock(61)).toBe("0:01:01");
    expect(formatClock(3 * 3600 + 5)).toBe("3:00:05");
    expect(formatClock(-4)).toBe("0:00:00");
  });
});

describe("players and ping", () => {
  it("shows the limit, or infinity without one", () => {
    expect(playersText(row({ players: 12, soft_max_players: 80 }))).toBe("12 / 80");
    expect(playersText(row({ players: 3, soft_max_players: 0 }))).toBe("3 / ∞");
  });

  it("ping thresholds", () => {
    expect(pingClass(null)).toBe("unknown");
    expect(pingClass(undefined)).toBe("unknown");
    expect(pingClass(0)).toBe("good");
    expect(pingClass(120)).toBe("good");
    expect(pingClass(121)).toBe("mid");
    expect(pingClass(220)).toBe("mid");
    expect(pingClass(221)).toBe("bad");
  });
});

describe("table columns", () => {
  const all = { round_time: true, players: true, map: true, mode: true, ping: true };

  it("always has the name and the connect button", () => {
    const none = { round_time: false, players: false, map: false, mode: false, ping: false };
    expect(gridTemplate(none).split(" ").length).toBe(3); // minmax(0, 1fr) is two tokens, plus the button
    expect(gridTemplate(none)).toMatch(/^minmax\(0, 1fr\) 132px$/);
  });

  it("adds a column for each switch", () => {
    const base = gridTemplate({ ...all, round_time: false, players: false, map: false, mode: false, ping: false });
    for (const key of ["round_time", "players", "map", "mode", "ping"] as const) {
      const only = {
        round_time: false,
        players: false,
        map: false,
        mode: false,
        ping: false,
        [key]: true,
      };
      expect(gridTemplate(only).length).toBeGreaterThan(base.length);
    }
    expect(gridTemplate(all).match(/px/g)?.length).toBe(6);
  });
});

describe("durations", () => {
  it("formats minutes and hours", () => {
    expect(formatMinutes(0)).toBe("0 min");
    expect(formatMinutes(59)).toBe("59 min");
    expect(formatMinutes(60)).toBe("1 h 00 min");
    expect(formatMinutes(125)).toBe("2 h 05 min");
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
