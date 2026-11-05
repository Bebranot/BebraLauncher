// App-wide state. One instance, shared by the components.
import { untrack } from "svelte";
import {
  api,
  errorText,
  type AccountRow,
  type AppInfo,
  type BackgroundRow,
  type HistoryEntry,
  type NewsItem,
  type OwnNewsItem,
  type PendingAction,
  type PlayEvent,
  type RunningClient,
  type UpdateStatus,
  type PlayStage,
  type RedialEvent,
  type ServerDetails,
  type ServerRow,
  type Settings,
} from "./api";
import {
  DEFAULT_SORT,
  availableValues,
  canonicalRegion,
  emptyFilters,
  matchesFilters,
  matchesSearch,
  normalizePlayerRange,
  normalizeRp,
  parseFilters,
  primaryLanguage,
  pruneFilters,
  sortServers,
  type FilterState,
  type Sort,
  type SortKey,
} from "./filters";
import { applyDocumentLang, i18n } from "./i18n.svelte";
import { unseen } from "./news";
import { OPTIMIZATION_KEYS } from "./optimization";
import { DEFAULT_CUSTOM, applyTheme } from "./themes";

export type Tab = "favorites" | "servers" | "recent" | "news" | "updates" | "mods" | "options";
const TABS: Tab[] = ["favorites", "servers", "recent", "news", "updates", "mods", "options"];

export type Dialog =
  | { kind: "direct" }
  | { kind: "addfav" }
  | { kind: "editfav"; address: string; name?: string | null }
  | { kind: "confirm_connect"; server: ServerRow }
  | { kind: "login"; relogin?: { userId: string; username: string } };

export type PlayState =
  | { phase: "idle" }
  | {
      phase: "preparing";
      stage: PlayStage;
      done: number;
      total: number;
      /** Payload received so far; `done` may count files instead of bytes. */
      bytes: number;
      unit: "items" | "bytes";
    }
  | { phase: "running"; playsAs: string }
  | { phase: "failed"; code: string; message: string }
  | { phase: "exited"; code: number | null; logTail: string[] };

export type DetailsState = ServerDetails | "loading" | "failed";

export type NewsState =
  | { state: "idle" }
  | { state: "loading" }
  | { state: "ready"; items: NewsItem[] }
  | { state: "failed"; error: string };

export type OwnNewsState =
  | { state: "idle" }
  | { state: "loading" }
  | { state: "ready"; items: OwnNewsItem[] }
  | { state: "failed"; error: string };

export type NewsSource = "official" | "own";

const SEEN_NEWS_KEY = "bebra.news.seen";

function readSeenNews(): string[] {
  try {
    const v: unknown = JSON.parse(localStorage.getItem(SEEN_NEWS_KEY) ?? "[]");
    return Array.isArray(v) ? v.filter((id): id is string => typeof id === "string") : [];
  } catch {
    return [];
  }
}

/** Most addresses one ping request carries, backend limit included (P1-37). */
const PING_LIMIT = 200;
/** Asking again after a filter or sort change, once the player stops clicking (P1-37). */
const PING_REFRESH_DELAY_MS = 150;
/** A link or a redial points at a row for a while, then stops (P1-35). */
const FOCUS_TIMEOUT_MS = 30_000;
const SAVE_DELAY_MS = 300;
/** A failed save is retried, but slowly: being offline must not spin the disk. */
const SAVE_RETRY_MS = 5_000;
const UI_KEY = "bebra.ui";

/**
 * The one-second tick round timers share (P1-39). It runs only while somebody holds it, so a
 * list without a running round does not repaint every second. A future `RoundTimer` component
 * can `clock.retain()` on its own instead of reading the whole store.
 */
export class SecondClock {
  now = $state(Date.now());
  private timer: ReturnType<typeof setInterval> | null = null;
  private holders = 0;

  /** Keeps the clock running; call the returned function when the timer is gone. */
  retain(): () => void {
    this.holders += 1;
    this.start();
    let released = false;
    return () => {
      if (released) return;
      released = true;
      this.holders -= 1;
      if (this.holders <= 0) this.stop();
    };
  }

  start() {
    if (this.timer !== null) return;
    this.now = Date.now();
    this.timer = setInterval(() => {
      // A hidden window needs no ticking; catch up as soon as it is shown again.
      if (typeof document !== "undefined" && document.visibilityState === "hidden") return;
      this.now = Date.now();
    }, 1000);
  }

  stop() {
    if (this.timer !== null) {
      clearInterval(this.timer);
      this.timer = null;
    }
  }

  get running(): boolean {
    return this.timer !== null;
  }
}

export const clock = new SecondClock();

/** A server that is not in the hub list: typed in, named by a link or by a redial. */
function placeholderRow(address: string, name: string | null = null): ServerRow {
  return {
    address,
    name,
    players: 0,
    soft_max_players: 0,
    tags: [],
    map: null,
    preset: null,
    run_level: null,
    round_start_time: null,
    panic_bunker: null,
  };
}

function loadUi(): { tab?: Tab; filters?: FilterState; sort?: Sort; filtersOpen?: boolean } {
  try {
    const raw = localStorage.getItem(UI_KEY);
    if (!raw) return {};
    const v = JSON.parse(raw) as Record<string, unknown>;
    const sortKeys: SortKey[] = ["name", "mode", "map", "ping", "time", "players"];
    const s = v.sort as { key?: unknown; desc?: unknown } | undefined;
    return {
      tab: TABS.includes(v.tab as Tab) ? (v.tab as Tab) : undefined,
      filters: parseFilters(v.filters),
      sort:
        s && sortKeys.includes(s.key as SortKey)
          ? { key: s.key as SortKey, desc: s.desc === true }
          : undefined,
      filtersOpen: v.filtersOpen === true,
    };
  } catch {
    return {};
  }
}

export class Store {
  info = $state<AppInfo | null>(null);
  settings = $state<Settings | null>(null);

  tab = $state<Tab>("servers");
  dialog = $state<Dialog | null>(null);

  servers = $state<ServerRow[]>([]);
  serversLoading = $state(false);
  serversError = $state<string | null>(null);
  /** The list on screen is the saved one: the hub has not answered yet (or at all). */
  serversFromCache = $state(false);
  /** Startup or a data load failed in a way the player can retry (P2-65). */
  initError = $state<string | null>(null);
  loadError = $state<string | null>(null);
  /** Status of servers the hub does not list (favourites by address), from asking them. */
  extra = $state<Record<string, ServerRow>>({});
  /** Milliseconds per server address; null = asked, no answer. */
  pings = $state<Record<string, number | null>>({});

