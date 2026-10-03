import { describe, expect, it } from "vitest";
import {
  DEFAULT_CUSTOM,
  THEME_IDS,
  backgroundFill,
  deriveCustom,
  exportCustom,
  fontFamily,
  importCustom,
  isLight,
  mix,
  parseColor,
  stripePattern,
  toCss,
  tokensFor,
  type Tokens,
} from "./themes";

describe("colour helpers", () => {
  it("parses RGB and ARGB and refuses the rest", () => {
    expect(parseColor("#25252A")).toEqual({ r: 0x25, g: 0x25, b: 0x2a, a: 255 });
    expect(parseColor("#CC3E6C45")).toEqual({ a: 0xcc, r: 0x3e, g: 0x6c, b: 0x45 });
    for (const bad of ["", "red", "#12345", "#GGGGGG", "25252A", "#1234567"]) {
      expect(parseColor(bad), bad).toBeNull();
    }
  });

  it("writes CSS colours, adding alpha only when needed", () => {
    expect(toCss({ r: 1, g: 2, b: 255, a: 255 })).toBe("#0102FF");
    expect(toCss({ r: 1, g: 2, b: 255, a: 0xaa })).toBe("#0102FFAA");
  });

  it("mixes channel by channel and truncates", () => {
    const black = { r: 0, g: 0, b: 0, a: 255 };
    const white = { r: 255, g: 255, b: 255, a: 255 };
    expect(mix(black, white, 0)).toEqual(black);
    expect(mix(black, white, 1)).toEqual(white);
    expect(mix(black, white, 0.5)).toEqual({ r: 127, g: 127, b: 127, a: 255 });
    // 255 + (0 - 255) * 0.5 = 127.5, cut to 127 like a byte cast
    expect(mix(white, black, 0.5)).toEqual({ r: 127, g: 127, b: 127, a: 255 });
    // t is kept inside 0..1
    expect(mix(black, white, 7)).toEqual(white);
    expect(mix(black, white, -3)).toEqual(black);
  });

  it("tells light from dark", () => {
    expect(isLight("#F6F8FC")).toBe(true);
    expect(isLight("#25252A")).toBe(false);
    expect(isLight("nonsense")).toBe(false);
  });
});

describe("themes", () => {
  const KEYS: (keyof Tokens)[] = [
    "background", "popup", "foreground", "muted", "controlMid", "controlHigh", "accent", "sub",
    "stripeEdge", "buttonHover", "tabSelected", "tabHover", "separator", "separatorFade",
    "borderMid", "borderHigh", "altRow", "scrollSep",
  ];

  it("every theme has every colour, all valid", () => {
    for (const id of THEME_IDS) {
      const t = tokensFor(id, DEFAULT_CUSTOM);
      for (const key of KEYS) {
        expect(parseColor(t[key]), `${id}.${key} = ${t[key]}`).not.toBeNull();
      }
    }
  });

  it("the default theme is the official launcher's dark one", () => {
    const t = tokensFor("dark", DEFAULT_CUSTOM);
    expect(t.background).toBe("#25252A");
    expect(t.popup).toBe("#202025");
    expect(t.accent).toBe("#ADA24B");
    expect(t.tabSelected).toBe("#3E6C45CC");
  });

  it("unknown ids fall back to dark", () => {
    expect(tokensFor("ocean", DEFAULT_CUSTOM)).toEqual(tokensFor("dark", DEFAULT_CUSTOM));
  });

  it("the custom theme keeps the chosen colours and derives the rest", () => {
    const custom = { ...DEFAULT_CUSTOM, background: "#102030", accent: "#FF8800" };
    const t = deriveCustom(custom);
    expect(t.background).toBe("#102030");
    expect(t.accent).toBe("#FF8800");
    expect(t.tabSelected).toBe("#FF8800CC");
    // derived colours are between the colours they come from
    const r = (c: string) => parseColor(c)!.r;
    expect(r(t.controlMid)).toBeGreaterThanOrEqual(Math.min(r(t.background), r(t.popup)));
    expect(r(t.controlMid)).toBeLessThanOrEqual(Math.max(r(t.background), r(t.popup)));
    // CSS colours carry alpha last: #RRGGBBAA
    expect(t.separator.slice(7)).toBe("AA");
    expect(t.separatorFade.slice(7)).toBe("00");
  });

  it("broken custom colours fall back to the defaults instead of breaking the page", () => {
    const t = deriveCustom({ ...DEFAULT_CUSTOM, background: "nope", accent: "" });
    expect(t.background).toBe(DEFAULT_CUSTOM.background);
    expect(t.accent).toBe(DEFAULT_CUSTOM.accent);
  });
});

describe("page background", () => {
  const dark = tokensFor("dark", DEFAULT_CUSTOM);

  it("is flat without the gradient", () => {
    expect(backgroundFill("dark", dark, DEFAULT_CUSTOM, false)).toBe("#25252A");
  });

  it("is a three-stop gradient with it", () => {
    const g = backgroundFill("dark", dark, DEFAULT_CUSTOM, true);
    expect(g).toMatch(/^linear-gradient\(to bottom right, #[0-9A-F]{6} 0%, #[0-9A-F]{6} 48%, #[0-9A-F]{6} 100%\)$/);
  });

  it("the custom theme uses its own gradient colours", () => {
    const custom = { ...DEFAULT_CUSTOM, gradient_start: "#112233", gradient_end: "#445566" };
    const g = backgroundFill("custom", deriveCustom(custom), custom, true);
    expect(g).toContain("#112233 0%");
    expect(g).toContain("#445566 100%");
  });

  it("the stripe tile is an inline image with both colours", () => {
    const url = stripePattern("#25252A", "#26262CAA");
    expect(url.startsWith('url("data:image/svg+xml,')).toBe(true);
    const svg = decodeURIComponent(url.slice('url("data:image/svg+xml,'.length, -2));
    expect(svg).toContain("fill='#25252A'");
    expect(svg).toContain("fill='#26262C'");
  });
});

describe("fonts", () => {
  it("empty means the built-in font, a name goes first", () => {
    expect(fontFamily("")).toMatch(/^"Noto Sans"/);
    expect(fontFamily("Consolas")).toMatch(/^"Consolas", "Noto Sans"/);
  });

  it("cannot break out of the declaration", () => {
    expect(fontFamily('Evil"; } body { display:none')).not.toMatch(/[;{}]/);
  });
});

describe("sharing a custom theme", () => {
  it("round-trips", () => {
    const custom = { ...DEFAULT_CUSTOM, background: "#010203", gradient_end: "#A0B0C0" };
    expect(importCustom(exportCustom(custom), DEFAULT_CUSTOM)).toEqual(custom);
  });

  it("keeps current colours for missing or broken ones", () => {
    const got = importCustom('{"background":"#111111","accent":"gold"}', DEFAULT_CUSTOM)!;
    expect(got.background).toBe("#111111");
    expect(got.accent).toBe(DEFAULT_CUSTOM.accent);
    expect(got.popup).toBe(DEFAULT_CUSTOM.popup);
  });

  it("refuses things that are not a theme", () => {
    for (const text of ["", "not json", "[]", "42", '"x"', "{}", '{"unrelated":1}']) {
      expect(importCustom(text, DEFAULT_CUSTOM), text).toBeNull();
    }
  });
});
