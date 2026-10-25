// Browser-only preview (`pnpm dev` in a normal browser): fake backend with sample data.
// Never part of the release build: main.ts imports it only when `import.meta.env.DEV`.
import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import type {
  AccountRow,
  PackRow,
  ServerDetails,
  ServerRow,
  Settings,
} from "./lib/api";

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
  tags: ["lang:ru", "rp:low", "region:eu_w"],
  map: "Box",
  preset: "Survival",
  run_level: 1,
  round_start_time: new Date(Date.now() - 47 * 60_000).toISOString(),
  panic_bunker: null,
  ...extra,
});

const servers: ServerRow[] = [
  server("ss14s://lizard.spacestation14.com", "Lizard — Wizards' Den", 72, 80, {
    tags: ["lang:en", "rp:med", "region:am_n_e"],
    preset: "Secret",
  }),
  server("ss14s://example-one.test", "Сектор Фронтир | RP", 41, 60, {
    map: "Saltern",
    preset: "Extended",
    tags: ["lang:ru", "rp:high", "region:eu_e"],
  }),
  server("ss14s://example-two.test", "Лобби-сервер", 12, 40, { run_level: 0, map: null, preset: null }),
  server("ss14s://example-three.test", "Тихая станция", 0, 30, { run_level: 2 }),
  server("ss14://example-four.test:1212", "Очень-очень длинное название сервера, которое не помещается в одну строку списка", 5, 50, {
    panic_bunker: true,
  }),
  server("ss14s://example-five.test", "Хардкор | Без респавна", 28, 45, {
    tags: ["lang:ru", "rp:none", "18+", "region:eu_w"],
    preset: "Nukeops",
    map: "Marathon",
  }),
  server("ss14s://example-six.test", "Deutscher Server", 17, 0, { tags: ["lang:de", "region:eu_w"] }),
];

let accounts: AccountRow[] = [
  { user_id: "11111111-1111-1111-1111-111111111111", username: "serjo13", expire_time: "2026-11-02T10:00:00Z", state: "valid" },
  { user_id: "22222222-2222-2222-2222-222222222222", username: "alexwo", expire_time: "2026-11-02T10:00:00Z", state: "valid" },
];

const DEFAULT_SETTINGS: Settings = {
  guest_name: "JoeGenero",
  compat_mode: false,
  minimize_on_play: true,
  theme: "dark",
  theme_gradient: false,
  theme_decor: false,
  update_repo: "Bebranot/vabecide-musor",
  update_check: true,
  update_auto: true,
  custom_theme: {
    background: "#25252A",
    accent: "#ADA24B",
    foreground: "#EEEEEE",
    popup: "#202025",
    gradient_start: "#2A2A31",
    gradient_end: "#2F3A36",
  },
  font: "",
  columns: { round_time: true, players: true, map: true, mode: true, ping: true },
  enabled_packs: ["russian.zip"],
  ignore_redial: false,
  verbose_log: false,
  proxy: "",
  language: "auto",
  last_account: null,
  favorites: ["ss14s://example-two.test", "ss14s://private.example:1212"],
  history: [
    {
      address: "ss14s://example-one.test",
      name: null,
      last_played: new Date().toISOString(),
      play_seconds: 3 * 3600 + 25 * 60,
      account: null,
    },
  ],
};
let settings: Settings = structuredClone(DEFAULT_SETTINGS);

const packs: PackRow[] = [
  {
    id: "russian.zip",
    name: "Русский",
    description: "Перевод интерфейса",
    files: 371,
    bytes: 2_400_000,
    cultures: ["ru-RU"],
    problems: [],
    enabled: true,
  },
  {
    id: "sneaky.zip",
    name: "sneaky",
    description: "",
    files: 1,
    bytes: 10,
    cultures: [],
    problems: ["only Locale/<culture>/*.ftl text files are allowed: Assemblies/Patch.dll"],
    enabled: false,
  },
];

