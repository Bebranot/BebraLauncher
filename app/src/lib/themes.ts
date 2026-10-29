// Colour themes. Every theme is a set of named colours; `applyTheme` turns the set into CSS
// variables on the page root, and the styles only ever use those variables.
//
// The set of colours follows the official launcher; the extra themes, the custom theme and the
// gradient/decor switches follow MusyaLoader. Colours are written as ARGB (`#AARRGGBB`), like
// the launcher's own theme files, and converted for CSS.

export const THEME_IDS = [
  "dark",
  "classic",
  "light",
  "dark_red",
  "dark_purple",
  "midnight_blue",
  "emerald_dusk",
  "copper_night",
  "nord",
  "dracula",
  "rose",
  "amber",
  "lagoon",
  "forest",
  "mono",
  "sand",
  "sky",
  "custom",
] as const;

export type ThemeId = (typeof THEME_IDS)[number];

/** Label keys for the theme list (see i18n). */
export const themes = THEME_IDS.map((id) => ({ id, label: `theme.${id}` as const }));

export interface Tokens {
  background: string;
  popup: string;
  foreground: string;
  muted: string;
  controlMid: string;
  controlHigh: string;
  /** The accent: headings, the line under the header, links. */
  accent: string;
  sub: string;
  stripeEdge: string;
  buttonHover: string;
  tabSelected: string;
  tabHover: string;
  separator: string;
  separatorFade: string;
  borderMid: string;
  borderHigh: string;
  altRow: string;
  scrollSep: string;
}

type Palette = Omit<Tokens, "scrollSep"> & { scrollSep?: string };

/**
 * Themes given by four colours, the rest is worked out the same way as for the user's own theme.
 * The light ones keep the popup colour a little darker than the page so buttons stay visible.
 */
const DERIVED = {
  nord: { background: "#2E3440", popup: "#3B4252", foreground: "#ECEFF4", accent: "#88C0D0" },
  dracula: { background: "#21222C", popup: "#282A36", foreground: "#F8F8F2", accent: "#BD93F9" },
  rose: { background: "#1C171E", popup: "#271F2A", foreground: "#F7E9EF", accent: "#F472B6" },
  amber: { background: "#191714", popup: "#23201B", foreground: "#F5EFE6", accent: "#F59E0B" },
  lagoon: { background: "#0E1A1D", popup: "#15262B", foreground: "#E3F6F5", accent: "#2DD4BF" },
  forest: { background: "#131C14", popup: "#1C271E", foreground: "#E8F2E4", accent: "#84CC16" },
  mono: { background: "#111111", popup: "#1B1B1B", foreground: "#EDEDED", accent: "#FFFFFF" },
  sand: { background: "#F7F1E5", popup: "#E9DEC6", foreground: "#2B2418", accent: "#9A6410" },
  sky: { background: "#EEF4FB", popup: "#D9E6F5", foreground: "#17222F", accent: "#2F6FD0" },
} as const;

type PaletteId = Exclude<ThemeId, "custom" | keyof typeof DERIVED>;

