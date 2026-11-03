import { afterEach, beforeEach, describe, expect, it, vi } from "vitest";
import type { ServerRow, Settings } from "./api";

const mocks = vi.hoisted(() => ({
  api: {
    listServers: vi.fn(),
    pingServers: vi.fn(),
    getSettings: vi.fn(),
    setSettings: vi.fn(),
    getBackground: vi.fn(),
    onPlay: vi.fn(),
    onRedial: vi.fn(),
    onOpenLink: vi.fn(),
    onUpdate: vi.fn(),
    takePendingActions: vi.fn(),
  },
}));

vi.mock("./api", () => ({
  api: mocks.api,
  errorText: (e: unknown) =>
    typeof e === "string" ? e : e instanceof Error ? e.message : String(e),
}));

import { SecondClock, Store, clock } from "./store.svelte";
import { matchesFilters } from "./filters";

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

const settings = (over: Partial<Settings> = {}): Settings => ({
  schema_version: 1,
  guest_name: "Joe",
  compat_mode: false,
  discord_rpc: true,
  minimize_on_play: true,
  theme: "dark",
  theme_gradient: true,
  theme_decor: true,
  custom_theme: {
    background: "#000000",
    accent: "#111111",
    foreground: "#222222",
    popup: "#333333",
    gradient_start: "#444444",
    gradient_end: "#555555",
  },
  font: "",
  columns: { round_time: true, players: true, map: false, mode: false, ping: true },
  enabled_packs: [],
  ignore_redial: false,
  verbose_log: false,
  proxy: "",
  language: "en",
  update_repo: "",
  update_check: false,
  update_auto: false,
  last_account: null,
  favorites: [],
  history: [],
  hubs: [],
  extra_args: "",
  enabled_patches: [],
  opt_profile: "balanced",
  opt_fast_lighting: false,
  opt_unlock_fps: false,
  opt_high_priority: false,
  opt_server_gc: false,
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
  ...over,
});

function deferred<T>() {
  let resolve!: (value: T) => void;
  let reject!: (reason?: unknown) => void;
  const promise = new Promise<T>((res, rej) => {
    resolve = res;
    reject = rej;
  });
  return { promise, resolve, reject };
}

function makeStore(over: Partial<Settings> = {}) {
  const store = new Store();
  store.settings = settings(over);
  return store;
}

function stubPage() {
  const root = {
    style: { setProperty: vi.fn(), zoom: "", colorScheme: "" },
    dataset: {} as Record<string, string>,
  };
  (globalThis as Record<string, unknown>).document = {
    documentElement: root,
    visibilityState: "visible",
    addEventListener: vi.fn(),
  };
  (globalThis as Record<string, unknown>).window = { addEventListener: vi.fn() };
}

beforeEach(() => {
  stubPage();
  vi.clearAllMocks();
  // Otherwise a `mockResolvedValue` from one test leaks into the next one.
  mocks.api.takePendingActions.mockReset();
  mocks.api.onPlay.mockResolvedValue(() => {});
  mocks.api.onRedial.mockResolvedValue(() => {});
  mocks.api.onOpenLink.mockResolvedValue(() => {});
  mocks.api.onUpdate.mockResolvedValue(() => {});
  mocks.api.takePendingActions.mockResolvedValue([]);
  mocks.api.pingServers.mockResolvedValue([]);
  mocks.api.getSettings.mockResolvedValue(settings());
  mocks.api.setSettings.mockImplementation(async (s: Settings) => s);
  mocks.api.getBackground.mockResolvedValue("data:image/png;base64,AAAA");
});

afterEach(() => {
  vi.useRealTimers();
  delete (globalThis as Record<string, unknown>).document;
  delete (globalThis as Record<string, unknown>).window;
  delete (globalThis as Record<string, unknown>).requestAnimationFrame;
});

describe("cold-start events (P1-18)", () => {
  it("replays the kept events through the live handlers, oldest first", async () => {
    mocks.api.takePendingActions.mockResolvedValueOnce([
      { event: "play", payload: { kind: "exited", pid: 7, code: 3, log_tail: ["boom"] } },
      { event: "open-link", payload: "ss14s://cold.test:1212" },
      { event: "redial", payload: { address: "ss14s://cold.test:1212", reason: "restart" } },
      { event: "play", payload: { kind: "running", pid: 9, plays_as: "Joe", address: "ss14s://cold.test:1212" } },
    ]);
    const store = makeStore();
    await store.subscribe();

    expect(mocks.api.takePendingActions).toHaveBeenCalledTimes(1);
    // The running event is the newest: it wins over the older exit.
    expect(store.play).toEqual({ phase: "idle" });
    expect(store.runningClients).toEqual([
      { pid: 9, playsAs: "Joe", address: "ss14s://cold.test:1212" },
    ]);
    expect(store.redial).toBeNull();
    expect(store.focusAddress).toBe("ss14s://cold.test:1212");
  });

  it("does not replay the buffer twice", async () => {
    mocks.api.takePendingActions.mockResolvedValueOnce([
      { event: "play", payload: { kind: "failed", code: "other", message: "cold" } },
    ]);
    const store = makeStore();
    await store.subscribe();
    expect(store.play.phase).toBe("failed");

    store.dismissPlay();
    // A second call joins the existing subscription: it must not take the buffer again.
    await store.subscribe();
    expect(mocks.api.takePendingActions).toHaveBeenCalledTimes(1);
    expect(store.play).toEqual({ phase: "idle" });
  });
});

