import { describe, expect, it } from "vitest";
import type { Columns } from "./api";
import { FULL_TABLE_WIDTH, compactColumns } from "./tableLayout";

const ALL: Columns = { round_time: true, players: true, map: true, mode: true, ping: true };

describe("compactColumns", () => {
  it("keeps everything the player asked for on a wide window", () => {
    expect(compactColumns(ALL, FULL_TABLE_WIDTH)).toEqual(ALL);
    expect(compactColumns(ALL, 1920)).toEqual(ALL);
    // A window that is not measured yet must not collapse the table.
    expect(compactColumns(ALL, Number.NaN)).toEqual(ALL);
  });

  it("drops the long text columns first when the window narrows", () => {
    const wide = compactColumns(ALL, 800);
    expect(wide.map).toBe(false);
    expect(wide.mode).toBe(false);
    expect(wide.ping).toBe(true);
    expect(wide.round_time).toBe(true);
    expect(wide.players).toBe(true);
  });

  it("walks down to name and Connect at 200% zoom of an 800 px window", () => {
    const narrow = compactColumns(ALL, 400);
    expect(narrow).toEqual({
      round_time: false,
      players: false,
      map: false,
      mode: false,
      ping: false,
    });
  });

  it("only ever removes columns, and the threshold width itself still shows the column", () => {
    const widths = [0, 439, 440, 519, 520, 639, 640, 899, 900];
    let previous = 0;
    for (const width of widths) {
      const got = compactColumns(ALL, width);
      const shown = Object.values(got).filter(Boolean).length;
      expect(shown, `${width}px`).toBeGreaterThanOrEqual(previous);
      previous = shown;
    }
    expect(compactColumns(ALL, 640).ping).toBe(true);
    expect(compactColumns(ALL, 639).ping).toBe(false);
    expect(compactColumns(ALL, 520).round_time).toBe(true);
    expect(compactColumns(ALL, 519).round_time).toBe(false);
    expect(compactColumns(ALL, 440).players).toBe(true);
    expect(compactColumns(ALL, 439).players).toBe(false);
  });

  it("does not turn on a column the player turned off", () => {
    const got = compactColumns({ ...ALL, ping: false, map: false }, 2000);
    expect(got.ping).toBe(false);
    expect(got.map).toBe(false);
  });
});