const PALETTES: Record<PaletteId, Palette> = {
  // The default: graphite with one calm accent. Surfaces are told apart by tone, not by lines.
  dark: {
    background: "#15161A",
    popup: "#1C1D24",
    foreground: "#E7E8EC",
    muted: "#6A6C77",
    controlMid: "#252731",
    controlHigh: "#323543",
    accent: "#6C8DFF",
    sub: "#9EA2B3",
    stripeEdge: "#2A2C33",
    buttonHover: "#2F323E",
    tabSelected: "#266C8DFF",
    tabHover: "#14FFFFFF",
    separator: "#26FFFFFF",
    separatorFade: "#00FFFFFF",
    borderMid: "#2C2F3A",
    borderHigh: "#3D4150",
    altRow: "#18191E",
    scrollSep: "#25272E",
  },
  // The official launcher's own dark theme (gold accent).
  classic: {
    background: "#25252A",
    popup: "#202025",
    foreground: "#EEEEEE",
    muted: "#666666",
    controlMid: "#464966",
    controlHigh: "#525259",
    accent: "#ADA24B",
    sub: "#AAAAAA",
    stripeEdge: "#525252",
    buttonHover: "#575B7F",
    tabSelected: "#CC3E6C45",
    tabHover: "#CC575B7F",
    separator: "#AA575B7F",
    separatorFade: "#00575B7F",
    borderMid: "#6B6F8F",
    borderHigh: "#8E94B8",
    altRow: "#26262C",
    scrollSep: "#2E2E35",
  },
  light: {
    background: "#F6F8FC",
    popup: "#FFFFFF",
    foreground: "#1B2230",
    muted: "#6F7A8D",
    controlMid: "#E6ECF5",
    controlHigh: "#2E7A66",
    accent: "#2E7A66",
    sub: "#6B778A",
    stripeEdge: "#D8E1EE",
    buttonHover: "#DCE5F2",
    tabSelected: "#C02E7A66",
    tabHover: "#AADCE5F2",
    separator: "#AABAC9DF",
    separatorFade: "#00BAC9DF",
    borderMid: "#B7C4D9",
    borderHigh: "#9CAECC",
    altRow: "#EEF3FA",
  },
  dark_red: {
    background: "#1E1719",
    popup: "#231A1D",
    foreground: "#F0E8EA",
    muted: "#A49297",
    controlMid: "#4A3136",
    controlHigh: "#8A3B48",
    accent: "#D06B4D",
    sub: "#B9A5AA",
    stripeEdge: "#5C3C43",
    buttonHover: "#66444B",
    tabSelected: "#CC8A3B48",
    tabHover: "#AA66444B",
    separator: "#AA775059",
    separatorFade: "#00775059",
    borderMid: "#7B565F",
    borderHigh: "#9A6A76",
    altRow: "#241B1E",
  },
  dark_purple: {
    background: "#1C1824",
    popup: "#211C2C",
    foreground: "#EEE9F7",
    muted: "#9E93B3",
    controlMid: "#41355A",
    controlHigh: "#6F4EA1",
    accent: "#B486D8",
    sub: "#B9AFCD",
    stripeEdge: "#544872",
    buttonHover: "#5B4D79",
    tabSelected: "#CC6F4EA1",
    tabHover: "#AA5B4D79",
    separator: "#AA6B5A8E",
    separatorFade: "#006B5A8E",
    borderMid: "#75659C",
    borderHigh: "#9383BC",
    altRow: "#231E2F",
  },
  midnight_blue: {
    background: "#141B24",
    popup: "#18212C",
    foreground: "#E9F1F9",
    muted: "#91A4B8",
    controlMid: "#2C4158",
    controlHigh: "#3C7FA6",
    accent: "#65B0C8",
    sub: "#A9BED1",
    stripeEdge: "#3C536B",
    buttonHover: "#35506A",
    tabSelected: "#CC3C7FA6",
    tabHover: "#AA35506A",
    separator: "#AA44617D",
    separatorFade: "#0044617D",
    borderMid: "#4D6986",
    borderHigh: "#6585A7",
    altRow: "#17202B",
  },
  emerald_dusk: {
    background: "#12211D",
    popup: "#162A25",
    foreground: "#E7F6F0",
    muted: "#95B3A8",
    controlMid: "#27473E",
    controlHigh: "#2F8F73",
    accent: "#78C9A9",
    sub: "#A8C6BA",
    stripeEdge: "#356154",
    buttonHover: "#2E574B",
    tabSelected: "#CC2F8F73",
    tabHover: "#AA2E574B",
    separator: "#AA3A6D5E",
    separatorFade: "#003A6D5E",
    borderMid: "#487A6A",
    borderHigh: "#5D9D88",
    altRow: "#172C26",
  },
  copper_night: {
    background: "#211915",
    popup: "#2A201A",
    foreground: "#F6ECE5",
    muted: "#B5A197",
    controlMid: "#5B4033",
    controlHigh: "#B06A45",
    accent: "#D79A62",
    sub: "#CAB4A8",
    stripeEdge: "#775242",
    buttonHover: "#724D3D",
    tabSelected: "#CCB06A45",
    tabHover: "#AA724D3D",
    separator: "#AA8A5E49",
    separatorFade: "#008A5E49",
    borderMid: "#936551",
    borderHigh: "#B07E67",
    altRow: "#2B211C",
  },
};

/** The colours of the user's own theme (`#RRGGBB`). */
export interface CustomColors {
  background: string;
  accent: string;
  foreground: string;
  popup: string;
  gradient_start: string;
  gradient_end: string;
}

export const DEFAULT_CUSTOM: CustomColors = {
  background: "#25252A",
  accent: "#ADA24B",
  foreground: "#EEEEEE",
  popup: "#202025",
  gradient_start: "#2A2A31",
  gradient_end: "#2F3A36",
};