describe("tag filters from the list (P1-32)", () => {
  it("filters by the canonical region, so the clicked server stays", () => {
    const store = makeStore();
    const server = row({ tags: ["region:ru"] });

    store.toggleTagFilter("region:ru");
    expect(store.filters.regions).toEqual(["eu_e"]);
    expect(matchesFilters(server, store.filters)).toBe(true);

    store.toggleTagFilter("region:ru");
    expect(store.filters.regions).toEqual([]);
  });

  it("normalizes rp:medium to med", () => {
    const store = makeStore();
    const server = row({ tags: ["rp:medium"] });

    store.toggleTagFilter("rp:medium");
    expect(store.filters.rp).toEqual(["med"]);
    expect(matchesFilters(server, store.filters)).toBe(true);
  });

  it("does not add a filter the panel could never show", () => {
    const store = makeStore();
    store.toggleTagFilter("region:mars");
    store.toggleTagFilter("rp:chaotic");
    expect(store.filters.regions).toEqual([]);
    expect(store.filters.rp).toEqual([]);
  });
});

describe("server refresh (P1-33)", () => {
  it("applies only the newest answer", async () => {
    const first = deferred<ServerRow[]>();
    const second = deferred<ServerRow[]>();
    mocks.api.listServers.mockReturnValueOnce(first.promise).mockReturnValueOnce(second.promise);
    const store = makeStore();

    const older = store.refreshServers();
    const newer = store.refreshServers();
    second.resolve([row({ address: "ss14s://new.test" })]);
    await newer;
    first.resolve([row({ address: "ss14s://old.test" })]);
    await older;

    expect(store.servers.map((s) => s.address)).toEqual(["ss14s://new.test"]);
    expect(store.serversLoading).toBe(false);
  });

  it("moves the refresh time only after a successful answer", async () => {
    mocks.api.listServers.mockRejectedValueOnce("offline");
    const store = makeStore();
    await store.refreshServers();
    expect(store.serversError).toBe("offline");
    expect(store.serversRefreshedAt).toBe(0);

    mocks.api.listServers.mockResolvedValueOnce([row()]);
    await store.refreshServers();
    expect(store.serversError).toBeNull();
    expect(store.serversRefreshedAt).toBeGreaterThan(0);
  });
});

describe("settings saving (P1-34)", () => {
  it("keeps an edit made while the write is running", async () => {
    vi.useFakeTimers();
    const first = deferred<Settings>();
    mocks.api.setSettings.mockReturnValueOnce(first.promise);

    const store = makeStore();
    store.updateSettings({ guest_name: "first" });
    vi.advanceTimersByTime(300);
    expect(mocks.api.setSettings).toHaveBeenCalledTimes(1);

    // The player types again before the file is written.
    store.updateSettings({ guest_name: "second" });
    first.resolve(settings({ guest_name: "first" }));
    await vi.runOnlyPendingTimersAsync();

    expect(mocks.api.setSettings).toHaveBeenCalledTimes(2);
    const last = mocks.api.setSettings.mock.calls[1][0] as Settings;
    expect(last.guest_name).toBe("second");
    expect(store.settings?.guest_name).toBe("second");
  });

  it("does not roll back unsaved edits when the settings are reloaded", async () => {
    const store = makeStore();
    store.updateSettings({ guest_name: "typed" });
    mocks.api.getSettings.mockResolvedValueOnce(
      settings({ guest_name: "old", favorites: ["ss14s://fav.test"] }),
    );
    await store.loadSettings();
    expect(store.settings?.guest_name).toBe("typed");
    expect(store.settings?.favorites).toEqual(["ss14s://fav.test"]);
  });

  it("does not restart the wallpaper or the timer for an unrelated field (P1-30)", () => {
    const store = makeStore();
    const background = vi.spyOn(store, "loadActiveBackground");
    const timer = vi.spyOn(store, "restartAutoRefresh");

    store.updateSettings({ guest_name: "Joe2" });
    expect(background).not.toHaveBeenCalled();
    expect(timer).not.toHaveBeenCalled();

    store.updateSettings({ bg_type: "custom", bg_custom_name: "a.png" });
    expect(background).toHaveBeenCalledTimes(1);

    store.updateSettings({ auto_refresh_interval: 30 });
    expect(timer).toHaveBeenCalledTimes(1);
  });

  it("applies dim and blur once per frame", () => {
    const frames: FrameRequestCallback[] = [];
    (globalThis as Record<string, unknown>).requestAnimationFrame = (cb: FrameRequestCallback) => {
      frames.push(cb);
      return frames.length;
    };
    const store = makeStore();
    store.updateSettings({ bg_dim: 10 });
    store.updateSettings({ bg_dim: 11 });
    store.updateSettings({ bg_blur: 4 });
    expect(store.settings?.bg_dim).toBe(40);
    expect(frames).toHaveLength(1);
    frames[0](0);
    expect(store.settings?.bg_dim).toBe(11);
    expect(store.settings?.bg_blur).toBe(4);
  });
});

