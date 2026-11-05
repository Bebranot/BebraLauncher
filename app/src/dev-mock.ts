// Browser-only preview (`pnpm dev` in a normal browser): fake backend with sample data.
// Never part of the release build: main.ts imports it only when `import.meta.env.DEV`.
import { emit } from "@tauri-apps/api/event";
import { mockIPC } from "@tauri-apps/api/mocks";
import type {
  AccountRow,
  HwidState,
  OptimizationConfig,
  OptimizationPresets,
  PackRow,
  ServerDetails,
  ServerRow,
  Settings,
} from "./lib/api";
import { OPTIMIZATION_KEYS, type OptimizationKey } from "./lib/optimization";

const optConfig = (on: string[]): OptimizationConfig => {
  const all = {} as Record<OptimizationKey, boolean>;
  for (const key of OPTIMIZATION_KEYS) all[key] = on.includes(key);
  return all;
};
const MOCK_OPT_BALANCED_KEYS = [
  "ao_lowres",
  "parallax_low",
  "particles_lite",
  "server_gc",
  "low_latency_gc",
  "high_priority",
  "no_throttling",
  "patch_outline",
];
const MOCK_OPT_BALANCED = optConfig(MOCK_OPT_BALANCED_KEYS);
const MOCK_OPT_PRESETS: OptimizationPresets = {
  standard: optConfig([]),
  balanced: MOCK_OPT_BALANCED,
  optimized: optConfig([
    ...MOCK_OPT_BALANCED_KEYS,
    "fast_lighting",
    "light_lowres",
    "ao_off",
    "calm_effects",
    "tile_edges_off",
    "audio_lite",
    "steady_fps",
    "prefer_dgpu",
    "patch_status_icons",
  ]),
};

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
  schema_version: 1,
  guest_name: "JoeGenero",
  compat_mode: false,
  discord_rpc_mode: "vanilla",
  minimize_on_play: true,
  theme: "dark",
  theme_gradient: false,
  theme_decor: false,
  update_repo: "Bebranot/BebraLauncher",
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
  favorite_names: { "ss14s://example-two.test": "Тестовое лобби" },
  launcher_ui_scale: 100,
  auto_clean_cache: true,
  dismissed_intel_warning: false,
  history: [
    {
      address: "ss14s://example-one.test",
      name: null,
      last_played: new Date().toISOString(),
      play_seconds: 3 * 3600 + 25 * 60,
      account: null,
    },
  ],
  hubs: [
    { url: "https://hub.spacestation14.com/", enabled: true },
  ],
  extra_args: "",
  enabled_patches: [],
  opt_profile: "balanced",
  opt_custom: { ...MOCK_OPT_BALANCED },
  bg_type: "space",
  bg_custom_name: null,
  bg_dim: 40,
  bg_blur: 0,
  client_window_mode: "default",
  client_mute_unfocused: true,
  client_ui_scale: "auto",
  client_max_fps: 0,
  close_on_play: false,
  auto_refresh_interval: 0,
  confirm_connect: false,
  server_notes: {
    "ss14s://example-one.test": "Любимый РП сервер (мейн: Инженер)",
  },
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

let mockPid = 1000;
/** Clients the mock "runs": `list_running_clients` and the header badge use them (P2-63). */
const running = new Map<number, { pid: number; playsAs: string; address?: string }>();
async function fakePlay(address?: string) {
  cancelled = false;
  const ev = (e: object) => emit("play", e);
  await ev({ kind: "stage", stage: "account" });
  await sleep(500);
  await ev({ kind: "stage", stage: "info" });
  await sleep(500);
  const total = 150 * 1024 * 1024;
  for (let done = 0; done <= total; done += total / 40) {
    if (cancelled) return ev({ kind: "cancelled" });
    await ev({ kind: "progress", stage: "files", done, total, bytes: done, unit: "bytes" });
    await sleep(120);
  }
  await ev({ kind: "stage", stage: "starting" });
  await sleep(600);
  const pid = mockPid++;
  // The game plays as the selected account, like the real backend (P2-63).
  const playsAs =
    accounts.find((x) => x.user_id === settings.last_account)?.username ??
    settings.guest_name ??
    "Guest";
  running.set(pid, { pid, playsAs, address });
  await ev({ kind: "running", pid, plays_as: playsAs, address });
}

