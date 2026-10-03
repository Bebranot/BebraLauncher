import type { Columns, PlayStage, ServerRow } from "./api";
import { i18n, t, type Key } from "./i18n.svelte";

export function serverTitle(s: ServerRow): string {
  return s.name?.trim() || s.address;
}

/** "1 h 05 min" for a number of minutes. */
export function formatMinutes(minutes: number): string {
  if (minutes < 60) return `${minutes} ${t("unit.min")}`;
  return `${Math.floor(minutes / 60)} ${t("unit.hour")} ${String(minutes % 60).padStart(2, "0")} ${t("unit.min")}`;
}

/** Time played, from seconds. Under a minute counts as one minute: zero would look like a bug. */
export function formatPlayed(seconds: number): string {
  return formatMinutes(Math.max(1, Math.round(seconds / 60)));
}

/** `h:mm:ss`, for a running round. */
export function formatClock(totalSeconds: number): string {
  const s = Math.max(0, Math.floor(totalSeconds));
  const h = Math.floor(s / 3600);
  const m = Math.floor((s % 3600) / 60);
  return `${h}:${String(m).padStart(2, "0")}:${String(s % 60).padStart(2, "0")}`;
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
      const start = s.round_start_time ? Date.parse(s.round_start_time) : NaN;
      if (Number.isNaN(start) || start > now) return null;
      return formatClock((now - start) / 1000);
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

/** The CSS grid columns of the server table for the columns the player wants. */
export function gridTemplate(c: Columns): string {
  const parts = ["minmax(0, 1fr)"];
  if (c.mode) parts.push("110px");
  if (c.map) parts.push("110px");
  if (c.ping) parts.push("88px");
  if (c.round_time) parts.push("96px");
  if (c.players) parts.push("80px");
  parts.push("124px"); // the Connect button
  return parts.join(" ");
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} ${t("unit.B")}`;
  const units: Key[] = ["unit.KB", "unit.MB", "unit.GB"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 ? 1 : 0)} ${t(units[i])}`;
}

export function stageText(stage: PlayStage): string {
  return t(`stage.${stage}` as Key);
}

const DATE_LOCALE = { ru: "ru-RU", en: "en-GB", uk: "uk-UA", de: "de-DE" } as const;

export function formatDate(iso: string): string {
  const d = Date.parse(iso);
  return Number.isNaN(d) ? iso : new Date(d).toLocaleDateString(DATE_LOCALE[i18n.lang]);
}
