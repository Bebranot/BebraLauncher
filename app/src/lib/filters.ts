// Searching, filtering and sorting the server list. The rules are those of the official launcher:
// inside one category (language, region, roleplay level) a server must match any ticked value;
// ticking nothing means "don't care"; "unspecified" matches servers that carry no tag of that kind.

import type { ServerRow } from "./api";
import { nativeName, type Lang } from "./languages";

/** Value for "the server does not say" in the language, region and roleplay categories. */
export const UNSPECIFIED = "";

export interface FilterState {
  languages: string[];
  regions: string[];
  /** `none`, `low`, `med`, `high` or UNSPECIFIED. */
  rp: string[];
  /** `yes` (the server is 18+) or `no`. Both ticked is the same as none. */
  age: string[];
  hideEmpty: boolean;
  hideFull: boolean;
  minPlayers: number | null;
  maxPlayers: number | null;
}

export function emptyFilters(): FilterState {
  return {
    languages: [],
    regions: [],
    rp: [],
    age: [],
    hideEmpty: false,
    hideFull: false,
    minPlayers: null,
    maxPlayers: null,
  };
}

/** Reads filters kept from an earlier run; anything unexpected becomes "no filter". */
export function parseFilters(value: unknown): FilterState {
  const result = emptyFilters();
  if (typeof value !== "object" || value === null) return result;
  const v = value as Record<string, unknown>;
  const strings = (x: unknown): string[] =>
    Array.isArray(x) ? x.filter((s): s is string => typeof s === "string").slice(0, 200) : [];
  const count = (x: unknown): number | null =>
    typeof x === "number" && Number.isFinite(x) && x >= 0 ? Math.floor(x) : null;
  result.languages = strings(v.languages);
  result.regions = strings(v.regions);
  result.rp = strings(v.rp);
  result.age = strings(v.age).filter((a) => a === "yes" || a === "no");
  result.hideEmpty = v.hideEmpty === true;
  result.hideFull = v.hideFull === true;
  result.minPlayers = count(v.minPlayers);
  result.maxPlayers = count(v.maxPlayers);
  return result;
}

/** How many filters are switched on. */
export function activeFilterCount(f: FilterState): number {
  return (
    f.languages.length +
    f.regions.length +
    f.rp.length +
    (f.age.length === 1 ? 1 : 0) +
    (f.hideEmpty ? 1 : 0) +
    (f.hideFull ? 1 : 0) +
    (f.minPlayers !== null ? 1 : 0) +
    (f.maxPlayers !== null ? 1 : 0)
  );
}

/** Values of the tags that start with `prefix`, e.g. `lang:` gives `["ru", "en"]`. */
export function tagValuesOf(tags: string[], prefix: string): string[] {
  return tags
    .filter((t) => t.toLowerCase().startsWith(prefix))
    .map((t) => t.slice(prefix.length).toLowerCase());
}

/** `ru-RU` is Russian: only the part before the dash counts for the language filter. */
export function primaryLanguage(tag: string): string {
  const dash = tag.indexOf("-");
  return dash === -1 ? tag : tag.slice(0, dash);
}

function categoryMatches(selected: string[], values: string[]): boolean {
  if (selected.length === 0) return true;
  if (values.length === 0) return selected.includes(UNSPECIFIED);
  return values.some((v) => selected.includes(v));
}

export function isFull(s: ServerRow): boolean {
  return s.soft_max_players > 0 && s.players >= s.soft_max_players;
}

export function matchesFilters(s: ServerRow, f: FilterState): boolean {
  if (f.age.length === 1) {
    const is18 = s.tags.includes("18+");
    if ((f.age[0] === "yes") !== is18) return false;
  }
  const languages = tagValuesOf(s.tags, "lang:").map(primaryLanguage);
  if (!categoryMatches(f.languages, languages)) return false;
  if (!categoryMatches(f.regions, tagValuesOf(s.tags, "region:"))) return false;
  if (!categoryMatches(f.rp, tagValuesOf(s.tags, "rp:"))) return false;

  if (f.hideEmpty && s.players === 0) return false;
  if (f.hideFull && isFull(s)) return false;
  if (f.minPlayers !== null && s.players < f.minPlayers) return false;
  if (f.maxPlayers !== null && s.players > f.maxPlayers) return false;
  return true;
}

/** Free-text search over what the player sees of a server. */
export function matchesSearch(s: ServerRow, query: string): boolean {
  const q = query.trim().toLowerCase();
  if (!q) return true;
  const hay = `${s.name ?? ""} ${s.address} ${s.tags.join(" ")} ${s.map ?? ""} ${s.preset ?? ""}`;
  return hay.toLowerCase().includes(q);
}

// ---------------------------------------------------------------- sorting

export type SortKey = "name" | "mode" | "map" | "ping" | "time" | "players";

export interface Sort {
  key: SortKey;
  desc: boolean;
}

export const DEFAULT_SORT: Sort = { key: "players", desc: true };

export function titleOf(s: ServerRow): string {
  return s.name?.trim() || s.address;
}

/** The sort a header click leads to: the same column flips direction, a new one starts sensibly. */
export function nextSort(current: Sort, key: SortKey): Sort {
  if (current.key === key) return { key, desc: !current.desc };
  // Numbers are most useful biggest first, words from A.
  return { key, desc: key === "players" || key === "time" };
}

