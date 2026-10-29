import { describe, expect, it } from "vitest";
// The UI project has no Node types in tsconfig; this test runs in Node even so.
// @ts-expect-error -- no @types/node here
import { readFileSync } from "node:fs";
import { DEFAULT_CUSTOM, THEME_IDS, parseColor, tokensFor, type Rgba } from "./themes";

/**
 * P1-23: with `bg_type` = space/custom the page background used to be fully transparent, so a
 * light theme drew dark text on the near-black space canvas (≈1.2:1). The page sheet is now
 * `color-mix(in srgb, var(--bg) 94%, transparent)`; this test pins that arithmetic.
 */
const SURFACE_ALPHA = 0.94;
/** The darkest the canvas under the sheet gets: `bg-root` paint plus the optional dim layer. */
const CANVASES = ["#0A0B0E", "#000000"];

function composite(top: Rgba, bottom: Rgba, alpha: number): Rgba {
  const ch = (a: number, b: number) => Math.round(a * alpha + b * (1 - alpha));
  return { r: ch(top.r, bottom.r), g: ch(top.g, bottom.g), b: ch(top.b, bottom.b), a: 255 };
}

function luminance(c: Rgba): number {
  const linear = (v: number) => {
    const s = v / 255;
    return s <= 0.04045 ? s / 12.92 : ((s + 0.055) / 1.055) ** 2.4;
  };
  return 0.2126 * linear(c.r) + 0.7152 * linear(c.g) + 0.0722 * linear(c.b);
}

function contrast(a: Rgba, b: Rgba): number {
  const [hi, lo] = [luminance(a), luminance(b)].sort((x, y) => y - x);
  return (hi + 0.05) / (lo + 0.05);
}

describe("page surface over a transparent background", () => {
  it("keeps the text readable in every theme, over every canvas", () => {
    for (const id of THEME_IDS) {
      const tokens = tokensFor(id, DEFAULT_CUSTOM);
      const background = parseColor(tokens.background)!;
      const foreground = parseColor(tokens.foreground)!;
      for (const canvasHex of CANVASES) {
        const canvas = parseColor(canvasHex)!;
        const surface = composite(background, canvas, SURFACE_ALPHA);
        const ratio = contrast(foreground, surface);
        expect(ratio, `${id} over ${canvasHex}: ${ratio.toFixed(2)}:1`).toBeGreaterThanOrEqual(4.5);
      }
    }
  });

  it("is the contrast the old transparent page failed", () => {
    // The report's number: dark text straight on the space canvas.
    const light = parseColor(tokensFor("light", DEFAULT_CUSTOM).foreground)!;
    const canvas = parseColor("#0A0B0E")!;
    expect(contrast(light, canvas)).toBeLessThan(2);
  });

  it("app.css declares a nearly opaque page sheet and uses it for space/custom", () => {
    const css = readFileSync(new URL("../app.css", import.meta.url), "utf8");
    expect(css).toMatch(/--page-surface:[^;]*var\(--bg[^;]*94%,\s*transparent\);/);
    expect(css).toMatch(
      /html\[data-bg="space"\] \.shell,\s*html\[data-bg="custom"\] \.shell \{\s*background: var\(--page-surface\);/,
    );
  });
});