let tokenSet = false;
let found = false;
let currentUpdate: ReturnType<typeof updateStatus> | null = null;

/** The wallpapers on disk: a saved one must appear in the gallery at once (P2-63). */
let backgrounds = [{ id: "sample_station.png", name: "sample_station", file_size: 1_240_000 }];

/** The mock session: credentials are accepted unless the name asks for a second factor. */
function fakeLogin(a: Record<string, unknown>) {
  const username = String(a.username ?? accounts[0]?.username ?? "");
  if (!a.password) return { status: "denied", code: "invalid_credentials", errors: [] };
  if (username === "tfa" && !a.tfaCode) {
    return { status: "denied", code: "tfa_required", errors: [] };
  }
  if (username === "tfa" && a.tfaCode !== "123456") {
    return { status: "denied", code: "tfa_invalid", errors: [] };
  }
  const account =
    accounts.find((x) => x.username === username) ??
    {
      user_id: `mock-${accounts.length + 1}`,
      username: username || "Guest",
      expire_time: "2027-01-01T00:00:00Z",
      state: "valid" as const,
    };
  if (!accounts.some((x) => x.user_id === account.user_id)) accounts = [...accounts, account];
  return { status: "ok", account };
}
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
  currentUpdate = updateStatus("checking");
  await emit("update", currentUpdate);
  await sleep(700);
  found = true;
  currentUpdate = updateStatus("idle");
  await emit("update", currentUpdate);
  await sleep(500);
  for (let done = 0; done <= 36_000_000; done += 3_600_000) {
    currentUpdate = updateStatus("downloading", done);
    await emit("update", currentUpdate);
    await sleep(200);
  }
  currentUpdate = updateStatus("ready", 36_000_000);
  await emit("update", currentUpdate);
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

/** The HWID controls: the real backend keeps these in the settings file. */
let mockHwid: HwidState = { auto: true, current: null, previous: null, patch: true };
const randomHwid = () =>
  Array.from({ length: 64 }, () => "0123456789abcdef"[Math.floor(Math.random() * 16)]).join("");

/**
 * `?demo&theme=nord&lang=en`: fictional servers and a neutral account for the README screenshots
 * (`scripts/readme-media`).
 */
function applyDemoMode(query: URLSearchParams) {
  const hour = 3600_000;
  servers.splice(
    0,
    servers.length,
    server("ss14s://aurora.example", "Aurora Station | Medium RP", 64, 80, {
      tags: ["lang:en", "rp:med", "region:eu_w"],
      preset: "Secret",
      map: "Meridian",
    }),
    server("ss14s://zarya.example", "Станция «Заря» | RP", 52, 70, {
      tags: ["lang:ru", "rp:high", "region:eu_e"],
      preset: "Extended",
      map: "Saltern",
    }),
    server("ss14s://outpost.example", "Outpost Nine", 38, 60, {
      tags: ["lang:en", "rp:low", "region:am_n_e"],
      preset: "Nukeops",
      map: "Marathon",
    }),
    server("ss14s://sector.example", "Дальний сектор | Без респавна", 27, 45, {
      tags: ["lang:ru", "rp:none", "region:eu_e"],
      preset: "Survival",
      map: "Packed",
    }),
    server("ss14s://kolonie.example", "Kolonie Nord", 17, 40, {
      tags: ["lang:de", "rp:med", "region:eu_w"],
      preset: "Traitor",
    }),
    server("ss14s://lowpop.example", "Quiet Hours", 9, 30, {
      tags: ["lang:en", "rp:low", "region:eu_w"],
      run_level: 0,
      map: null,
      preset: null,
    }),
    server("ss14s://mayak.example", "Маяк", 4, 25, {
      tags: ["lang:ru", "rp:low", "region:eu_e"],
      round_start_time: new Date(Date.now() - 2 * hour).toISOString(),
    }),
  );
  details["ss14s://aurora.example"] = {
    description: "A friendly medium-roleplay crew. New players welcome.",
    links: [
      { name: "Discord", url: "https://example.com/discord" },
      { name: "Rules", url: "https://example.com/rules" },
    ],
    auth: "required",
  };
  accounts = [
    { ...accounts[0], username: "station_engineer" },
    { ...accounts[1], username: "night_shift" },
  ];
  settings.guest_name = "Guest";
  settings.dismissed_intel_warning = true;
  settings.favorites = ["ss14s://aurora.example", "ss14s://zarya.example"];
  settings.favorite_names = {};
  settings.history = [];
  settings.server_notes = {};
  settings.language = query.get("lang") ?? "ru";
  settings.theme = query.get("theme") ?? "dark";
}

