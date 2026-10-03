import { describe, expect, it } from "vitest";
import { detectLang, dictionaries, i18n, interpolate, t } from "./i18n.svelte";
import { nativeName, UI_LANGUAGES } from "./languages";

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
  it("picks the language of the Windows locale when there is a translation", () => {
    expect(detectLang("ru-RU")).toBe("ru");
    expect(detectLang("uk-UA")).toBe("uk");
    expect(detectLang("de-AT")).toBe("de");
    expect(detectLang("en-GB")).toBe("en");
  });

  it("gives Belarusian speakers Russian and everything else English", () => {
    expect(detectLang("be")).toBe("ru");
    for (const l of ["fr-FR", "pt-BR", "", "rum"]) expect(detectLang(l)).toBe("en");
  });
});

describe("nativeName", () => {
  it("names a language by itself, whatever the case of the code", () => {
    expect(nativeName("ru")).toBe("Русский");
    expect(nativeName("ru-RU")).toBe("Русский");
    expect(nativeName("RU-ru")).toBe("Русский");
    expect(nativeName("uk")).toBe("Українська");
    expect(nativeName("de")).toBe("Deutsch");
    expect(nativeName("en")).toBe("English");
  });

  it("keeps the region where it changes the language, and leaves non-codes alone", () => {
    expect(nativeName("pt-BR").toLowerCase()).toContain("brasil");
    expect(nativeName("not a code!")).toBe("not a code!");
  });
});

describe("t", () => {
  it("follows the language preference", () => {
    i18n.preference = "ru";
    expect(t("tab.servers")).toBe("Серверы");
    i18n.preference = "en";
    expect(t("tab.servers")).toBe("Servers");
    i18n.preference = "de";
    expect(t("tab.servers")).toBe("Server");
    i18n.preference = "auto";
  });

  it("falls back to the Windows language for a saved language it does not know", () => {
    i18n.preference = "xx";
    expect(UI_LANGUAGES as readonly string[]).toContain(i18n.lang);
    i18n.preference = "auto";
  });

  it("every language has the same keys and placeholders as Russian", () => {
    const placeholders = (s: string) => [...s.matchAll(/\{(\w+)\}/g)].map((m) => m[1]).sort();
    const reference = dictionaries.ru;
    expect(Object.keys(reference).length).toBeGreaterThan(100);
    for (const lang of UI_LANGUAGES) {
      const dict = dictionaries[lang];
      expect(Object.keys(dict).sort(), lang).toEqual(Object.keys(reference).sort());
      for (const [key, value] of Object.entries(reference)) {
        expect(placeholders(dict[key as keyof typeof dict]), `${lang}: ${key}`).toEqual(
          placeholders(value),
        );
      }
    }
  });
});
