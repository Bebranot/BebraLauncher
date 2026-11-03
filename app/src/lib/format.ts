import type { Columns, PlayStage, ServerRow } from "./api";
import { i18n, t, type Key } from "./i18n.svelte";

export function serverTitle(s: ServerRow): string {
  return s.name?.trim() || s.address;
}

const DATE_LOCALE = { ru: "ru-RU", en: "en-GB", uk: "uk-UA", de: "de-DE" } as const;

/** A number in the interface language: `1.5` in English, `1,5` in Russian (P2-48). */
export function formatNumber(value: number, digits = 0): string {
  try {
    return new Intl.NumberFormat(DATE_LOCALE[i18n.lang], {
      minimumFractionDigits: digits,
      maximumFractionDigits: digits,
    }).format(value);
  } catch {
    return digits > 0 ? value.toFixed(digits) : String(value);
  }
}

/** "1 h 05 min" for a number of minutes. */
export function formatMinutes(minutes: number): string {
  // A fractional or negative count must not print `-3` or `2.5`: round to whole minutes.
  const total = Math.max(0, Math.round(Number.isFinite(minutes) ? minutes : 0));
  if (total < 60) return `${total} ${t("unit.min")}`;
  return `${Math.floor(total / 60)} ${t("unit.hour")} ${String(total % 60).padStart(2, "0")} ${t("unit.min")}`;
}

/** Time played, from seconds. Under a minute counts as one minute: zero would look like a bug. */
export function formatPlayed(seconds: number): string {
  return formatMinutes(Math.max(1, Math.round((Number.isFinite(seconds) ? seconds : 0) / 60)));
}

/** `h:mm:ss`, for a running round. */
export function formatClock(totalSeconds: number): string {
  const s = Math.max(0, Math.floor(Number.isFinite(totalSeconds) ? totalSeconds : 0));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return `${h}:${String(m).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
}

/**
 * `Date.parse` of a time without a zone follows the machine's zone (the backend sends UTC);
 * say UTC explicitly so the round time is the same everywhere (P2-48).
 */
export function parseTime(text: string): number {
  const trimmed = text.trim();
  const hasZone = /(?:[zZ]|[+-]\d{2}:?\d{2})$/.test(trimmed);
  return Date.parse(hasZone ? trimmed : `${trimmed}Z`);
}

/**
 * What the round column shows: how long the round has been going, or the state of the server.
 * `null` when the server does not say.
 */
export function roundText(s: ServerRow, now = Date.now()): string | null {
  switch (s.run_level) {
    case 0:
      return t("run.lobby");
    case 2:
      return t("run.ended");
    case 1: {
      const start = s.round_start_time ? parseTime(s.round_start_time) : NaN;
      if (Number.isNaN(start)) return null;
      const seconds = (now - start) / 1000;
      // A clock a few seconds ahead of ours is not an empty cell: show 0:00:00. Hours in the
      // future is not clock skew but bad data, and the cell stays empty.
      if (seconds < -300) return null;
      return formatClock(Math.max(0, seconds));
    }
    default:
      return null;
  }
}

/** The players column: `12 / 80`, with ∞ for a server without a limit. */
export function playersText(s: ServerRow): string {
  return `${s.players} / ${s.soft_max_players > 0 ? s.soft_max_players : "∞"}`;
}

export type PingClass = "good" | "mid" | "bad" | "unknown";

/** Colour class of a ping, with the thresholds of the reference launcher. */
export function pingClass(ms: number | null | undefined): PingClass {
  if (ms === null || ms === undefined) return "unknown";
  if (ms <= 120) return "good";
  if (ms <= 220) return "mid";
  return "bad";
}

/**
 * The CSS grid columns of the server table for the columns the player wants. Everything but the
 * name and the button is allowed to shrink, so a 400 px wide viewport (200 % zoom) still fits
 * (P1-31).
 */
export function gridTemplate(c: Columns): string {
  const parts = ["minmax(96px, 2fr)"];
  if (c.mode) parts.push("minmax(64px, 0.8fr)");
  if (c.map) parts.push("minmax(64px, 0.8fr)");
  if (c.ping) parts.push("minmax(52px, 0.6fr)");
  if (c.round_time) parts.push("minmax(64px, 0.8fr)");
  if (c.players) parts.push("minmax(56px, 0.6fr)");
  parts.push("minmax(80px, 124px)"); // the Connect button
  return parts.join(" ");
}

const BYTE_UNITS: Key[] = ["unit.B", "unit.KB", "unit.MB", "unit.GB", "unit.TB", "unit.PB"];

/** Bytes in the units people read, up to petabytes; garbage or a negative size shows 0 B. */
export function formatBytes(n: number): string {
  if (!Number.isFinite(n) || n <= 0) return `0 ${t("unit.B")}`;
  if (n < 1024) return `${Math.round(n)} ${t("unit.B")}`;
  let v = n;
  let i = 0;
  while (v >= 1024 && i < BYTE_UNITS.length - 1) {
    v /= 1024;
    i++;
  }
  return `${formatNumber(v, v < 10 ? 1 : 0)} ${t(BYTE_UNITS[i])}`;
}

export function stageText(stage: PlayStage): string {
  return t(`stage.${stage}` as Key);
}

/** A transfer speed in bits per second, shown in megabits per second. */
export function formatSpeed(bitsPerSecond: number): string {
  const bits = Number.isFinite(bitsPerSecond) && bitsPerSecond > 0 ? bitsPerSecond : 0;
  const mbit = bits / 1e6;
  return `${formatNumber(mbit, mbit < 10 ? 1 : 0)} ${t("unit.Mbit")}`;
}

/** A date in the interface language, not in the machine's locale (P2-48). */
export function formatDate(iso: string): string {
  const d = typeof iso === "number" ? iso : parseTime(iso);
  return Number.isNaN(d) ? iso : new Date(d).toLocaleDateString(DATE_LOCALE[i18n.lang]);
}

/** A time of day in the interface language (the update check time). */
export function formatTime(ms: number): string {
  const d = Number.isFinite(ms) ? ms : Date.now();
  return new Date(d).toLocaleTimeString(DATE_LOCALE[i18n.lang], {
    hour: "2-digit",
    minute: "2-digit",
  });
}
