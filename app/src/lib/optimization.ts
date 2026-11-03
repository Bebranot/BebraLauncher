// Optimization switches: the key list and the groups the settings page shows them in.

/** Every switch, in display order (same names as `OptimizationConfig` in the backend). */
export const OPTIMIZATION_KEYS = [
  "fast_lighting",
  "light_lowres",
  "ao_lowres",
  "ao_off",
  "parallax_low",
  "particles_lite",
  "calm_effects",
  "tile_edges_off",
  "audio_lite",
  "unlock_fps",
  "steady_fps",
  "server_gc",
  "low_latency_gc",
  "high_priority",
  "no_throttling",
  "prefer_dgpu",
  "patch_outline",
  "patch_status_icons",
] as const;

export type OptimizationKey = (typeof OPTIMIZATION_KEYS)[number];

export const OPTIMIZATION_GROUPS: { id: string; keys: OptimizationKey[] }[] = [
  { id: "lighting", keys: ["fast_lighting", "light_lowres", "ao_lowres", "ao_off", "parallax_low"] },
  { id: "effects", keys: ["particles_lite", "calm_effects", "tile_edges_off"] },
  { id: "audio", keys: ["audio_lite"] },
  { id: "fps", keys: ["unlock_fps", "steady_fps"] },
  { id: "memory", keys: ["server_gc", "low_latency_gc"] },
  { id: "windows", keys: ["high_priority", "no_throttling", "prefer_dgpu"] },
  { id: "patches", keys: ["patch_outline", "patch_status_icons"] },
];