const details: Record<string, ServerDetails> = {
  "ss14s://lizard.spacestation14.com": {
    description: "Official Wizards' Den test server.\nExpect bugs, updates and short rounds.",
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

let tokenSet = false;
let found = false;
const updateStatus = (phase = "idle", done = 0) => ({
  phase,
  current: "0.2.000",
  latest: found
    ? {
        version: "0.2.001",
        notes: "### Исправлено\n- Пример исправления из релиза\n- Ещё одно с `кодом`",
        published: "2026-10-05T10:00:00Z",
        page_url: "https://github.com/example/example/releases/tag/v0.2.001",
        size: 36_000_000,
      }
    : null,
  done,
  total: 36_000_000,
  error: null,
  can_install: phase === "ready",
});

/** A check that finds a new version, downloads it and gets ready (the real flow, faster). */
async function fakeUpdate() {
  await emit("update", updateStatus("checking"));
  await sleep(700);
  found = true;
  await emit("update", updateStatus("idle"));
  await sleep(500);
  for (let done = 0; done <= 36_000_000; done += 3_600_000) {
    await emit("update", updateStatus("downloading", done));
    await sleep(200);
  }
  await emit("update", updateStatus("ready", 36_000_000));
}

const CHANGELOG = `# Журнал изменений

## [0.2.000] — 2026-10-04

### Добавлено
- Обновление лаунчера прямо в нём
- Журнал изменений

### Изменено
- Новый дизайн

## [0.1.000] — 2026-10-03

### Добавлено
- Первая версия
`;

const pingFor = (address: string) => 20 + ((address.length * 37) % 260);

export function installMock() {
  mockIPC(
    (cmd, args) => {
      const a = (args ?? {}) as Record<string, unknown>;
      switch (cmd) {
        case "app_info":
          return { version: "0.2.0", data_dir: "C:\\Users\\you\\AppData\\Local\\BebraLauncher", loader_problem: null };
        case "list_servers":
          return servers;
        case "cached_servers":
          return servers.slice(0, 3);
        case "ping_servers":
          return (a.addresses as string[]).map((address) => {
            const known = servers.find((s) => s.address === address);
            return {
              address,
              ms: known || address.includes("private") ? pingFor(address) : null,
              status: address.includes("private")
                ? server(address, "Приватный сервер друзей", 3, 20, { tags: ["lang:ru"] })
                : null,
            };
          });
        case "server_info": {
          const d = details[a.address as string];
          if (!d) throw "timeout";
          return d;
        }
        case "news":
          return [
            { title: "Патч 1.2: новые карты и исправления", link: "https://spacestation14.com/post/a/", date: "Fri, 03 Oct 2026 10:00:00 +0000" },
            { title: "Space Station 14 в Steam: итоги месяца", link: "https://spacestation14.com/post/b/", date: "Mon, 28 Sep 2026 09:00:00 +0000" },
          ];
        case "select_account":
          settings.last_account = a.account ? String(a.account) : "";
          return null;
        case "list_accounts":
          return accounts;
        case "get_settings":
          return settings;
        case "set_settings": {
          const incoming = a.settings as Settings;
          settings = {
            ...incoming,
            favorites: settings.favorites,
            history: settings.history,
            last_account: settings.last_account,
          };
          return settings;
        }
        case "reset_settings":
          settings = { ...structuredClone(DEFAULT_SETTINGS), favorites: settings.favorites, history: settings.history };
          return settings;
        case "toggle_favorite": {
          const addr = a.address as string;
          settings.favorites = settings.favorites.includes(addr)
            ? settings.favorites.filter((x) => x !== addr)
            : [...settings.favorites, addr];
          return settings.favorites;
        }
        case "add_favorite": {
          const addr = (a.address as string).trim();
          if (!addr || addr.includes(" ")) throw "invalid";
          if (!settings.favorites.includes(addr)) settings.favorites = [...settings.favorites, addr];
          return settings.favorites;
        }
        case "raise_favorite": {
          const addr = a.address as string;
          settings.favorites = [addr, ...settings.favorites.filter((x) => x !== addr)];
          return settings.favorites;
        }
        case "list_packs":
          return packs.map((p) => ({ ...p, enabled: settings.enabled_packs.includes(p.id) }));
        case "open_packs_folder":
        case "open_logs_folder":
        case "open_data_folder":
          return null;
        case "open_url":
          console.log("open_url", a.url);
          return null;
        case "get_autostart":
        case "get_links_registered":
          return false;
        case "set_autostart":
        case "set_links_registered":
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
        case "dump_locale":
          return { files: 370, bytes: 2_400_000, version_id: 2, dir: "C:\\dump\\Locale" };
        case "diagnostics":
          return [
            { name: "Hub", ok: true, ms: 120, detail: "HTTP 200" },
            { name: "Auth", ok: true, ms: 95, detail: "HTTP 401" },
            { name: "Builds", ok: false, ms: null, detail: "error sending request" },
            { name: "Loader", ok: true, ms: null, detail: "C:\\Program Files\\BebraLauncher\\loader\\SS14.Loader.exe" },
          ];
        case "content_versions":
          return [{ id: 2, fork: "ss14", version: "v2", engine: "291.0.0", files: 6780, last_used: "2026-10-03" }];
        case "launch_preview":
          void (async () => {
            await sleep(600);
            await emit("play", {
              kind: "preview",
              command: "C:\\loader\\SS14.Loader.exe [0] engine.zip --username serjo13 ROBUST_AUTH_TOKEN=<hidden>",
            });
          })();
          return null;
        case "update_status":
          return updateStatus();
        case "check_update":
          void fakeUpdate();
          return { ...updateStatus(), phase: "checking" };
        case "download_update":
          return updateStatus();
        case "install_update":
          return null;
        case "update_token_state":
          return tokenSet;
        case "set_update_token":
          tokenSet = String(a.token ?? "").trim() !== "";
          return tokenSet;
        case "changelog":
          return CHANGELOG;
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
      }
    },
    { shouldMockEvents: true },
  );
}