export interface ThemeOptions {
  theme: string;
  gradient: boolean;
  decor: boolean;
  custom: CustomColors;
  /** Font family name, or empty for the built-in one. */
  font: string;
  bg_type?: string;
}

// ---------------------------------------------------------------- colour helpers

export interface Rgba {
  r: number;
  g: number;
  b: number;
  a: number;
}

/** Parses `#RRGGBB` or `#AARRGGBB`. Returns null for anything else. */
export function parseColor(text: string): Rgba | null {
  const m = /^#([0-9a-fA-F]{6}|[0-9a-fA-F]{8})$/.exec(text.trim());
  if (!m) return null;
  const h = m[1];
  const withAlpha = h.length === 8;
  const n = (i: number) => parseInt(h.slice(i, i + 2), 16);
  return withAlpha
    ? { a: n(0), r: n(2), g: n(4), b: n(6) }
    : { a: 255, r: n(0), g: n(2), b: n(4) };
}

const hex2 = (v: number) => v.toString(16).padStart(2, "0").toUpperCase();

/** `#RRGGBB`, or `#RRGGBBAA` when not opaque: what CSS wants. */
export function toCss(c: Rgba): string {
  const base = `#${hex2(c.r)}${hex2(c.g)}${hex2(c.b)}`;
  return c.a === 255 ? base : base + hex2(c.a);
}

/** Linear mix of two colours; `t` = 0 gives `a`, 1 gives `b`. Channels are truncated. */
export function mix(a: Rgba, b: Rgba, t: number): Rgba {
  const k = Math.min(1, Math.max(0, t));
  const ch = (x: number, y: number) => Math.trunc(x + (y - x) * k);
  return { a: 255, r: ch(a.r, b.r), g: ch(a.g, b.g), b: ch(a.b, b.b) };
}

const withAlpha = (c: Rgba, a: number): Rgba => ({ ...c, a });
const opaque = (c: Rgba): Rgba => withAlpha(c, 255);
const WHITE: Rgba = { r: 255, g: 255, b: 255, a: 255 };