  intelWarning = $state(false);
  accounts = $state<AccountRow[]>([]);
  /** User id of the account to play with; `null` = guest. */
  selectedAccount = $state<string | null>(null);

  search = $state("");
  filters = $state<FilterState>(emptyFilters());
  filtersOpen = $state(false);
  sort = $state<Sort>({ ...DEFAULT_SORT });

  /** A server that is not in the list but was asked for. */
  adhoc = $state<ServerRow | null>(null);
  /** Expand and scroll to this server once its row is on screen. */
  focusAddress = $state<string | null>(null);
  details = $state<Record<string, DetailsState>>({});

  play = $state<PlayState>({ phase: "idle" });
  lastPlayTarget = $state<{ address: string; name: string | null; account: string | null } | null>(null);
  /** Shown in the connecting box: the game asked to reconnect. */
  redial = $state<{ reason: string | null } | null>(null);
  /** A short message at the bottom of the window. */
  notice = $state<string | null>(null);
  /** Debug: the launch command shown by "preview" (secrets hidden). */
  preview = $state<string | null>(null);

  news = $state<NewsState>({ state: "idle" });
  ownNews = $state<OwnNewsState>({ state: "idle" });
  newsSource = $state<NewsSource>("official");
  seenOwnNews = $state<string[]>(readSeenNews());
  /** The launcher's own update, as the backend last said. */
  update = $state<UpdateStatus | null>(null);
  /** When the user last asked for a check (ms), to show it. */
  updateCheckedAt = $state<number | null>(null);

  /** Current time in milliseconds, ticked by the shared clock while round timers are in use. */
  get now(): number {
    return clock.now;
  }

  customBgUrl = $state<string | null>(null);
  backgrounds = $state<BackgroundRow[]>([]);
  backgroundsLoading = $state(false);
  /** Wallpaper previews by file id: each is asked for once (P1-29). */
  bgThumbnails = $state.raw<Record<string, string>>({});

  /** Some servers need pinging but only `PING_LIMIT` fit in one request (P1-37). */
  pingsTruncated = $state(false);
  /** The last ping request failed: the values on screen may be old (P1-37). */
  pingsStale = $state(false);

  private autoRefreshTimer: ReturnType<typeof setInterval> | null = null;
  private lastServersRefresh = 0;
  /** Only the answer to the newest refresh may reach the list (P1-33). */
  private serversGeneration = 0;
  private saveTimer: ReturnType<typeof setTimeout> | undefined;
  /** The file does not have the current settings yet (P1-34). */
  private settingsDirty = false;
  /** One write at a time; edits made meanwhile schedule the next one (P1-34). */
  private saveInFlight: Promise<void> | null = null;
  /** Player edits the file has not got yet, kept so a reload cannot roll them back (P1-34). */
  private unsaved: Partial<Settings> = {};
  /** `bg_dim`/`bg_blur` changes are collected and applied once per frame (P1-30). */
  private visualFrame: number | null = null;
  private visualPatch: { bg_dim?: number; bg_blur?: number } | null = null;
  private noticeTimer: ReturnType<typeof setTimeout> | undefined;
  private focusTimer: ReturnType<typeof setTimeout> | undefined;
  private pingTimer: ReturnType<typeof setTimeout> | undefined;
  /** `init` runs once per page; a failed start can be retried (P2-65). */
  private initializing: Promise<void> | null = null;
  private initialized = false;
  private subscribed = false;
  private effectsStarted = false;
  private envWatched = false;
  /** Everything to undo on `dispose`: backend listeners, DOM listeners, the effect root. */
  private disposers: (() => void)[] = [];
  /** Wallpaper ids already asked for; outside $state so asking does not restart the effect. */
  private thumbnailsRequested = new Set<string>();
  /** Which wallpaper is on screen, so an unrelated save does not read it from disk again. */
  private appliedBackgroundKey: string | null = null;

  readonly favorites = $derived(new Set(this.settings?.favorites ?? []));
  readonly history = $derived(new Map((this.settings?.history ?? []).map((h) => [h.address, h])));
  readonly columns = $derived(
    this.settings?.columns ?? { round_time: true, players: true, map: false, mode: false, ping: false },
  );

  readonly languages = $derived(availableValues(this.servers, "lang:", true));
  readonly regions = $derived(availableValues(this.servers, "region:"));

  /** The servers of the Servers tab: searched, filtered, sorted. */
  readonly visible = $derived(
    sortServers(
      this.servers.filter(
        (s) => this.matchesRow(s) && matchesFilters(s, this.filters),
      ),
      this.sort,
      this.pings,
    ),
  );

  /** The name the player gave a server: it is searched like the hub's own name (P2-47). */
  private customNameOf(address: string): string | null {
    return this.settings?.favorite_names?.[address] ?? null;
  }

  /** Search for one row, including the note and the favourite's custom name. */
  private matchesRow(s: ServerRow): boolean {
    return matchesSearch(
      s,
      this.search,
      this.settings?.server_notes?.[s.address],
      this.customNameOf(s.address),
    );
  }

  /** Which rows the ping column asks about. No pings inside, so asking does not change it (P1-37). */
  private readonly pingTargets = $derived(
    this.servers
      .filter((s) => this.matchesRow(s) && matchesFilters(s, this.filters))
      .map((s) => s.address)
      .join(" "),
  );

  /** Favourites in the order the player keeps them. Those the hub does not list use their own answer. */
  readonly favoriteRows = $derived(
    (this.settings?.favorites ?? []).map((address) => this.rowFor(address)),
  );

  readonly recentRows = $derived(
    (this.settings?.history ?? []).map((h) => this.rowFor(h.address, h.name)),
  );

  readonly adhocRow = $derived(
    this.adhoc && !this.servers.some((s) => s.address === this.adhoc!.address) ? this.adhoc : null,
  );

  readonly selectedAccountRow = $derived(
    this.accounts.find((a) => a.user_id === this.selectedAccount) ?? null,
  );

  runningClients = $state<RunningClient[]>([]);

  readonly busy = $derived(this.play.phase === "preparing");
  /** Anything but idle means the backend owns the machine: cleaning storage or starting a second
   * game must wait (P2-61). */
  readonly isGameBusy = $derived(this.play.phase !== "idle");

  isRunning(address: string): boolean {
    return this.runningClients.some((c) => c.address === address);
  }

