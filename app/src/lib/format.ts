import type { PlayStage, ServerRow } from "./api";

export function serverTitle(s: ServerRow): string {
  return s.name?.trim() || s.address;
}

export function runLevelText(level: number | null): string | null {
  switch (level) {
    case 0:
      return "Лобби";
    case 1:
      return "Раунд идёт";
    case 2:
      return "Конец раунда";
    default:
      return null;
  }
}

/** "1 ч 05 мин" since the round started, or null when unknown. */
export function roundDuration(start: string | null, now = Date.now()): string | null {
  if (!start) return null;
  const t = Date.parse(start);
  if (Number.isNaN(t) || t > now) return null;
  const minutes = Math.floor((now - t) / 60_000);
  if (minutes < 60) return `${minutes} мин`;
  return `${Math.floor(minutes / 60)} ч ${String(minutes % 60).padStart(2, "0")} мин`;
}

export function formatBytes(n: number): string {
  if (n < 1024) return `${n} Б`;
  const units = ["КБ", "МБ", "ГБ"];
  let v = n / 1024;
  let i = 0;
  while (v >= 1024 && i < units.length - 1) {
    v /= 1024;
    i++;
  }
  return `${v.toFixed(v < 10 ? 1 : 0)} ${units[i]}`;
}

export const stageText: Record<PlayStage, string> = {
  account: "Проверка аккаунта…",
  info: "Запрос к серверу…",
  files: "Загрузка файлов игры",
  engine: "Загрузка движка",
  starting: "Запуск игры…",
};

export function formatDate(iso: string): string {
  const t = Date.parse(iso);
  return Number.isNaN(t) ? iso : new Date(t).toLocaleDateString("ru-RU");
}
