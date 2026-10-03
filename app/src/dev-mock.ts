// Browser-only preview (`pnpm dev` in a normal browser): fake backend with sample data.
// Never part of the release build: main.ts imports it only when `import.meta.env.DEV`.
import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import type { AccountRow, ServerDetails, ServerRow, Settings } from "./lib/api";

const server = (
  address: string,
  name: string,
  players: number,
  max: number,
  extra: Partial<ServerRow> = {},
): ServerRow => ({
  address,
  name,
  players,
  soft_max_players: max,
  tags: ["lang:ru", "rp:low"],
  map: "Box",
  preset: "Survival",
  run_level: 1,
  round_start_time: new Date(Date.now() - 47 * 60_000).toISOString(),
  panic_bunker: null,
  ...extra,
});

const servers: ServerRow[] = [
  server("ss14s://lizard.spacestation14.com", "Lizard — Wizards' Den", 72, 80, { tags: ["lang:en", "rp:med", "region:us_e"] }),
  server("ss14s://example-one.test", "Сектор Фронтир | RP", 41, 60, { map: "Saltern", preset: "Extended" }),
  server("ss14s://example-two.test", "Лобби-сервер", 12, 40, { run_level: 0, map: null, preset: null }),
  server("ss14s://example-three.test", "Тихая станция", 0, 30, { run_level: 2 }),
  server("ss14://example-four.test:1212", "Очень-очень длинное название сервера, которое не помещается в одну строку списка", 5, 50, { panic_bunker: true }),
  server("ss14s://example-five.test", "Хардкор | Без респавна", 28, 45, { tags: ["lang:ru", "rp:high", "18+"] }),
];

let accounts: AccountRow[] = [
  { user_id: "11111111-1111-1111-1111-111111111111", username: "serjo13", expire_time: "2026-11-02T10:00:00Z", state: "valid" },
];

let settings: Settings = {
  guest_name: "JoeGenero",
  compat_mode: false,
  minimize_on_play: true,
  theme: "dark",
  language: "auto",
  last_account: null,
  favorites: ["ss14s://example-two.test"],
  history: [
    {
      address: "ss14s://example-one.test",
      name: null,
      last_played: new Date().toISOString(),
      play_seconds: 3 * 3600 + 25 * 60,
    },
  ],
};

const details: Record<string, ServerDetails> = {
  "ss14s://lizard.spacestation14.com": {
    description:
      "Official Wizards' Den test server.\nExpect bugs, updates and short rounds.",
    links: [
      { name: "Discord", url: "https://discord.gg/example" },
      { name: "Rules", url: "https://example.com/rules" },
    ],
    auth: "required",
  },
  "ss14s://example-two.test": { description: null, links: [], auth: "optional" },
};

let storageUsed = 410 * 1024 * 1024;

const sleep = (ms: number) => new Promise((r) => setTimeout(r, ms));
let cancelled = false;

async function fakePlay() {
  cancelled = false;
  const ev = (e: object) => emit("play", e);
  await ev({ kind: "stage", stage: "account" });
  await sleep(500);
  await ev({ kind: "stage", stage: "info" });
  await sleep(500);
  const total = 150 * 1024 * 1024;
  for (let done = 0; done <= total; done += total / 40) {
    if (cancelled) return ev({ kind: "cancelled" });
    await ev({ kind: "progress", stage: "files", done, total, unit: "bytes" });
    await sleep(120);
  }
  await ev({ kind: "stage", stage: "starting" });
  await sleep(600);
  await ev({ kind: "running", pid: 1234, plays_as: "serjo13" });
}

export function installMock() {
  mockIPC(
    (cmd, args) => {
      const a = (args ?? {}) as Record<string, unknown>;
      switch (cmd) {
        case "app_info":
          return { version: "0.1.0", data_dir: "C:\Users\you\AppData\Local\BebraLauncher", loader_problem: null };
        case "list_servers":
          return servers;
        case "list_accounts":
          return accounts;
        case "get_settings":
          return settings;
        case "set_settings":
          settings = a.settings as Settings;
          return settings;
        case "cached_servers":
          return servers.slice(0, 3);
        case "server_info": {
          const d = details[a.address as string];
          if (!d) throw "timeout";
          return d;
        }
        case "open_url":
          console.log("open_url", a.url);
          return null;
        case "get_autostart":
          return false;
        case "set_autostart":
          return a.enabled;
        case "storage_info":
          return {
            content: storageUsed * 0.7,
            engines: storageUsed * 0.2,
            modules: 0,
            other: storageUsed * 0.1,
            total: storageUsed,
          };
        case "clean_storage": {
          const freed = storageUsed * 0.4;
          storageUsed -= freed;
          return { freed, versions_removed: 2, engines_removed: 1, modules_removed: 0 };
        }
        case "toggle_favorite": {
          const addr = a.address as string;
          settings.favorites = settings.favorites.includes(addr)
            ? settings.favorites.filter((x) => x !== addr)
            : [...settings.favorites, addr];
          return settings.favorites;
        }
        case "login":
          return { status: "denied", code: "invalid_credentials", errors: [] };
        case "logout":
          accounts = accounts.filter((x) => x.user_id !== a.userId);
          return true;
        case "play":
          void fakePlay();
          return null;
        case "cancel_play":
          cancelled = true;
          return null;
        case "open_logs_folder":
          return null;
      }
    },
    { shouldMockEvents: true },
  );
}