  toggleTagFilter(tag: string) {
    const lower = tag.trim().toLowerCase();
    if (lower.startsWith("lang:")) {
      const val = primaryLanguage(lower.slice(5));
      const cur = this.filters.languages;
      this.filters = {
        ...this.filters,
        languages: cur.includes(val) ? cur.filter((x) => x !== val) : [...cur, val],
      };
      this.filtersOpen = true;
    } else if (lower.startsWith("region:")) {
      // A server writes `region:ru`; the panel only knows `eu_e`. Without this, ticking its tag
      // hides that very server behind a value the panel cannot show (P1-32).
      const val = canonicalRegion(lower.slice(7));
      if (!val) return;
      const cur = this.filters.regions;
      this.filters = {
        ...this.filters,
        regions: cur.includes(val) ? cur.filter((x) => x !== val) : [...cur, val],
      };
      this.filtersOpen = true;
    } else if (lower.startsWith("rp:")) {
      // `rp:medium` and `rp:mid` mean the panel's `med`; unknown levels cannot be shown (P1-32).
      const val = normalizeRp(lower.slice(3));
      if (!val) return;
      const cur = this.filters.rp;
      this.filters = {
        ...this.filters,
        rp: cur.includes(val) ? cur.filter((x) => x !== val) : [...cur, val],
      };
      this.filtersOpen = true;
    } else if (lower === "18+") {
      const cur = this.filters.age;
      this.filters = {
        ...this.filters,
        age: cur.includes("yes") ? [] : ["yes"],
      };
      this.filtersOpen = true;
    }
  }

  /** Sets one player limit; `min > max` is swapped instead of hiding the whole list (P2-46). */
  setPlayerRange(which: "min" | "max", value: number | null) {
    const next: FilterState =
      which === "min"
        ? { ...this.filters, minPlayers: value }
        : { ...this.filters, maxPlayers: value };
    this.filters = normalizePlayerRange(next);
  }

  /** Drops filter values the loaded list no longer offers, so no invisible filter stays on. */
  private pruneFilters() {
    if (this.servers.length === 0) return;
    this.filters = pruneFilters(
      this.filters,
      availableValues(this.servers, "lang:", true),
      availableValues(this.servers, "region:"),
    );
  }

  /** The row for an address: from the hub, from its own answer, or a bare one. */
  rowFor(address: string, name: string | null = null): ServerRow {
    const favName = this.favoriteNameOf(address);
    return (
      this.servers.find((s) => s.address === address) ??
      this.extra[address] ??
      (this.adhoc?.address === address ? this.adhoc : null) ??
      placeholderRow(address, favName ?? name ?? this.history.get(address)?.name ?? null)
    );
  }

  /** Starts the launcher once; a second call joins the first (P2-65). */
  async init(): Promise<void> {
    if (this.initialized) return;
    if (this.initializing) return this.initializing;
    this.initializing = this.start().finally(() => {
      this.initializing = null;
    });
    return this.initializing;
  }

  private async start(): Promise<void> {
    this.initError = null;
    const ui = loadUi();
    if (ui.tab) this.tab = ui.tab;
    if (ui.filters) this.filters = ui.filters;
    if (ui.sort) this.sort = ui.sort;
    if (ui.filtersOpen) this.filtersOpen = true;

    // Listen first, so nothing the backend sends at startup (a redial) is missed. A broken
    // `listen` must not leave an empty launcher without a word: the error is shown with "retry".
    try {
      await this.subscribe();
    } catch (e) {
      this.initError = errorText(e);
    }
    void api.updateStatus().then((s) => (this.update ??= s)).catch(() => {});

    await Promise.all([this.loadInfo(), this.loadSettings(), this.loadAccounts()]);
    this.pickDefaultAccount();
    void this.checkIntelWarning();
    void this.loadBackgrounds();
    void this.loadRunningClients();
    void this.loadOwnNews();

    // Something to look at while the hub answers.
    const cached = await api.cachedServers().catch(() => []);
    if (cached.length > 0) {
      this.servers = cached;
      this.serversFromCache = true;
      this.pruneFilters();
    }
    void this.refreshServers();

    this.watchEnvironment();
    this.watchPings();
    this.initialized = true;
  }

  /** Retries what failed at startup or while loading the settings (P2-65). */
  async retryInit(): Promise<void> {
    this.initError = null;
    this.loadError = null;
    this.initialized = false;
    await this.init();
    await Promise.all([this.loadSettings(), this.loadAccounts()]);
    this.pickDefaultAccount();
  }

  /** Document and window listeners, removed by `dispose`. */
  private watchEnvironment() {
    if (this.envWatched) return;
    this.envWatched = true;
    if (typeof document !== "undefined") {
      const onVisibility = () => {
        if (document.visibilityState === "visible") {
          const intervalSec = this.settings?.auto_refresh_interval ?? 0;
          if (
            intervalSec > 0 &&
            Date.now() - this.lastServersRefresh >= intervalSec * 1000 &&
            !this.serversLoading &&
            this.play.phase !== "preparing"
          ) {
            void this.refreshServers();
          }
        } else {
          // Going away: the last edit must reach the file, not wait for the debounce (P1-34).
          this.flushSettingsNow();
        }
      };
      document.addEventListener("visibilitychange", onVisibility);
      this.disposers.push(() => document.removeEventListener("visibilitychange", onVisibility));
    }
    if (typeof window !== "undefined") {
      // Closing the window (also `close_on_play`) skips the save timer otherwise (P1-34).
      const onHide = () => this.flushSettingsNow();
      window.addEventListener("pagehide", onHide);
      this.disposers.push(() => window.removeEventListener("pagehide", onHide));
    }
  }

  /** Remember tab, filters and sorting; keep the ping column fresh (P1-37). */
  private watchPings() {
    if (this.effectsStarted) return;
    this.effectsStarted = true;
    const root = $effect.root(() => {
      $effect(() => {
        const snapshot = $state.snapshot({
          tab: this.tab,
          filters: this.filters,
          sort: this.sort,
          filtersOpen: this.filtersOpen,
        });
        try {
          localStorage.setItem(UI_KEY, JSON.stringify(snapshot));
        } catch {
          // No storage (private window, blocked): the choices are just not remembered.
        }
      });
      // The ping column needs pings as soon as it is switched on and whenever the rows it would
      // ask about change (P1-37). `pingTargets` holds no pings, so asking cannot restart this.
      $effect(() => {
        if (!this.columns.ping) return;
        void this.pingTargets;
        void this.sort.key;
        void this.sort.desc;
        untrack(() => this.schedulePings());
      });
      // The store no longer owns a ticker (P1-39): `RoundTimer` ticks only for a running round.
      // `clock` above is the shared mechanism for any future timer that needs one.
    });
    this.disposers.push(root);
  }

