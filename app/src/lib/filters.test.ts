import { describe, expect, it } from "vitest";
import type { ServerRow } from "./api";
import {
  DEFAULT_SORT,
  UNSPECIFIED,
  activeFilterCount,
  availableValues,
  emptyFilters,
  languageName,
  matchesFilters,
  matchesSearch,
  nextSort,
  parseFilters,
  primaryLanguage,
  canonicalRegion,
  regionName,
  regionsOf,
  sortServers,
  tagValuesOf,
} from "./filters";

const row = (over: Partial<ServerRow> = {}): ServerRow => ({
  address: "ss14s://a.test",
  name: "A",
  players: 10,
  soft_max_players: 50,
  tags: [],
  map: null,
  preset: null,
  run_level: null,
  round_start_time: null,
  panic_bunker: null,
  ...over,
});

describe("tags", () => {
  it("reads values of one kind, case-insensitively", () => {
    expect(tagValuesOf(["lang:RU", "region:eu_w", "lang:en-US"], "lang:")).toEqual(["ru", "en-us"]);
    expect(tagValuesOf([], "lang:")).toEqual([]);
  });

  it("the language filter looks at the part before the dash", () => {
    expect(primaryLanguage("ru-RU")).toBe("ru");
    expect(primaryLanguage("ru")).toBe("ru");
  });
});

describe("filters", () => {
  it("no filter lets everything through", () => {
    expect(matchesFilters(row(), emptyFilters())).toBe(true);
    expect(activeFilterCount(emptyFilters())).toBe(0);
  });

  it("a category matches any ticked value", () => {
    const f = { ...emptyFilters(), languages: ["ru", "de"] };
    expect(matchesFilters(row({ tags: ["lang:ru"] }), f)).toBe(true);
    expect(matchesFilters(row({ tags: ["lang:en", "lang:de"] }), f)).toBe(true);
    expect(matchesFilters(row({ tags: ["lang:en"] }), f)).toBe(false);
    expect(matchesFilters(row({ tags: ["lang:ru-RU"] }), f)).toBe(true);
  });

  it("'unspecified' matches servers without a tag of that kind, and only those", () => {
    const f = { ...emptyFilters(), regions: [UNSPECIFIED] };
    expect(matchesFilters(row({ tags: [] }), f)).toBe(true);
    expect(matchesFilters(row({ tags: ["region:eu_w"] }), f)).toBe(false);
    const both = { ...emptyFilters(), regions: [UNSPECIFIED, "eu_w"] };
    expect(matchesFilters(row({ tags: ["region:eu_w"] }), both)).toBe(true);
    expect(matchesFilters(row({ tags: [] }), both)).toBe(true);
    expect(matchesFilters(row({ tags: ["region:am_c"] }), both)).toBe(false);
  });

  it("categories combine with AND", () => {
    const f = { ...emptyFilters(), languages: ["ru"], rp: ["high"] };
    expect(matchesFilters(row({ tags: ["lang:ru", "rp:high"] }), f)).toBe(true);
    expect(matchesFilters(row({ tags: ["lang:ru", "rp:low"] }), f)).toBe(false);
  });

  it("18+: yes, no, or both (= no filter)", () => {
    const adult = row({ tags: ["18+"] });
    const normal = row({ tags: [] });
    expect(matchesFilters(adult, { ...emptyFilters(), age: ["yes"] })).toBe(true);
    expect(matchesFilters(normal, { ...emptyFilters(), age: ["yes"] })).toBe(false);
    expect(matchesFilters(adult, { ...emptyFilters(), age: ["no"] })).toBe(false);
    expect(matchesFilters(normal, { ...emptyFilters(), age: ["no"] })).toBe(true);
    expect(matchesFilters(adult, { ...emptyFilters(), age: ["yes", "no"] })).toBe(true);
    expect(activeFilterCount({ ...emptyFilters(), age: ["yes", "no"] })).toBe(0);
  });

  it("player counts", () => {
    const base = emptyFilters();
    expect(matchesFilters(row({ players: 0 }), { ...base, hideEmpty: true })).toBe(false);
    expect(matchesFilters(row({ players: 50 }), { ...base, hideFull: true })).toBe(false);
    expect(matchesFilters(row({ players: 49 }), { ...base, hideFull: true })).toBe(true);
    // a server with no limit is never "full"
    expect(
      matchesFilters(row({ players: 500, soft_max_players: 0 }), { ...base, hideFull: true }),
    ).toBe(true);
    expect(matchesFilters(row({ players: 5 }), { ...base, minPlayers: 6 })).toBe(false);
    expect(matchesFilters(row({ players: 6 }), { ...base, minPlayers: 6 })).toBe(true);
    expect(matchesFilters(row({ players: 7 }), { ...base, maxPlayers: 6 })).toBe(false);
    expect(matchesFilters(row({ players: 6 }), { ...base, maxPlayers: 6 })).toBe(true);
  });

  it("counts what is switched on", () => {
    expect(
      activeFilterCount({
        ...emptyFilters(),
        languages: ["ru", "en"],
        age: ["yes"],
        hideFull: true,
        minPlayers: 3,
      }),
    ).toBe(5);
  });

  it("filters kept from an earlier run are cleaned", () => {
    expect(parseFilters(null)).toEqual(emptyFilters());
    expect(parseFilters("junk")).toEqual(emptyFilters());
    const f = parseFilters({
      languages: ["ru", 5, null],
      age: ["yes", "maybe"],
      hideEmpty: "yes",
      minPlayers: -3,
      maxPlayers: 12.7,
    });
    expect(f.languages).toEqual(["ru"]);
    expect(f.age).toEqual(["yes"]);
    expect(f.hideEmpty).toBe(false);
    expect(f.minPlayers).toBeNull();
    expect(f.maxPlayers).toBe(12);
  });
});

