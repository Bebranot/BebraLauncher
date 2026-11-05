// Takes the README screenshots from the real interface on its fictional demo data.
// Needs `pnpm dev` running in app/. Uses installed Edge: the same engine as WebView2.
import { mkdir } from "node:fs/promises";
import { fileURLToPath } from "node:url";
import puppeteer from "puppeteer-core";

const BASE = process.env.UI_URL ?? "http://127.0.0.1:1420";
const EDGE =
  process.env.BROWSER ?? "C:\\Program Files (x86)\\Microsoft\\Edge\\Application\\msedge.exe";
const OUT = new URL("./public/shots/", import.meta.url);
const LANGS = ["ru", "en"];
const THEMES = ["dark", "light", "nord", "dracula", "amber", "forest", "copper_night", "classic"];

const sleep = (ms) => new Promise((resolve) => setTimeout(resolve, ms));

await mkdir(OUT, { recursive: true });
const browser = await puppeteer.launch({ executablePath: EDGE, headless: true });

async function open(lang, theme = "dark") {
  const page = await browser.newPage();
  await page.setViewport({ width: 1000, height: 650, deviceScaleFactor: 2 });
  await page.goto(`${BASE}/?demo&lang=${lang}&theme=${theme}`, { waitUntil: "networkidle0" });
  await sleep(1200);
  return page;
}

const save = (page, name) => page.screenshot({ path: fileURLToPath(new URL(`${name}.png`, OUT)) });

/** Opens a settings section. The joke promo rolls at random there: never let it into a shot. */
async function settings(page, section) {
  await page.click("#tab-options");
  await sleep(300);
  await page.evaluate((id) => {
    document.querySelector(".promo")?.remove();
    document.getElementById(id)?.scrollIntoView({ block: "start" });
  }, section);
  await sleep(500);
}

for (const lang of LANGS) {
  let page = await open(lang);
  await page.click("#tab-servers");
  await sleep(300);
  await page.evaluate(() => document.querySelector(".scroll [aria-expanded]")?.click());
  await sleep(600);
  await save(page, `servers-${lang}`);
  await page.close();

  page = await open(lang);
  await settings(page, "opt-appearance");
  await save(page, `appearance-${lang}`);
  await settings(page, "opt-optimization");
  await save(page, `optimization-${lang}`);
  await page.close();

  page = await open(lang);
  await page.click("#tab-servers");
  await sleep(300);
  await page.evaluate(() => {
    const row = document.querySelector(".scroll");
    [...row.querySelectorAll("button")].find((b) => b.classList.contains("soft"))?.click();
  });
  await sleep(3400);
  await save(page, `connecting-${lang}`);
  await page.close();

  for (const theme of THEMES) {
    page = await open(lang, theme);
    await page.click("#tab-servers");
    await sleep(400);
    await save(page, `theme-${theme}-${lang}`);
    await page.close();
  }
}

await browser.close();