  /** Removes every subscription and timer the store owns. The page calls this on unload. */
  dispose() {
    for (const off of this.disposers.splice(0)) {
      try {
        off();
      } catch {
        // A listener that is already gone needs no care.
      }
    }
    this.subscribed = false;
    this.effectsStarted = false;
    this.envWatched = false;
    this.initialized = false;
    if (this.autoRefreshTimer) clearInterval(this.autoRefreshTimer);
    this.autoRefreshTimer = null;
    clearTimeout(this.saveTimer);
    clearTimeout(this.noticeTimer);
    clearTimeout(this.focusTimer);
    clearTimeout(this.pingTimer);
  }

  /**
   * Subscribes to the backend events, then replays what happened before the page could listen:
   * a cold-start link, a redial, the first play events (P1-18). Safe to call twice.
   */
  async subscribe() {
    if (this.subscribed) return;
    const offs: (() => void)[] = [];
    offs.push(await api.onPlay((e) => this.onPlayEvent(e)));
    offs.push(await api.onRedial((e) => this.onRedial(e)));
    offs.push(await api.onOpenLink((address) => this.onOpenLink(address)));
    offs.push(await api.onUpdate((s) => (this.update = s)));
    this.disposers.push(...offs);
    this.subscribed = true;
    // Tauri does not buffer events: what already happened is kept by the backend until asked.
    const pending = await api.takePendingActions().catch(() => []);
    for (const action of pending) this.replayPending(action);
  }

  /** The same handlers as the live events, oldest first. */
  private replayPending(action: PendingAction) {
    switch (action.event) {
      case "open-link":
        if (typeof action.payload === "string") this.onOpenLink(action.payload);
        break;
      case "redial":
        if (action.payload) this.onRedial(action.payload as RedialEvent);
        break;
      case "play":
        if (action.payload) this.onPlayEvent(action.payload as PlayEvent);
        break;
    }
  }

  private async loadInfo() {
    this.info = await api.appInfo().catch(() => null);
  }

  async loadSettings() {
    try {
      const loaded = await api.getSettings();
      if (!loaded) return;
      if (this.settingsDirty || this.saveInFlight) {
        // The backend knows favourites, history and the account; the player's unsaved edits must
        // not be rolled back by that answer (P1-34).
        this.settings = { ...loaded, ...this.unsaved };
      } else {
        this.settings = loaded;
      }
      this.applySettings();
    } catch (e) {
      // Silent before P2-65: the window looked normal with no settings at all.
      this.loadError = errorText(e);
    }
  }

  /** Puts the theme, font, language, wallpaper and refresh timer of the settings on screen. */
  applySettings() {
    this.applyAppearance();
    void this.loadActiveBackground();
    this.restartAutoRefresh();
  }

  /** Theme, font, language and UI scale: no IPC, cheap enough for every keystroke (P1-30). */
  private applyAppearance() {
    const s = this.settings;
    applyTheme({
      theme: s?.theme ?? "dark",
      gradient: s?.theme_gradient ?? true,
      decor: s?.theme_decor ?? true,
      custom: s?.custom_theme ?? DEFAULT_CUSTOM,
      font: s?.font ?? "",
      bg_type: s?.bg_type ?? "space",
    });
    i18n.preference = s?.language ?? "auto";
    applyDocumentLang();
    if (typeof document !== "undefined") {
      const scale = s?.launcher_ui_scale ?? 100;
      document.documentElement.style.zoom = String(scale / 100);
    }
  }

  async loadRunningClients() {
    this.runningClients = await api.runningClients().catch(() => []);
  }

  async loadBackgrounds() {
    this.backgroundsLoading = true;
    try {
      this.backgrounds = await api.listBackgrounds();
    } catch {
      this.backgrounds = [];
    } finally {
      this.backgroundsLoading = false;
    }
  }

  async loadActiveBackground() {
    this.appliedBackgroundKey = this.backgroundKey();
    if (this.settings?.bg_type === "custom" && this.settings?.bg_custom_name) {
      try {
        this.customBgUrl = await api.getBackground(this.settings.bg_custom_name);
      } catch {
        this.customBgUrl = null;
      }
    } else {
      this.customBgUrl = null;
    }
  }

  /**
   * The preview of one wallpaper. Each id is asked for once: a gallery of N files needs N
   * requests, not N², and updating the map must not restart the caller (P1-29).
   */
  async ensureThumbnail(id: string): Promise<void> {
    if (this.thumbnailsRequested.has(id)) return;
    this.thumbnailsRequested.add(id);
    try {
      const url = await api.getBackground(id);
      this.bgThumbnails = { ...this.bgThumbnails, [id]: url };
    } catch {
      // No preview: the tile keeps showing its placeholder.
    }
  }

  async uploadBackground(file: File): Promise<void> {
    return new Promise<void>((resolve, reject) => {
      const reader = new FileReader();
      reader.onload = async () => {
        try {
          const base64 = reader.result as string;
          const bg = await api.saveBackground(file.name, base64);
          await this.loadBackgrounds();
          this.updateSettings({ bg_type: "custom", bg_custom_name: bg.id });
          resolve();
        } catch (e) {
          this.say(errorText(e));
          reject(e);
        }
      };
      reader.onerror = () => reject(reader.error);
      reader.readAsDataURL(file);
    });
  }

  selectBackground(name: string) {
    this.updateSettings({ bg_type: "custom", bg_custom_name: name });
  }

  async deleteBackground(name: string) {
    try {
      await api.deleteBackground(name);
      await this.loadBackgrounds();
      if (this.settings?.bg_custom_name === name) {
        const remaining = this.backgrounds[0];
        if (remaining) {
          this.updateSettings({ bg_custom_name: remaining.id });
        } else {
          this.updateSettings({ bg_type: "space", bg_custom_name: null });
        }
      }
    } catch (e) {
      this.say(errorText(e));
    }
  }

  resetBackground() {
    this.updateSettings({ bg_type: "space" });
  }