describe("search", () => {
  it("looks at name, address, tags, map and mode", () => {
    const s = row({
      name: "Space Station",
      address: "ss14s://station.example",
      tags: ["lang:ru"],
      map: "Box",
      preset: "Nukeops",
    });
    for (const q of ["space", "EXAMPLE", "lang:ru", "box", "nuke", "  station  ", "space box", "station nuke ru"]) {
      expect(matchesSearch(s, q), q).toBe(true);
    }
    expect(matchesSearch(s, "kitchen")).toBe(false);
    expect(matchesSearch(s, "   ")).toBe(true);
    expect(matchesSearch(s, "инженер", "Мейн: Инженер")).toBe(true);
    expect(matchesSearch(s, "капитан", "Мейн: Инженер")).toBe(false);
  });

  it("supports fuzzy transliteration RU <-> EN and 'ё' folding", () => {
    const s1 = row({ name: "Corvax Solaris", address: "ss14s://corvax.net" });
    expect(matchesSearch(s1, "корвакс")).toBe(true);
    expect(matchesSearch(s1, "соларис")).toBe(true);

    const s2 = row({ name: "Бебра Станция", address: "ss14s://bebra.space" });
    expect(matchesSearch(s2, "bebra")).toBe(true);

    const s3 = row({ name: "Зелёный Чай", address: "ss14s://tea.org" });
    expect(matchesSearch(s3, "зеленый")).toBe(true);
    expect(matchesSearch(s3, "зелёный")).toBe(true);
  });
});

