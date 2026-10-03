import type { PlayStage, ServerRow } from "./api";
import { i18n, t, type Key } from "./i18n.svelte";

export function serverTitle(s: ServerRow): string {
  return s.name?.trim() || s.address;
}

export function runLevelText(level: number | null): string | null {
  switch (level) {
    case 0:
      return t("run.lobby");
    case 1:
      return t("run.round");
    case 2:
      return t("run.ended");
    default:
      return null;
  }
}

/** "1 h 05 min" for a number of minutes. */
export function formatMinutes(minutes: number): string {
  if (minutes < 60) return `${minutes} ${t("unit.min")}`;
  return `${Math.floor(minutes / 60)} ${t("unit.hour")} ${String(minutes % 60).padStart(2, "0")} ${t("unit.min")}`;
}

/** Time since the round started, or null when unknown. */
export function roundDuration(start: string | null, now = Date.now()): string | null {
  if (!start) return null;
  const t0 = Date.parse(start);
  if (Number.isNaN(t0) || t0 > now) return null;
  return formatMinutes(Math.floor((now - t0) / 60_000));
}

/** Time played, from seconds. Under a minute counts as one minute: zero would look like a bug. */
export function formatPlayed(seconds: number): string {
  return formatMinutes(Math.max(1, Math.round(seconds / 60)));
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

export function formatDate(iso: string): string {
  const d = Date.parse(iso);
  return Number.isNaN(d) ? iso : new Date(d).toLocaleDateString(i18n.lang === "ru" ? "ru-RU" : "en-GB");
}
