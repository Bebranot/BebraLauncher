// UI strings. Russian (locales/ru.ts) is the reference; every other dictionary has the same keys,
// which the types enforce, and a test compares the placeholders. To add a language: a file in
// `locales/`, its code in `UI_LANGUAGES` (languages.ts), one line in `dictionaries` below.

import { detectLang, UI_LANGUAGES, type Lang } from "./languages";
import { de } from "./locales/de";
import { en } from "./locales/en";
import { uk } from "./locales/uk";
import { ru, type Dictionary, type Key } from "./locales/ru";

export type { Key } from "./locales/ru";
export { detectLang, type Lang } from "./languages";

export const dictionaries: Record<Lang, Dictionary> = { ru, en, uk, de };

class I18n {
  /** `auto` or a language code, as saved in the settings. */
  preference = $state("auto");

  get lang(): Lang {
    if ((UI_LANGUAGES as readonly string[]).includes(this.preference)) return this.preference as Lang;
    return detectLang(typeof navigator === "undefined" ? "en" : navigator.language);
  }
}

export const i18n = new I18n();

export function interpolate(text: string, params?: Record<string, string | number>): string {
  if (!params) return text;
  return text.replace(/\{(\w+)\}/g, (whole, name: string) =>
    Object.hasOwn(params, name) ? String(params[name]) : whole,
  );
}

/** Translates a key. Reads `i18n.lang`, so templates update when the language changes. */
export function t(key: Key, params?: Record<string, string | number>): string {
  const dict = dictionaries[i18n.lang];
  // A key the dictionary does not have (a missing translation, a key built at runtime) must
  // show something readable, never `undefined` and never throw (P2-45).
  const text = Object.hasOwn(dict, key)
    ? dict[key]
    : Object.hasOwn(ru, key)
      ? ru[key]
      : String(key);
  return interpolate(String(text ?? key), params);
}

export function hasKey(key: string): key is Key {
  return Object.hasOwn(ru, key);
}

/** Keeps `<html lang>` in step with the interface language (screen readers, spell check). */
export function applyDocumentLang() {
  if (typeof document !== "undefined") document.documentElement.lang = i18n.lang;
}
