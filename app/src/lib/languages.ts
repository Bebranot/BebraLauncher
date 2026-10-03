// Language names. A language is always shown by its own name ("Русский", "Deutsch"), the way
// language pickers usually do, never as a code like `ru-RU`. The runtime knows the names.

/** Languages the launcher itself is translated into. */
export const UI_LANGUAGES = ["ru", "en", "uk", "de"] as const;
export type Lang = (typeof UI_LANGUAGES)[number];

/** Languages where the region changes what is meant: shown as "Português (Brasil)". */
const KEEP_REGION = new Set(["pt", "zh", "en", "es", "fr"]);

/** `RU-ru`, `ru_RU`, `ru-RU` -> `{ base: "ru", region: "RU" }`; null when it is not a code. */
function split(code: string): { base: string; region: string | null } | null {
  const m = /^([A-Za-z]{2,3})(?:[-_]([A-Za-z]{2}|\d{3}))?$/.exec(code.trim());
  if (!m) return null;
  return { base: m[1].toLowerCase(), region: m[2] ? m[2].toUpperCase() : null };
}

function capitalise(text: string): string {
  return text.charAt(0).toLocaleUpperCase() + text.slice(1);
}

/** The name of a language in that language: `ru-RU` -> "Русский". Unknown codes as they are. */
export function nativeName(code: string): string {
  const parts = split(code);
  if (!parts) return code;
  const full = parts.region ? `${parts.base}-${parts.region}` : parts.base;
  const wanted = parts.region && KEEP_REGION.has(parts.base) ? full : parts.base;
  try {
    const name = new Intl.DisplayNames([parts.base], { type: "language" }).of(wanted);
    if (name && name.toLowerCase() !== wanted.toLowerCase()) return capitalise(name);
  } catch {
    // The runtime does not know this language: fall through.
  }
  return code;
}

/** Picks the interface language for the Windows language; anything unknown gets English. */
export function detectLang(navigatorLanguage: string): Lang {
  const base = (split(navigatorLanguage)?.base ?? "").toLowerCase();
  // Belarusian speakers read Russian; the other languages have their own translation.
  if (base === "be") return "ru";
  return (UI_LANGUAGES as readonly string[]).includes(base) ? (base as Lang) : "en";
}
