// The setup wizard: which steps there are and in what order.

/**
 * The wizard the player has to see. Raise it (together with `CURRENT_SETUP_VERSION` in
 * app/src-tauri/src/settings.rs) when a release adds settings worth showing everybody again.
 */
export const SETUP_VERSION = 1;

export const STEP_IDS = [
  "welcome",
  "theme",
  "look",
  "profile",
  "game",
  "launch",
  "servers",
  "import",
  "system",
  "privacy",
  "tools",
  "finish",
] as const;

export type StepId = (typeof STEP_IDS)[number];

/** The steps to walk through; the import step exists only when there is something to import. */
export function stepsFor(officialFound: boolean): StepId[] {
  return STEP_IDS.filter((id) => id !== "import" || officialFound);
}

/** The launcher's choice for a whole play session: what the window does while the game runs. */
export type PlayMode = "stay" | "minimize" | "unload" | "close";

export interface PlayFlags {
  close_on_play: boolean;
  unload_on_play: boolean;
  minimize_on_play: boolean;
}

/** Closing wins over unloading, unloading over minimizing (the options tab greys the rest out). */
export function playModeOf(flags: PlayFlags): PlayMode {
  if (flags.close_on_play) return "close";
  if (flags.unload_on_play) return "unload";
  if (flags.minimize_on_play) return "minimize";
  return "stay";
}

export function flagsOf(mode: PlayMode): PlayFlags {
  return {
    close_on_play: mode === "close",
    unload_on_play: mode === "unload",
    minimize_on_play: mode === "minimize",
  };
}

/** Same address, whatever the case or the closing slash. */
export function sameHub(a: string, b: string): boolean {
  const key = (x: string) => x.trim().toLowerCase().replace(/\/+$/, "");
  return key(a) === key(b);
}

/** Whole gigabytes for display, or null when the system did not say. */
export function gigabytes(ramMb: number | null): number | null {
  return ramMb === null ? null : Math.max(1, Math.round(ramMb / 1024));
}