/** Returns a sorted copy. `pings` maps an address to its milliseconds (missing = unknown). */
export function sortServers(
  list: ServerRow[],
  sort: Sort,
  pings: Record<string, number | null | undefined> = {},
): ServerRow[] {
  const text = (a: string | null, b: string | null) =>
    (a ?? "").localeCompare(b ?? "", undefined, { sensitivity: "base" });
  const startOf = (s: ServerRow) =>
    s.run_level === 1 && s.round_start_time ? Date.parse(s.round_start_time) || null : null;

  const compare = (a: ServerRow, b: ServerRow): number => {
    switch (sort.key) {
      case "name":
        return text(titleOf(a), titleOf(b));
      case "mode":
        return text(a.preset, b.preset);
      case "map":
        return text(a.map, b.map);
      case "players":
        return a.players - b.players;
      case "ping": {
        // Unknown pings go last whichever way it is sorted.
        const pa = pings[a.address];
        const pb = pings[b.address];
        if (pa == null && pb == null) return 0;
        if (pa == null) return sort.desc ? -1 : 1;
        if (pb == null) return sort.desc ? 1 : -1;
        return pa - pb;
      }
      case "time": {
        // A longer round has an earlier start; servers without a running round go last.
        const sa = startOf(a);
        const sb = startOf(b);
        if (sa === null && sb === null) return 0;
        if (sa === null) return sort.desc ? -1 : 1;
        if (sb === null) return sort.desc ? 1 : -1;
        return sb - sa;
      }
    }
  };

  return [...list].sort((a, b) => {
    const c = compare(a, b);
    if (c !== 0) return sort.desc ? -c : c;
    // The same value: busier servers first, then by name, so the order is stable.
    return b.players - a.players || text(titleOf(a), titleOf(b));
  });
}

// ---------------------------------------------------------------- names

const REGIONS: Record<string, Record<Lang, string>> = {
  af_c: { ru: "Центральная Африка", en: "Central Africa", uk: "Центральна Африка", de: "Zentralafrika" },
  af_n: { ru: "Северная Африка", en: "North Africa", uk: "Північна Африка", de: "Nordafrika" },
  af_s: { ru: "Южная Африка", en: "South Africa", uk: "Південна Африка", de: "Südafrika" },
  ata: { ru: "Антарктида", en: "Antarctica", uk: "Антарктида", de: "Antarktis" },
  as_e: { ru: "Восточная Азия", en: "East Asia", uk: "Східна Азія", de: "Ostasien" },
  as_n: { ru: "Северная Азия", en: "North Asia", uk: "Північна Азія", de: "Nordasien" },
  as_se: { ru: "Юго-Восточная Азия", en: "Southeast Asia", uk: "Південно-Східна Азія", de: "Südostasien" },
  am_c: { ru: "Центральная Америка", en: "Central America", uk: "Центральна Америка", de: "Mittelamerika" },
  eu_e: { ru: "Восточная Европа", en: "Eastern Europe", uk: "Східна Європа", de: "Osteuropa" },
  eu_w: { ru: "Западная Европа", en: "Western Europe", uk: "Західна Європа", de: "Westeuropa" },
  grl: { ru: "Гренландия", en: "Greenland", uk: "Гренландія", de: "Grönland" },
  ind: { ru: "Индия", en: "India", uk: "Індія", de: "Indien" },
  me: { ru: "Ближний Восток", en: "Middle East", uk: "Близький Схід", de: "Naher Osten" },
  luna: { ru: "Луна", en: "The Moon", uk: "Місяць", de: "Der Mond" },
  am_n_c: { ru: "Северная Америка: центр", en: "North America: Central", uk: "Північна Америка: центр", de: "Nordamerika: Mitte" },
  am_n_e: { ru: "Северная Америка: восток", en: "North America: East", uk: "Північна Америка: схід", de: "Nordamerika: Osten" },
  am_n_w: { ru: "Северная Америка: запад", en: "North America: West", uk: "Північна Америка: захід", de: "Nordamerika: Westen" },
  oce: { ru: "Океания", en: "Oceania", uk: "Океанія", de: "Ozeanien" },
  am_s_e: { ru: "Южная Америка: восток", en: "South America: East", uk: "Південна Америка: схід", de: "Südamerika: Osten" },
  am_s_s: { ru: "Южная Америка: юг", en: "South America: South", uk: "Південна Америка: південь", de: "Südamerika: Süden" },
  am_s_w: { ru: "Южная Америка: запад", en: "South America: West", uk: "Південна Америка: захід", de: "Südamerika: Westen" },
};

/** The name of a region code in the given language; unknown codes are shown as they are. */
export function regionName(code: string, lang: Lang): string {
  return REGIONS[code]?.[lang] ?? code;
}

/** A language by its own name ("Русский"), never as a code; unknown codes as they are. */
export function languageName(code: string): string {
  return nativeName(code);
}

/** What a server tag says to a person: `lang:ru-RU` is "Русский", `region:eu_w` a region name. */
export function tagLabel(tag: string, lang: Lang): string {
  const lower = tag.toLowerCase();
  if (lower.startsWith("lang:")) return nativeName(tag.slice(5));
  if (lower.startsWith("region:")) return regionName(lower.slice(7), lang);
  return tag;
}

/** The distinct values of a tag kind over the servers, in a stable order (most common first). */
export function availableValues(servers: ServerRow[], prefix: string, primary = false): string[] {
  const counts = new Map<string, number>();
  for (const s of servers) {
    const seen = new Set<string>();
    for (const raw of tagValuesOf(s.tags, prefix)) {
      const v = primary ? primaryLanguage(raw) : raw;
      if (!seen.has(v)) {
        seen.add(v);
        counts.set(v, (counts.get(v) ?? 0) + 1);
      }
    }
  }
  return [...counts.entries()]
    .sort((a, b) => b[1] - a[1] || a[0].localeCompare(b[0]))
    .map(([v]) => v);
}