export function installMock() {
  const query = new URLSearchParams(location.search);
  if (query.has("demo")) applyDemoMode(query);
  mockIPC(
    (cmd, args) => {
      const a = (args ?? {}) as Record<string, unknown>;
      switch (cmd) {
        case "app_info":
          return { version: "0.2.0", data_dir: "C:\\Users\\you\\AppData\\Local\\BebraLauncher", loader_problem: null };
        case "take_pending_actions":
          return [];
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
        case "own_news":
          return [
            {
              id: "news-channel",
              date: "2026-11-05",
              title: { ru: "Новости BebraLauncher теперь здесь", en: "BebraLauncher news now live here" },
              body: {
                ru: "На этой вкладке будут появляться новости самого лаунчера: что вышло, что готовится и что важно знать.\n\nНовости Space Station 14 остались на соседней вкладке.",
                en: "This tab will carry news about the launcher itself: what has shipped, what is coming and what is worth knowing.\n\nSpace Station 14 news stays on the tab next to it.",
              },
              link: null,
            },
            {
              id: "themes",
              date: "2026-10-20",
              title: { ru: "Девять новых тем оформления" },
              body: { ru: "Загляните в настройки: появились новые темы, а свою можно передать другу через JSON." },
              link: "https://example.com/themes",
            },
          ];
        case "select_account":
          settings.last_account = a.account ? String(a.account) : "";
          return null;
        case "list_accounts":
          return accounts;
        case "get_settings":
          return settings;
        case "optimization_presets":
          return MOCK_OPT_PRESETS;
        case "get_hwid":
          return mockHwid;
        case "set_hwid_auto":
          mockHwid = { ...mockHwid, auto: Boolean(a.enabled) };
          return mockHwid;
        case "regenerate_hwid":
          mockHwid = { ...mockHwid, previous: mockHwid.current, current: randomHwid() };
          return mockHwid;
        case "restore_hwid":
          if (!mockHwid.previous) throw "Предыдущего HWID нет";
          mockHwid = { ...mockHwid, current: mockHwid.previous, previous: null };
          return mockHwid;
        case "set_settings": {
          const incoming = a.settings as Settings;
          // Like the real backend: what only the launcher writes is not undone by the UI's copy.
          settings = {
            ...incoming,
            schema_version: settings.schema_version,
            favorites: settings.favorites,
            favorite_names: settings.favorite_names,
            server_notes: settings.server_notes,
            history: settings.history,
            last_account: settings.last_account,
            enabled_patches: settings.enabled_patches,
            dismissed_intel_warning: settings.dismissed_intel_warning,
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
        case "move_favorite": {
          const addr = a.address as string;
          const delta = a.delta as number;
          const i = settings.favorites.indexOf(addr);
          if (i !== -1) {
            const target = Math.max(0, Math.min(settings.favorites.length - 1, i + delta));
            if (target !== i) {
              const item = settings.favorites.splice(i, 1)[0];
              settings.favorites.splice(target, 0, item);
            }
          }
          return settings.favorites;
        }
        case "edit_favorite": {
          const oldAddr = (a.oldAddress ?? a.old_address) as string;
          const newAddr = ((a.newAddress ?? a.new_address) as string).trim();
          const name = a.name as string | null | undefined;
          const i = settings.favorites.indexOf(oldAddr);
          if (i !== -1) {
            settings.favorites[i] = newAddr;
            settings.favorite_names = { ...(settings.favorite_names ?? {}) };
            delete settings.favorite_names[oldAddr];
            if (name && name.trim()) {
              settings.favorite_names[newAddr] = name.trim();
            }
          }
          return settings.favorites;
        }
        case "set_favorite_name": {
          const addr = a.address as string;
          const name = a.name as string | null | undefined;
          settings.favorite_names = { ...(settings.favorite_names ?? {}) };
          if (name && name.trim()) {
            settings.favorite_names[addr] = name.trim();
          } else {
            delete settings.favorite_names[addr];
          }
          return settings.favorite_names;
        }
        case "check_intel_warning":
          return !settings.dismissed_intel_warning;
        case "dismiss_intel_warning":
          settings.dismissed_intel_warning = true;
          return;
        case "get_recommended_hubs":
          return [
            { url: "https://hub.spacestation14.com/", name: "Wizards' Den" },
            { url: "https://hub.singularity14.co.uk/", name: "Singularity" },
            { url: "https://hub.simplestation.org/", name: "SimpleStation" },
            { url: "https://cdn.spacestationmultiverse.com/hub/", name: "Multiverse" },
          ];
        case "set_server_note": {
          const addr = a.address as string;
          const note = ((a.note as string) ?? "").trim();
          settings.server_notes = { ...(settings.server_notes ?? {}) };
          if (note) {
            settings.server_notes[addr] = note;
          } else {
            delete settings.server_notes[addr];
          }
          return settings.server_notes;
        }
        case "list_packs":
          return packs.map((p) => ({ ...p, enabled: settings.enabled_packs.includes(p.id) }));
        case "list_patches":
          return [
            { id: "Translator.dll", name: "Translator", file_size: 45_200, enabled: settings.enabled_patches.includes("Translator.dll") }
          ];
        case "delete_patch":
          settings.enabled_patches = settings.enabled_patches.filter((p) => p !== a.id);
          return null;
        case "install_patch": {
          const id = (a.name as string) || "Mod.dll";
          if (!settings.enabled_patches.includes(id)) settings.enabled_patches.push(id);
          return { id, name: id.replace(/\.dll$/i, ""), file_size: 32_000, enabled: true };
        }
        case "list_backgrounds":
          return backgrounds;
        case "save_background": {
          const id = (a.name as string) || "custom.png";
          const entry = { id, name: id.replace(/\.[^.]+$/, ""), file_size: 500_000 };
          if (!backgrounds.some((x) => x.id === id)) backgrounds = [...backgrounds, entry];
          return entry;
        }
        case "get_background":
          return "data:image/svg+xml;utf8,<svg xmlns='http://www.w3.org/2000/svg' width='800' height='600'><rect width='800' height='600' fill='%231a2234'/><text x='50%25' y='50%25' fill='%23fff' font-family='sans-serif' font-size='24' text-anchor='middle'>Custom Wallpaper Preview</text></svg>";
        case "delete_background": {
          const id = a.name as string;
          backgrounds = backgrounds.filter((x) => x.id !== id);
          return null;
        }
        case "open_backgrounds_folder":
        case "open_patches_folder":
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
        case "list_running_clients":
          return [...running.values()];
        case "clean_storage": {
          const freed = storageUsed * 0.4;
          storageUsed -= freed;
          return { freed, versions_removed: 2, engines_removed: 1, modules_removed: 0 };
        }
        case "clean_storage_all": {
          const freed = storageUsed * 0.95;
          storageUsed -= freed;
          return { freed, versions_removed: 5, engines_removed: 3, modules_removed: 0 };
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
          return (currentUpdate ??= updateStatus());
        case "check_update":
          void fakeUpdate();
          currentUpdate = { ...updateStatus(), phase: "checking" };
          return currentUpdate;
        case "download_update":
          void fakeUpdate();
          return (currentUpdate ??= updateStatus("downloading"));
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
          return fakeLogin(a);
        case "logout":
          accounts = accounts.filter((x) => x.user_id !== a.userId);
          return true;
        case "play":
          void fakePlay(a.address as string);
          return null;
        case "cancel_play":
          cancelled = true;
          return null;
        default:
          throw new Error("unmocked " + cmd);
      }
    },
    { shouldMockEvents: true },
  );
}
