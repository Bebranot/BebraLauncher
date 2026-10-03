import { describe, expect, it } from "vitest";
import type { ServerRow } from "./api";
import { tagValues } from "./store.svelte";

const row = (tags: string[]): ServerRow => ({
  address: "ss14s://a.test",
  name: null,
  players: 0,
  soft_max_players: 0,
  tags,
  map: null,
  preset: null,
  run_level: null,
  round_start_time: null,
  panic_bunker: null,
});

describe("tagValues", () => {
  it("counts tag values by prefix, most common first", () => {
    const servers = [
      row(["lang:ru", "region:eu_w"]),
      row(["lang:ru", "lang:en", "region:us_e"]),
      row(["lang:en", "rp:low"]),
      row(["lang:ru"]),
    ];
    expect(tagValues(servers, "lang:")).toEqual(["ru", "en"]);
    expect(tagValues(servers, "region:")).toEqual(["eu_w", "us_e"]);
    expect(tagValues(servers, "nothing:")).toEqual([]);
  });

  it("breaks ties alphabetically so the order is stable between refreshes", () => {
    expect(tagValues([row(["region:b", "region:a"])], "region:")).toEqual(["a", "b"]);
  });
});