describe("links and redials (P1-35)", () => {
  it("clears the filters hiding the server a link points at", async () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-29T12:00:00Z"));
    const store = makeStore();
    store.servers = [row({ address: "ss14s://a.test", tags: ["lang:ru"] })];
    store.search = "nothing matches";
    store.filters = { ...store.filters, languages: ["de"] };

    mocks.api.takePendingActions.mockResolvedValueOnce([
      { event: "open-link", payload: "ss14s://a.test" },
    ]);
    await store.subscribe();

    expect(store.search).toBe("");
    expect(store.filters.languages).toEqual([]);
    expect(store.focusAddress).toBe("ss14s://a.test");
    expect(store.tab).toBe("servers");

    vi.advanceTimersByTime(30_000);
    expect(store.focusAddress).toBeNull();
  });

  it("shows a server the hub does not list", async () => {
    const store = makeStore();
    mocks.api.takePendingActions.mockResolvedValueOnce([
      { event: "redial", payload: { address: "ss14s://elsewhere.test:1212", reason: null } },
    ]);
    await store.subscribe();
    expect(store.adhoc?.address).toBe("ss14s://elsewhere.test:1212");
    expect(store.redial).toEqual({ reason: null });
  });
});

describe("pings (P1-37)", () => {
  it("asks for at most 200 visible servers and reports the truncation", async () => {
    const store = makeStore({ columns: { round_time: true, players: true, map: false, mode: false, ping: true } });
    store.servers = Array.from({ length: 250 }, (_, i) =>
      row({ address: `ss14s://s${i}.test`, name: `S${i}`, players: 250 - i }),
    );
    await store.refreshPings();
    const asked = mocks.api.pingServers.mock.calls[0][0] as string[];
    expect(asked).toHaveLength(200);
    expect(store.pingsTruncated).toBe(true);
  });

  it("drops answers for servers that are gone", async () => {
    const store = makeStore();
    store.servers = [row({ address: "ss14s://keep.test" })];
    store.pings = { "ss14s://keep.test": 5, "ss14s://gone.test": 6 };
    mocks.api.pingServers.mockResolvedValueOnce([
      { address: "ss14s://keep.test", ms: 9, status: null },
    ]);
    await store.refreshPings();
    expect(store.pings).toEqual({ "ss14s://keep.test": 9 });
  });

  it("marks the values as old when the request fails", async () => {
    const store = makeStore();
    store.servers = [row({ address: "ss14s://keep.test" })];
    store.pings = { "ss14s://keep.test": 5 };
    mocks.api.pingServers.mockRejectedValueOnce("offline");
    await store.refreshPings();
    expect(store.pings).toEqual({ "ss14s://keep.test": 5 });
    expect(store.pingsStale).toBe(true);
  });
});

describe("wallpaper previews (P1-29)", () => {
  it("asks for each wallpaper once", async () => {
    const store = makeStore();
    const first = store.ensureThumbnail("a.png");
    const second = store.ensureThumbnail("a.png");
    await Promise.all([first, second]);
    expect(mocks.api.getBackground).toHaveBeenCalledTimes(1);
    expect(store.bgThumbnails["a.png"]).toBe("data:image/png;base64,AAAA");
  });
});

describe("the second tick (P1-39)", () => {
  it("runs only while somebody holds it", () => {
    vi.useFakeTimers();
    vi.setSystemTime(new Date("2026-10-29T12:00:00Z"));
    const tick = new SecondClock();
    expect(tick.running).toBe(false);

    const release = tick.retain();
    expect(tick.running).toBe(true);
    const start = tick.now;
    vi.advanceTimersByTime(1000);
    expect(tick.now).toBe(start + 1000);

    release();
    expect(tick.running).toBe(false);
  });

  it("keeps the store's `now` for the components that still read it", () => {
    const store = makeStore();
    expect(store.now).toBe(clock.now);
  });

  it("exports and imports configuration correctly", async () => {
    const store = makeStore();
    await store.loadSettings();
    store.updateSettings({
      opt_profile: "optimized",
      favorites: ["ss14s://example.com"],
      favorite_names: { "ss14s://example.com": "Super Server" },
    });
    const exported = store.exportConfigJson();
    expect(exported).toContain('"opt_profile": "optimized"');
    expect(exported).toContain("Super Server");

    const res = await store.importConfigJson(exported);
    expect(res.success).toBe(true);
    expect(store.settings?.opt_profile).toBe("optimized");
  });
});