describe("sorting", () => {
  const a = row({ address: "a", name: "Alpha", players: 5 });
  const b = row({ address: "b", name: "Bravo", players: 30 });
  const c = row({ address: "c", name: "Charlie", players: 30 });
  const names = (list: ServerRow[]) => list.map((s) => s.name);

  it("the default is by players, busiest first, ties by name", () => {
    expect(names(sortServers([a, c, b], DEFAULT_SORT))).toEqual(["Bravo", "Charlie", "Alpha"]);
  });

  it("by name both ways", () => {
    expect(names(sortServers([c, a, b], { key: "name", desc: false }))).toEqual([
      "Alpha",
      "Bravo",
      "Charlie",
    ]);
    expect(names(sortServers([c, a, b], { key: "name", desc: true }))).toEqual([
      "Charlie",
      "Bravo",
      "Alpha",
    ]);
  });

  it("does not change the list it was given", () => {
    const input = [c, a, b];
    sortServers(input, { key: "name", desc: false });
    expect(names(input)).toEqual(["Charlie", "Alpha", "Bravo"]);
  });

  it("ping: unknown goes last in both directions", () => {
    const pings = { a: 80, b: 200 };
    expect(names(sortServers([c, b, a], { key: "ping", desc: false }, pings))).toEqual([
      "Alpha",
      "Bravo",
      "Charlie",
    ]);
    expect(names(sortServers([c, b, a], { key: "ping", desc: true }, pings))).toEqual([
      "Bravo",
      "Alpha",
      "Charlie",
    ]);
  });

  it("round time: longest round first; servers without a round last", () => {
    const long = row({ address: "l", name: "Long", run_level: 1, round_start_time: "2026-10-03T08:00:00Z" });
    const short = row({ address: "s", name: "Short", run_level: 1, round_start_time: "2026-10-03T11:00:00Z" });
    const lobby = row({ address: "x", name: "Lobby", run_level: 0 });
    expect(names(sortServers([lobby, short, long], { key: "time", desc: true }))).toEqual([
      "Long",
      "Short",
      "Lobby",
    ]);
  });

  it("a header click flips the same column and starts others sensibly", () => {
    expect(nextSort({ key: "players", desc: true }, "players")).toEqual({ key: "players", desc: false });
    expect(nextSort({ key: "players", desc: true }, "name")).toEqual({ key: "name", desc: false });
    expect(nextSort({ key: "name", desc: false }, "time")).toEqual({ key: "time", desc: true });
  });
});

describe("names", () => {
  it("regions in both languages, unknown codes as they are", () => {
    expect(regionName("eu_w", "ru")).toBe("Западная Европа");
    expect(regionName("eu_w", "en")).toBe("Western Europe");
    expect(regionName("mars", "en")).toBe("mars");
  });

  it("languages by their own names where the runtime knows them", () => {
    expect(languageName("ru")).toBe("Русский");
    expect(languageName("RU-ru")).toBe("Русский");
    expect(languageName("de")).toBe("Deutsch");
    expect(languageName("not a code!")).toBe("not a code!");
  });

  it("files codes of the servers' own under the launcher's regions", () => {
    expect(canonicalRegion("ru")).toBe("eu_e");
    expect(canonicalRegion("UA")).toBe("eu_e");
    expect(canonicalRegion("es")).toBe("eu_w");
    expect(canonicalRegion("eu")).toBe("eu_w");
    expect(canonicalRegion("na")).toBe("am_n_e");
    expect(canonicalRegion("us_w")).toBe("am_n_w");
    expect(canonicalRegion("eu_e")).toBe("eu_e");
    expect(canonicalRegion("mars")).toBeNull();
    expect(regionsOf(["region:ru", "region:eu_e", "region:mars"])).toEqual(["eu_e"]);
  });

  it("the region list holds no bare codes, and the filter finds such servers", () => {
    const servers = [row({ tags: ["region:ru"] }), row({ tags: ["region:es"] }), row({ tags: ["region:eu_e"] })];
    expect(availableValues(servers, "region:")).toEqual(["eu_e", "eu_w"]);
    const f = { ...emptyFilters(), regions: ["eu_e"] };
    expect(servers.filter((s) => matchesFilters(s, f))).toHaveLength(2);
  });

  it("lists the available values, most common first, one count per server", () => {
    const servers = [
      row({ tags: ["lang:ru", "lang:ru-RU"] }),
      row({ tags: ["lang:ru"] }),
      row({ tags: ["lang:en"] }),
      row({ tags: [] }),
    ];
    expect(availableValues(servers, "lang:", true)).toEqual(["ru", "en"]);
    expect(availableValues(servers, "region:")).toEqual([]);
  });
});