/** WCAG relative luminance of a colour. */
export function luminance(c: Rgba): number {
  const channel = (v: number) => {
    const s = v / 255;
    return s <= 0.03928 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * channel(c.r) + 0.7152 * channel(c.g) + 0.0722 * channel(c.b);
}

/** WCAG contrast of two `#RRGGBB` colours; 1 for unparsable input. */
export function contrastOf(a: string, b: string): number {
  const ca = parseColor(a);
  const cb = parseColor(b);
  if (!ca || !cb) return 1;
  const la = luminance(ca);
  const lb = luminance(cb);
  return (Math.max(la, lb) + 0.05) / (Math.min(la, lb) + 0.05);
}

/** Minimum contrast for body text (WCAG AA). */
export const MIN_CONTRAST = 4.5;

/**
 * A secondary text colour that stays readable on every surface it is used on: mixed towards
 * the foreground until it reaches `min` contrast against all the given backgrounds (P2-49).
 */
function readable(text: string, backgrounds: string[], foreground: Rgba, min = MIN_CONTRAST): string {
  const start = parseColor(text);
  if (!start) return toCss(foreground);
  for (let step = 0; step <= 1.0001; step += 0.02) {
    const candidate = mix(start, foreground, step);
    const hex = toCss(candidate);
    if (backgrounds.every((bg) => contrastOf(hex, bg) >= min)) return hex;
  }
  return toCss(foreground);
}

function color(text: string, fallback: string): Rgba {
  return parseColor(text) ?? (parseColor(fallback) as Rgba);
}

/** Colours of the user's theme: everything else is worked out from these four. */
export function deriveCustom(custom: CustomColors): Tokens {
  const background = opaque(color(custom.background, DEFAULT_CUSTOM.background));
  const accent = opaque(color(custom.accent, DEFAULT_CUSTOM.accent));
  const foreground = opaque(color(custom.foreground, DEFAULT_CUSTOM.foreground));
  const popup = opaque(color(custom.popup, DEFAULT_CUSTOM.popup));

  const controlMid = mix(background, popup, 0.62);
  const controlHigh = mix(controlMid, accent, 0.35);
  const buttonHover = mix(controlMid, accent, 0.18);
  const borderHigh = mix(controlHigh, foreground, 0.28);
  const separator = withAlpha(mix(borderHigh, background, 0.4), 0xaa);
  const bgCss = toCss(background);
  const popupCss = toCss(popup);
  // Secondary text must stay readable on the page and in popups (P2-49).
  const muted = readable(toCss(mix(foreground, background, 0.58)), [bgCss, popupCss], foreground);
  const sub = readable(toCss(mix(foreground, background, 0.46)), [bgCss, popupCss], foreground);

  return {
    background: bgCss,
    popup: popupCss,
    foreground: toCss(foreground),
    muted,
    controlMid: toCss(controlMid),
    controlHigh: toCss(controlHigh),
    accent: toCss(accent),
    sub,
    stripeEdge: toCss(mix(background, popup, 0.45)),
    buttonHover: toCss(buttonHover),
    tabSelected: toCss(withAlpha(accent, 0xcc)),
    tabHover: toCss(withAlpha(mix(controlMid, accent, 0.2), 0xaa)),
    separator: toCss(separator),
    separatorFade: toCss(withAlpha(separator, 0)),
    borderMid: toCss(mix(popup, foreground, 0.18)),
    borderHigh: toCss(borderHigh),
    altRow: toCss(mix(background, popup, 0.28)),
    scrollSep: toCss(mix(background, foreground, 0.08)),
  };
}

function fromPalette(p: Palette): Tokens {
  const css = (s: string) => toCss(parseColor(s) as Rgba);
  const background = parseColor(p.background) as Rgba;
  const foreground = parseColor(p.foreground) as Rgba;
  const surfaces = [p.background, p.popup];
  return {
    background: css(p.background),
    popup: css(p.popup),
    foreground: css(p.foreground),
    // The palettes kept their historical greys; lift them where they were too faint (P2-49).
    muted: readable(css(p.muted), surfaces, foreground),
    controlMid: css(p.controlMid),
    controlHigh: css(p.controlHigh),
    accent: css(p.accent),
    sub: readable(css(p.sub), surfaces, foreground),
    stripeEdge: css(p.stripeEdge),
    buttonHover: css(p.buttonHover),
    tabSelected: css(p.tabSelected),
    tabHover: css(p.tabHover),
    separator: css(p.separator),
    separatorFade: css(p.separatorFade),
    borderMid: css(p.borderMid),
    borderHigh: css(p.borderHigh),
    altRow: css(p.altRow),
    scrollSep: p.scrollSep ? css(p.scrollSep) : toCss(mix(background, foreground, 0.08)),
  };
}

export function isThemeId(id: string): id is ThemeId {
  return (THEME_IDS as readonly string[]).includes(id);
}

/** The colours of a theme. Unknown ids give the default dark theme. */
export function tokensFor(theme: string, custom: CustomColors): Tokens {
  if (theme === "custom") return deriveCustom(custom);
  if (Object.hasOwn(DERIVED, theme)) {
    const c = DERIVED[theme as keyof typeof DERIVED];
    return deriveCustom({ ...DEFAULT_CUSTOM, ...c });
  }
  const palette = Object.hasOwn(PALETTES, theme)
    ? PALETTES[theme as PaletteId]
    : PALETTES.dark;
  return fromPalette(palette);
}

/** The page background: a soft diagonal gradient, or a flat colour. */
export function backgroundFill(
  theme: string,
  tokens: Tokens,
  custom: CustomColors,
  gradient: boolean,
): string {
  if (!gradient) return tokens.background;
  const background = parseColor(tokens.background) as Rgba;
  const popup = parseColor(tokens.popup) as Rgba;
  const controlHigh = parseColor(tokens.controlHigh) as Rgba;
  const isCustom = theme === "custom";
  const lightPage = isLight(tokens.background);

  let start = lightPage ? mix(background, WHITE, 0.35) : mix(background, popup, 0.18);
  let end = lightPage ? mix(background, controlHigh, 0.14) : mix(background, controlHigh, 0.28);
  if (isCustom) {
    start = opaque(color(custom.gradient_start, DEFAULT_CUSTOM.gradient_start));
    end = opaque(color(custom.gradient_end, DEFAULT_CUSTOM.gradient_end));
  }
  const middle = mix(mix(start, end, 0.5), popup, isCustom ? 0.12 : 0.2);
  return `linear-gradient(to bottom right, ${toCss(start)} 0%, ${toCss(middle)} 48%, ${toCss(end)} 100%)`;
}

/** The striped tile behind the lists, as a CSS `url(...)`. */
export function stripePattern(background: string, stripe: string): string {
  // Only the colour, not an alpha part: `#RRGGBB`.
  const bg = background.slice(0, 7);
  const fg = stripe.slice(0, 7);
  const svg =
    `<svg xmlns='http://www.w3.org/2000/svg' width='32' height='32'>` +
    `<rect width='32' height='32' fill='${bg}'/>` +
    `<path d='M0 8L24 32H8L0 24Z' fill='${fg}'/>` +
    `<path d='M8 0H24L32 8V24Z' fill='${fg}'/></svg>`;
  return `url("data:image/svg+xml,${encodeURIComponent(svg)}")`;
}

/** Is a colour closer to light than to dark? Decides the browser's own control colours. */
export function isLight(c: string): boolean {
  const p = parseColor(c);
  if (!p) return false;
  return 0.2126 * p.r + 0.7152 * p.g + 0.0722 * p.b > 150;
}

const FONT_STACK = `"Noto Sans", "Segoe UI", system-ui, sans-serif`;

/** The CSS `font-family` for the chosen font. The name was cleaned by the backend. */
export function fontFamily(font: string): string {
  const name = font.replace(/["\\;{}]/g, "").trim();
  return name ? `"${name}", ${FONT_STACK}` : FONT_STACK;
}

const VARIABLES: [keyof Tokens, string][] = [
  ["background", "--bg"],
  ["popup", "--popup"],
  ["foreground", "--fg"],
  ["muted", "--muted"],
  ["controlMid", "--ctrl-mid"],
  ["controlHigh", "--ctrl-high"],
  ["accent", "--accent"],
  ["sub", "--sub"],
  ["stripeEdge", "--stripe-edge"],
  ["buttonHover", "--btn-hover"],
  ["tabSelected", "--tab-selected"],
  ["tabHover", "--tab-hover"],
  ["separator", "--sep"],
  ["separatorFade", "--sep-fade"],
  ["borderMid", "--border-mid"],
  ["borderHigh", "--border-high"],
  ["altRow", "--alt-row"],
  ["scrollSep", "--scroll-sep"],
];

/** Puts a theme on the page. */
export function applyTheme(options: ThemeOptions, root: HTMLElement = document.documentElement) {
  const theme = isThemeId(options.theme) ? options.theme : "dark";
  const tokens = tokensFor(theme, options.custom);
  const set = (name: string, value: string) => root.style.setProperty(name, value);

  for (const [key, variable] of VARIABLES) set(variable, tokens[key]);
  set("--bg-fill", backgroundFill(theme, tokens, options.custom, options.gradient));
  set(
    "--decor",
    options.decor ? stripePattern(tokens.background, tokens.altRow) : "none",
  );
  set("--overlay", isLight(tokens.background) ? "#00000055" : "#000000B3");
  set("--on-accent", isLight(tokens.accent) ? "#14151A" : "#FFFFFF");
  set("--font", fontFamily(options.font));
  root.dataset.theme = theme;
  root.dataset.bg = options.bg_type ?? "space";
  root.style.colorScheme = isLight(tokens.background) ? "light" : "dark";
}

/** Colours of the custom theme as JSON text, for sharing. */
export function exportCustom(custom: CustomColors): string {
  return JSON.stringify(custom, null, 2);
}

/**
 * Reads a custom theme shared as JSON. Missing or broken colours keep the current ones;
 * returns null if the text is not a theme at all.
 */
export function importCustom(text: string, current: CustomColors): CustomColors | null {
  let value: unknown;
  try {
    value = JSON.parse(text);
  } catch {
    return null;
  }
  if (typeof value !== "object" || value === null || Array.isArray(value)) return null;
  const record = value as Record<string, unknown>;
  const pick = (key: keyof CustomColors): string => {
    const v = record[key];
    return typeof v === "string" && /^#[0-9a-fA-F]{6}$/.test(v.trim())
      ? v.trim().toUpperCase()
      : current[key];
  };
  const known = (Object.keys(DEFAULT_CUSTOM) as (keyof CustomColors)[]).some((k) =>
    Object.hasOwn(record, k),
  );
  if (!known) return null;
  return {
    background: pick("background"),
    accent: pick("accent"),
    foreground: pick("foreground"),
    popup: pick("popup"),
    gradient_start: pick("gradient_start"),
    gradient_end: pick("gradient_end"),
  };
}
