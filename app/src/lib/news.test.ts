import { describe, expect, it } from "vitest";
import type { OwnNewsItem } from "./api";
import { localized, unseen } from "./news";

const item = (id: string): OwnNewsItem => ({ id, date: "", title: { ru: id }, body: {}, link: null });

describe("localized", () => {
  it("prefers the language, then Russian, English and anything else", () => {
    expect(localized({ ru: "р", de: "д" }, "de")).toBe("д");
    expect(localized({ ru: "р", en: "e" }, "uk")).toBe("р");
    expect(localized({ en: "e" }, "uk")).toBe("e");
    expect(localized({ de: "д" }, "uk")).toBe("д");
    expect(localized({}, "ru")).toBe("");
  });
});

describe("unseen", () => {
  it("leaves only the entries not seen yet", () => {
    expect(unseen([item("a"), item("b")], ["a"]).map((i) => i.id)).toEqual(["b"]);
    expect(unseen([item("a")], ["a"])).toEqual([]);
  });
});