  restartAutoRefresh() {
    if (this.autoRefreshTimer) {
      clearInterval(this.autoRefreshTimer);
      this.autoRefreshTimer = null;
    }
    const intervalSec = this.settings?.auto_refresh_interval ?? 0;
    if (intervalSec > 0) {
      this.autoRefreshTimer = setInterval(() => {
        if (
          document.visibilityState === "visible" &&
          !this.serversLoading &&
          this.tab === "servers" &&
          this.play.phase !== "preparing"
        ) {
          void this.refreshServers();
        }
      }, intervalSec * 1000);
    }
  }

  /**
   * Changes settings: the screen follows at once, the file is written a moment later (so typing
   * a name does not write on every key). Only the parts that changed are applied: no wallpaper
   * is re-read and no timer restarted for an unrelated field (P1-30).
   */
  updateSettings(patch: Partial<Settings>) {
    if (!this.settings) return;
    const before = this.settings;
    // The dim and blur sliders are pure CSS: collect them and apply once per frame.
    const { bg_dim, bg_blur, ...rest } = patch;
    const next = { ...before, ...rest };
    const changed = (key: keyof typeof rest) => key in rest && rest[key] !== before[key];

    this.settings = next;
    this.unsaved = { ...this.unsaved, ...rest };
    if (bg_dim !== undefined) this.unsaved.bg_dim = bg_dim;
    if (bg_blur !== undefined) this.unsaved.bg_blur = bg_blur;
    this.queueVisual({ bg_dim, bg_blur });

    if (
      changed("theme") ||
      changed("theme_gradient") ||
      changed("theme_decor") ||
      changed("custom_theme") ||
      changed("font") ||
      changed("language") ||
      changed("launcher_ui_scale") ||
      changed("bg_type")
    ) {
      this.applyAppearance();
    }
    if (changed("bg_type") || changed("bg_custom_name")) {
      if (this.backgroundKey() !== this.appliedBackgroundKey) void this.loadActiveBackground();
    }
    if (changed("auto_refresh_interval")) this.restartAutoRefresh();
    this.markSettingsDirty();
  }

  /** Writes unsaved settings right away: closing or hiding the window must not lose the last edit. */
  flushSettingsNow() {
    clearTimeout(this.saveTimer);
    this.foldVisual();
    void this.flushSettings();
  }

