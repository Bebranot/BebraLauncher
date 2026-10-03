import { describe, expect, it } from "vitest";
import { inline, parseChangelog, parseGroups } from "./changelog";

const SAMPLE = `# Журнал изменений

Вступление, которое не запись.

## [Не выпущено]

## [0.2.001] — 2026-10-05

### Исправлено
- Первое
  с переносом
- Второе

## [0.2.000] — 2026-10-04

### Добавлено
- Что-то с \`кодом\` и [ссылкой](https://example.com)

### Изменено
- Другое
`;

describe("parseChangelog", () => {
  it("reads entries newest first, with groups and items", () => {
    const entries = parseChangelog(SAMPLE);
    expect(entries.map((e) => e.version)).toEqual(["0.2.001", "0.2.000"]);
    expect(entries[0].date).toBe("2026-10-05");
    expect(entries[0].groups).toEqual([{ title: "Исправлено", items: ["Первое с переносом", "Второе"] }]);
    expect(entries[1].groups.map((g) => g.title)).toEqual(["Добавлено", "Изменено"]);
  });

  it("shows the not-released section only when it has items", () => {
    expect(parseChangelog(SAMPLE).some((e) => e.version === null)).toBe(false);
    const withItems = SAMPLE.replace("## [Не выпущено]\n", "## [Не выпущено]\n### Добавлено\n- Новое\n");
    const entries = parseChangelog(withItems);
    expect(entries[0]).toMatchObject({ version: null, date: null });
    expect(entries[0].groups[0].items).toEqual(["Новое"]);
  });

  it("copes with nothing and with Windows line ends", () => {
    expect(parseChangelog("")).toEqual([]);
    expect(parseChangelog(SAMPLE.replace(/\n/g, "\r\n")).length).toBe(2);
  });
});

describe("parseGroups", () => {
  it("reads a release's notes, which have no version heading", () => {
    const notes = "### Добавлено\n- Раз\n- Два\n";
    expect(parseGroups(notes.split("\n"))).toEqual([{ title: "Добавлено", items: ["Раз", "Два"] }]);
  });

  it("keeps bare items under an untitled group", () => {
    expect(parseGroups(["- просто"])).toEqual([{ title: "", items: ["просто"] }]);
  });
});

describe("inline", () => {
  it("splits out code and drops link addresses", () => {
    expect(inline("Что-то с `кодом` и [ссылкой](https://example.com)")).toEqual([
      { text: "Что-то с ", code: false },
      { text: "кодом", code: true },
      { text: " и ссылкой", code: false },
    ]);
    expect(inline("plain")).toEqual([{ text: "plain", code: false }]);
    expect(inline("``")).toEqual([{ text: "``", code: false }]);
  });
});
