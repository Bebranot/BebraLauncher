import type { Columns } from "./api";

/** The width at which every column the player asked for still fits beside the server name. */
export const FULL_TABLE_WIDTH = 900;

/**
 * The columns the table can show in `width` CSS pixels. At 200% zoom or in an 800×500 window
 * the viewport is only a few hundred pixels wide; the secondary columns go first, so the name
 * and the Connect button are never pushed off screen.
 */
export function compactColumns(columns: Columns, width: number): Columns {
  if (!Number.isFinite(width) || width >= FULL_TABLE_WIDTH) return columns;
  return {
    ...columns,
    map: false,
    mode: false,
    ping: width >= 640 && columns.ping,
    round_time: width >= 520 && columns.round_time,
    players: width >= 440 && columns.players,
  };
}