  private markSettingsDirty() {
    this.settingsDirty = true;
    clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => void this.flushSettings(), SAVE_DELAY_MS);
  }

  private backgroundKey(): string {
    return `${this.settings?.bg_type ?? ""}|${this.settings?.bg_custom_name ?? ""}`;
  }

  /** Coalesces the background look into one write per animation frame (P1-30). */
  private queueVisual(patch: { bg_dim?: number; bg_blur?: number }) {
    if (patch.bg_dim === undefined && patch.bg_blur === undefined) return;
    const merged = { ...this.visualPatch };
    if (patch.bg_dim !== undefined) merged.bg_dim = patch.bg_dim;
    if (patch.bg_blur !== undefined) merged.bg_blur = patch.bg_blur;
    this.visualPatch = merged;
    if (this.visualFrame !== null) return;
    const apply = () => {
      this.visualFrame = null;
      this.foldVisual();
    };
    this.visualFrame =
      typeof requestAnimationFrame === "function"
        ? requestAnimationFrame(apply)
        : (setTimeout(apply, 16) as unknown as number);
  }

  /** Pushes the collected dim/blur values into the settings once (before any save). */
  private foldVisual() {
    const patch = this.visualPatch;
    this.visualPatch = null;
    if (this.visualFrame !== null) {
      if (typeof cancelAnimationFrame === "function") cancelAnimationFrame(this.visualFrame);
      this.visualFrame = null;
    }
    if (patch && this.settings) this.settings = { ...this.settings, ...patch };
  }

  /**
   * Writes the settings file. Only one write runs at a time; an edit made while it runs is not
   * lost — it is written by the next round (P1-34).
   */
  private async flushSettings(): Promise<void> {
    if (!this.settings) return;
    if (this.saveInFlight) {
      this.settingsDirty = true;
      return this.saveInFlight;
    }
    this.foldVisual();
    this.settingsDirty = false;
    const snapshot = $state.snapshot(this.settings) as Settings;
    let failed = false;
    const run = (async () => {
      try {
        const saved = await api.setSettings(snapshot);
        // Take the cleaned-up values, unless the player changed something meanwhile.
        if (!this.settingsDirty) {
          const local = this.settings;
          this.unsaved = {};
          this.settings = local
            ? {
                ...saved,
                // Written by their own commands while this request ran; the answer is older.
                favorites: local.favorites,
                history: local.history,
                server_notes: local.server_notes,
                favorite_names: local.favorite_names,
                last_account: local.last_account,
              }
            : saved;
          this.applyAppearance();
          if (this.backgroundKey() !== this.appliedBackgroundKey) void this.loadActiveBackground();
          if (local && saved.auto_refresh_interval !== local.auto_refresh_interval) {
            this.restartAutoRefresh();
          }
        }
      } catch (e) {
        failed = true;
        this.settingsDirty = true;
        this.say(errorText(e));
      } finally {
        this.saveInFlight = null;
        if (this.settingsDirty) {
          clearTimeout(this.saveTimer);
          this.saveTimer = setTimeout(
            () => void this.flushSettings(),
            failed ? SAVE_RETRY_MS : SAVE_DELAY_MS,
          );
        }
      }
    })();
    this.saveInFlight = run;
    return run;
  }

  async loadAccounts() {
    try {
      this.accounts = await api.listAccounts();
    } catch (e) {
      this.accounts = [];
      this.loadError = errorText(e);
      return;
    }
    if (this.selectedAccount && !this.accounts.some((a) => a.user_id === this.selectedAccount)) {
      this.selectAccount(null);
    }
  }

  selectAccount(userId: string | null) {
    this.selectedAccount = userId;
    if (this.settings) {
      this.settings.last_account = userId ?? "";
    }
    void api.selectAccount(userId).catch(() => {});
  }

  private pickDefaultAccount() {
    const last = this.settings?.last_account;
    if (last === "") {
      this.selectedAccount = null; // the player chose to be a guest
    } else if (last && this.accounts.some((a) => a.user_id === last)) {
      this.selectedAccount = last;
    } else {
      // Never chosen (or that account is gone): the first account, if there is one.
      this.selectedAccount = this.accounts[0]?.user_id ?? null;
    }
  }

  /** Shows a short message for a while. */
  say(text: string, ms = 8000) {
    this.notice = text;
    clearTimeout(this.noticeTimer);
    this.noticeTimer = setTimeout(() => (this.notice = null), ms);
  }

  /** When the list was last received (ms); 0 until the first successful refresh. */
  get serversRefreshedAt(): number {
    return this.lastServersRefresh;
  }

  /**
   * Reloads the server list. Several callers may start at once (hub toggles, the timer, the
   * window becoming visible); only the newest answer is applied, and the refresh time only
   * moves on success (P1-33).
   */
  async refreshServers(): Promise<void> {
    const generation = ++this.serversGeneration;
    this.serversLoading = true;
    try {
      const rows = await api.listServers();
      if (generation !== this.serversGeneration) return; // a newer request is under way
      this.servers = rows;
      this.serversError = null;
      this.serversFromCache = false;
      this.lastServersRefresh = Date.now();
      this.pruneFilters();
    } catch (e) {
      if (generation === this.serversGeneration) {
        // Keep the old list on screen; just say the refresh failed.
        this.serversError = errorText(e);
      }
      return;
    } finally {
      if (generation === this.serversGeneration) this.serversLoading = false;
    }
    if (generation === this.serversGeneration) void this.refreshPings();
  }

  /** Asks for pings again once the player stops changing filters or sorting (P1-37). */
  private schedulePings() {
    clearTimeout(this.pingTimer);
    this.pingTimer = setTimeout(() => void this.refreshPings(), PING_REFRESH_DELAY_MS);
  }

  /**
   * Asks servers for their status: favourites, servers the hub does not list, and the visible
   * rows. At most `PING_LIMIT` addresses go in one request and the truncation is visible
   * (`pingsTruncated`); answers for servers that left the list are dropped, and a failed
   * request marks the values as old (P1-37).
   */
  async refreshPings() {
    const inHub = new Set(this.servers.map((s) => s.address));
    const wanted = new Set<string>();
    // Favourites and history first: their rows may have no other source of status.
    for (const a of this.settings?.favorites ?? []) wanted.add(a);
    for (const h of this.settings?.history ?? []) if (!inHub.has(h.address)) wanted.add(h.address);
    if (this.columns.ping) {
      for (const s of this.visible) wanted.add(s.address);
      for (const s of this.favoriteRows) wanted.add(s.address);
    }

    // What can be on screen; answers about anything else are stale.
    const known = new Set<string>([...inHub, ...wanted, ...(this.settings?.favorites ?? [])]);
    const addresses = [...wanted].slice(0, PING_LIMIT);
    this.pingsTruncated = wanted.size > PING_LIMIT;

    const pings: Record<string, number | null> = {};
    for (const [address, ms] of Object.entries(this.pings)) {
      if (known.has(address)) pings[address] = ms;
    }
    if (addresses.length === 0) {
      this.pings = pings;
      this.pingsStale = false;
      return;
    }

    let rows;
    try {
      rows = await api.pingServers(addresses);
    } catch {
      // Keep what is on screen, but say it may be old.
      this.pings = pings;
      this.pingsStale = true;
      return;
    }

    const extra = { ...this.extra };
    for (const r of rows) {
      pings[r.address] = r.ms;
      if (r.status && !inHub.has(r.address)) extra[r.address] = r.status;
    }
    this.pings = pings;
    this.extra = extra;
    this.pingsStale = false;
  }

  // ------------------------------------------------------------ favourites

  async toggleFavorite(address: string) {
    try {
      const favorites = await api.toggleFavorite(address);
      if (this.settings) this.settings = { ...this.settings, favorites };
    } catch (e) {
      // A failing toggle must say so, not look like a click that did nothing (P2-65).
      this.say(errorText(e));
    }
  }

  async addFavorite(address: string): Promise<string | null> {
    try {
      const favorites = await api.addFavorite(address);
      if (this.settings) this.settings = { ...this.settings, favorites };
      void this.refreshPings();
      return null;
    } catch (e) {
      return errorText(e);
    }
  }

  async raiseFavorite(address: string) {
    const favorites = await api.raiseFavorite(address).catch(() => null);
    if (favorites && this.settings) this.settings = { ...this.settings, favorites };
  }

  async moveFavorite(address: string, delta: number) {
    const favorites = await api.moveFavorite(address, delta).catch(() => null);
    if (favorites && this.settings) this.settings = { ...this.settings, favorites };
  }

  async editFavorite(oldAddress: string, newAddress: string, name?: string | null) {
    try {
      const favorites = await api.editFavorite(oldAddress, newAddress, name);
      if (this.settings) {
        const names = { ...(this.settings.favorite_names ?? {}) };
        if (oldAddress !== newAddress) {
          delete names[oldAddress];
        }
        if (name?.trim()) {
          names[newAddress] = name.trim();
        } else {
          delete names[newAddress];
        }
        this.settings = { ...this.settings, favorites, favorite_names: names };
      }
      void this.refreshPings();
    } catch (e) {
      this.say(errorText(e));
    }
  }

  favoriteNameOf(address: string): string | undefined {
    return this.settings?.favorite_names?.[address];
  }

  async setFavoriteName(address: string, name?: string | null) {
    try {
      const names = await api.setFavoriteName(address, name);
      if (this.settings) {
        this.settings = { ...this.settings, favorite_names: names };
      }
    } catch (e) {
      this.say(errorText(e));
    }
  }

  async checkIntelWarning() {
    this.intelWarning = await api.checkIntelWarning().catch(() => false);
  }

  async dismissIntelWarning() {
    this.intelWarning = false;
    await api.dismissIntelWarning().catch(() => {});
    if (this.settings) {
      this.settings = { ...this.settings, dismissed_intel_warning: true };
    }
  }

  historyOf(address: string): HistoryEntry | undefined {
    return this.history.get(address);
  }

  noteOf(address: string): string {
    return this.settings?.server_notes?.[address] ?? "";
  }

  async setServerNote(address: string, note: string) {
    if (!this.settings) return;
    const trimmed = note.trim();
    const updatedNotes = { ...(this.settings.server_notes ?? {}) };
    if (trimmed) {
      updatedNotes[address] = trimmed;
    } else {
      delete updatedNotes[address];
    }
    this.settings = { ...this.settings, server_notes: updatedNotes };
    const saved = await api.setServerNote(address, trimmed).catch(() => null);
    if (saved && this.settings) {
      this.settings = { ...this.settings, server_notes: saved };
    }
  }

  // ------------------------------------------------------------ details

  /** What the server says about itself; asked once per server and run of the launcher. */
  async loadDetails(address: string) {
    if (this.details[address]) return;
    this.details[address] = "loading";
    try {
      this.details[address] = await api.serverInfo(address);
    } catch {
      this.details[address] = "failed";
    }
  }

  // ------------------------------------------------------------ launcher updates

  /** A newer version of the launcher is known (the tab gets a dot). */
  readonly updateAvailable = $derived(this.update?.latest != null);

  async checkUpdate() {
    if (this.update?.phase === "checking" || this.update?.phase === "downloading") return;
    try {
      this.update = await api.checkUpdate();
    } catch (e) {
      this.say(errorText(e));
    }
    this.updateCheckedAt = Date.now();
  }

  async downloadUpdate() {
    try {
      this.update = await api.downloadUpdate();
    } catch (e) {
      this.say(errorText(e));
    }
  }

  async installUpdate(restart: boolean) {
    try {
      await api.installUpdate(restart);
    } catch (e) {
      this.say(errorText(e));
    }
  }

  // ------------------------------------------------------------ news

  /** Entries of the launcher's own news the player has not opened yet. */
  get ownNewsUnseen(): number {
    return this.ownNews.state === "ready" ? unseen(this.ownNews.items, this.seenOwnNews).length : 0;
  }

  async loadOwnNews(force = false) {
    if (this.ownNews.state === "loading") return;
    if (this.ownNews.state === "ready" && !force) return;
    this.ownNews = { state: "loading" };
    try {
      this.ownNews = { state: "ready", items: await api.ownNews() };
    } catch (e) {
      this.ownNews = { state: "failed", error: errorText(e) };
    }
  }

  /** Remembers that everything in the list has been seen, so it stops being marked as new. */
  markOwnNewsSeen() {
    if (this.ownNews.state !== "ready") return;
    this.seenOwnNews = this.ownNews.items.map((item) => item.id);
    try {
      localStorage.setItem(SEEN_NEWS_KEY, JSON.stringify(this.seenOwnNews));
    } catch {
      // No storage: the entries are marked as new again after a restart, nothing worse.
    }
  }

  async loadNews(force = false) {
    if (this.news.state === "loading") return;
    if (this.news.state === "ready" && !force) return;
    this.news = { state: "loading" };
    try {
      this.news = { state: "ready", items: await api.news() };
    } catch (e) {
      this.news = { state: "failed", error: errorText(e) };
    }
  }

  // ------------------------------------------------------------ connecting

  /** Connects to a server of the lists. */
  async connect(server: ServerRow, bypassConfirm = false) {
    if (this.busy) return;
    if (this.settings?.confirm_connect && !bypassConfirm) {
      this.dialog = { kind: "confirm_connect", server };
      return;
    }

    // Problems we already know about are said at once, in the same box as the others.
    const account = this.selectedAccountRow;
    const details = this.details[server.address];
    const known = typeof details === "object" ? details : null;
    if (account?.state === "expired") return this.fail("account_expired", account.username);
    if (!account && known?.auth === "required") return this.fail("auth_required", "");

    this.redial = null;
    this.lastPlayTarget = { address: server.address, name: server.name, account: this.selectedAccount };
    this.play = { phase: "preparing", stage: "account", done: 0, total: 0, bytes: 0, unit: "items" };
    try {
      await api.play(server.address, server.name, this.selectedAccount);
    } catch (e) {
      this.play = { phase: "failed", code: "other", message: errorText(e) };
    }
  }

  /** Reconnects to the last played server (e.g. after a crash). */
  async reconnectLast() {
    if (!this.lastPlayTarget || this.busy) return;
    const target = this.lastPlayTarget;
    this.redial = null;
    this.play = { phase: "preparing", stage: "account", done: 0, total: 0, bytes: 0, unit: "items" };
    try {
      await api.play(target.address, target.name, target.account);
    } catch (e) {
      this.play = { phase: "failed", code: "other", message: errorText(e) };
    }
  }

  exportConfigJson(): string {
    const s = this.settings;
    if (!s) return "{}";
    const exportData = {
      version: 1,
      exported_at: new Date().toISOString(),
      settings: {
        opt_profile: s.opt_profile,
        opt_custom: s.opt_custom,
        client_window_mode: s.client_window_mode,
        client_mute_unfocused: s.client_mute_unfocused,
        client_ui_scale: s.client_ui_scale,
        client_max_fps: s.client_max_fps,
        theme: s.theme,
        theme_gradient: s.theme_gradient,
        theme_decor: s.theme_decor,
        font: s.font,
        bg_type: s.bg_type,
        bg_dim: s.bg_dim,
        bg_blur: s.bg_blur,
        minimize_on_play: s.minimize_on_play,
        auto_clean_cache: s.auto_clean_cache,
      },
      favorites: [...s.favorites],
      favorite_names: { ...(s.favorite_names ?? {}) },
      server_notes: { ...(s.server_notes ?? {}) },
    };
    return JSON.stringify(exportData, null, 2);
  }

  async copyConfigToClipboard(): Promise<boolean> {
    try {
      await navigator.clipboard.writeText(this.exportConfigJson());
      this.say(t("opt.config.copied"));
      return true;
    } catch {
      this.say(t("opt.config.copyFailed"));
      return false;
    }
  }

  downloadConfigFile() {
    try {
      const blob = new Blob([this.exportConfigJson()], { type: "application/json" });
      const url = URL.createObjectURL(blob);
      const a = document.createElement("a");
      a.href = url;
      a.download = `bebra_settings_${new Date().toISOString().slice(0, 10)}.json`;
      a.click();
      // Revoking right after click() can cancel the download before WebView2 has read the blob.
      setTimeout(() => URL.revokeObjectURL(url), 10_000);
      this.say(t("opt.config.saved"));
    } catch {
      this.say(t("opt.config.saveFailed"));
    }
  }

  /**
   * Applies an exported configuration. Options go through the usual settings save; favourites,
   * their names and notes belong to the launcher (`merged_from_ui` ignores the UI's copy), so
   * they are added through their own commands — imported favourites are added, never removed.
   */
  async importConfigJson(raw: string): Promise<{ success: boolean; error?: string }> {
    const isObject = (v: unknown): v is Record<string, unknown> =>
      typeof v === "object" && v !== null && !Array.isArray(v);
    const strings = (v: unknown): [string, string][] =>
      isObject(v)
        ? Object.entries(v).filter((e): e is [string, string] => typeof e[1] === "string")
        : [];
    try {
      const data: unknown = JSON.parse(raw);
      if (!isObject(data)) {
        this.say(t("opt.config.badFormat"));
        return { success: false, error: t("opt.config.badFormat") };
      }

      const partial: Partial<Settings> = {};
      if (isObject(data.settings)) {
        const s = data.settings;
        const take = <K extends keyof Settings>(key: K, type: "string" | "boolean" | "number") => {
          if (typeof s[key] === type) partial[key] = s[key] as Settings[K];
        };
        take("opt_profile", "string");
        if (isObject(s.opt_custom)) {
          const custom = s.opt_custom;
          // Known keys only; anything missing or not a boolean is off.
          partial.opt_custom = Object.fromEntries(
            OPTIMIZATION_KEYS.map((k) => [k, custom[k] === true]),
          ) as Settings["opt_custom"];
        }
        take("client_window_mode", "string");
        take("client_mute_unfocused", "boolean");
        take("client_ui_scale", "string");
        take("client_max_fps", "number");
        take("theme", "string");
        take("theme_gradient", "boolean");
        take("theme_decor", "boolean");
        take("font", "string");
        take("bg_type", "string");
        take("bg_dim", "number");
        take("bg_blur", "number");
        take("minimize_on_play", "boolean");
        take("auto_clean_cache", "boolean");
      }
      this.updateSettings(partial);

      const have = new Set(this.settings?.favorites ?? []);
      const favorites = Array.isArray(data.favorites)
        ? data.favorites.filter((x): x is string => typeof x === "string" && !have.has(x))
        : [];
      for (const address of favorites) {
        const err = await this.addFavorite(address);
        if (err) throw new Error(err);
      }
      for (const [address, name] of strings(data.favorite_names)) {
        await this.setFavoriteName(address, name);
      }
      for (const [address, note] of strings(data.server_notes)) {
        await this.setServerNote(address, note);
      }

      this.say(t("opt.config.imported"));
      return { success: true };
    } catch (e) {
      const err = e instanceof Error ? e.message : String(e);
      this.say(t("opt.config.importFailed", { error: err }));
      return { success: false, error: err };
    }
  }

  /** Debug: prepares a server and shows the launch command without starting the game. */
  async launchPreview(address: string) {
    const typed = address.trim();
    if (!typed || this.busy) return;
    this.preview = null;
    this.redial = null;
    this.play = { phase: "preparing", stage: "account", done: 0, total: 0, bytes: 0, unit: "items" };
    try {
      await api.launchPreview(typed, this.selectedAccount);
    } catch (e) {
      this.play = { phase: "failed", code: "other", message: errorText(e) };
    }
  }

  /** Connects to an address typed in (it may not be in the list). */
  async connectTo(address: string) {
    const typed = address.trim();
    if (!typed || this.busy) return;
    await this.connect(this.rowFor(typed));
  }

  private fail(code: string, message: string) {
    this.play = { phase: "failed", code, message };
  }

  cancelPlay() {
    void api.cancelPlay();
  }

  /** Closes the error or crash message. */
  dismissPlay() {
    if (this.play.phase === "failed" || this.play.phase === "exited") {
      this.play = { phase: "idle" };
      this.redial = null;
    }
  }

  // ------------------------------------------------------------ accounts

  async logout(userId: string) {
    try {
      const told = await api.logout(userId);
      await this.loadAccounts();
      if (this.selectedAccount === userId) {
        this.selectAccount(this.accounts[0]?.user_id ?? null);
      }
      if (!told) return "offline";
    } catch (e) {
      this.say(errorText(e));
    }
    return null;
  }

  // ------------------------------------------------------------ events from the backend

  /**
   * Puts a server from a link or a redial on screen. A server of the hub can be hidden by the
   * current search or filters; then they are cleared, otherwise only a toast would appear (P1-35).
   */
  private reveal(address: string) {
    const inList = this.servers.find((s) => s.address === address);
    this.tab = "servers";
    if (inList) {
      const hidden = !this.matchesRow(inList) || !matchesFilters(inList, this.filters);
      if (hidden) {
        this.search = "";
        this.filters = emptyFilters();
      }
    } else {
      this.adhoc = placeholderRow(address, this.history.get(address)?.name ?? null);
    }
    this.focusAddress = address;
    // The row is shown and scrolled to at once; do not keep asking for it forever.
    clearTimeout(this.focusTimer);
    this.focusTimer = setTimeout(() => {
      if (this.focusAddress === address) this.focusAddress = null;
    }, FOCUS_TIMEOUT_MS);
  }

  /** A clicked ss14:// link: show the server and wait for the player to press Connect. */
  private onOpenLink(address: string) {
    if (this.busy) return;
    const typed = address.trim();
    this.reveal(typed);
    void this.loadDetails(typed);
    this.say(i18nNotice("link.opened"), 12000);
  }

  private onRedial(e: RedialEvent) {
    this.reveal(e.address);
    this.redial = { reason: e.reason };
    this.play = { phase: "preparing", stage: "account", done: 0, total: 0, bytes: 0, unit: "items" };
  }

  private onPlayEvent(e: PlayEvent) {
    switch (e.kind) {
      case "stage":
        this.play = { phase: "preparing", stage: e.stage, done: 0, total: 0, bytes: 0, unit: "items" };
        break;
      case "progress":
        this.play = {
          phase: "preparing",
          stage: e.stage,
          done: e.done,
          total: e.total,
          bytes: e.bytes ?? 0,
          unit: e.unit,
        };
        break;
      case "running":
        this.runningClients = [
          ...this.runningClients.filter((c) => c.pid !== e.pid),
          { pid: e.pid, playsAs: e.plays_as, address: e.address },
        ];
        this.play = { phase: "idle" };
        this.redial = null;
        // The backend recorded the server and the account for this game.
        void this.loadSettings();
        break;
      case "exited":
        this.runningClients = this.runningClients.filter((c) => c.pid !== e.pid);
        // A clean exit needs no message.
        if (e.code === 0) {
          if (this.play.phase !== "preparing") {
            this.play = { phase: "idle" };
          }
        } else {
          this.play = { phase: "exited", code: e.code, logTail: e.log_tail };
        }
        // Time played was added when the game closed.
        void this.loadSettings();
        break;
      case "failed":
        this.play = { phase: "failed", code: e.code, message: e.message };
        break;
      case "cancelled":
        this.play = { phase: "idle" };
        this.redial = null;
        break;
      case "preview":
        this.play = { phase: "idle" };
        this.preview = e.command;
        break;
    }
  }
}

import { t } from "./i18n.svelte";
function i18nNotice(key: "link.opened"): string {
  return t(key);
}

export const app = new Store();
