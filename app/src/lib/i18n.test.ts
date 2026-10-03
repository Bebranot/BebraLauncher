import { describe, expect, it } from "vitest";
import { detectLang, i18n, interpolate, t } from "./i18n.svelte";
// Importing the module also gives us both dictionaries through `t`; compare them by probing keys.
import * as source from "./i18n.svelte?raw";

describe("interpolate", () => {
  it("fills named placeholders", () => {
    expect(interpolate("{a} из {b}", { a: 3, b: "7" })).toBe("3 из 7");
  });

  it("leaves unknown placeholders and plain text alone", () => {
    expect(interpolate("x {missing} y", { a: 1 })).toBe("x {missing} y");
    expect(interpolate("no params {a}")).toBe("no params {a}");
  });
});

describe("detectLang", () => {
  it("gives Russian to Russian-speaking locales", () => {
    for (const l of ["ru", "ru-RU", "uk-UA", "be"]) expect(detectLang(l)).toBe("ru");
  });

  it("gives English to everything else", () => {
    for (const l of ["en-US", "de-DE", "pt-BR", "", "rum"]) expect(detectLang(l)).toBe("en");
  });
});

describe("t", () => {
  it("follows the language preference", () => {
    i18n.preference = "ru";
    expect(t("tab.servers")).toBe("Серверы");
    i18n.preference = "en";
    expect(t("tab.servers")).toBe("Servers");
    i18n.preference = "auto";
  });

  it("both languages use the same placeholders in every string", () => {
    // Pull `"key": "text"` pairs of the two dictionaries out of the source and compare them.
    const text = (source as unknown as { default: string }).default;
    const ruBlock = text.slice(text.indexOf("const ru = {"), text.indexOf("} as const;"));
    const enBlock = text.slice(text.indexOf("const en: Record<Key, string> = {"));
    const pairs = (block: string) =>
      new Map(
        [...block.matchAll(/"([\w.]+)":\s*\n?\s*"((?:[^"\\]|\\.)*)"/g)].map((m) => [m[1], m[2]]),
      );
    const placeholders = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();

    const ru = pairs(ruBlock);
    const en = pairs(enBlock);
    expect(ru.size).toBeGreaterThan(100);
    expect([...en.keys()].sort()).toEqual([...ru.keys()].sort());
    for (const [key, value] of ru) {
      expect(placeholders(en.get(key)!), key).toEqual(placeholders(value));
    }
  });
});
