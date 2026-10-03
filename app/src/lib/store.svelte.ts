// App-wide state. One instance, shared by the components.
import { untrack } from "svelte";
import {
  api,
  errorText,
  type AccountRow,
  type AppInfo,
  type HistoryEntry,
  type NewsItem,
  type PlayEvent,
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
  emptyFilters,
  matchesFilters,
  matchesSearch,
  parseFilters,
  sortServers,
  type FilterState,
  type Sort,
  type SortKey,
} from "./filters";
import { i18n } from "./i18n.svelte";
import { DEFAULT_CUSTOM, applyTheme } from "./themes";

export type Tab = "favorites" | "servers" | "recent" | "news" | "updates" | "options";
const TABS: Tab[] = ["favorites", "servers", "recent", "news", "updates", "options"];

export type Dialog =
  | { kind: "direct" }
  | { kind: "addfav" }
  | { kind: "login"; relogin?: { userId: string; username: string } };

export type PlayState =
  | { phase: "idle" }
  | {
      phase: "preparing";
      stage: PlayStage;
      done: number;
      total: number;
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

const REFRESH_MS = 60_000;
/** Most servers pinged for the ping column in one go. */
const PING_VISIBLE = 60;
const SAVE_DELAY_MS = 300;
const UI_KEY = "bebra.ui";

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

class Store {
  info = $state<AppInfo | null>(null);
  settings = $state<Settings | null>(null);

  tab = $state<Tab>("servers");
  dialog = $state<Dialog | null>(null);

  servers = $state<ServerRow[]>([]);
  serversLoading = $state(false);
  serversError = $state<string | null>(null);
  /** The list on screen is the saved one: the hub has not answered yet (or at all). */
  serversFromCache = $state(false);
  /** Status of servers the hub does not list (favourites by address), from asking them. */
  extra = $state<Record<string, ServerRow>>({});
  /** Milliseconds per server address; null = asked, no answer. */
  pings = $state<Record<string, number | null>>({});

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
  /** Shown in the connecting box: the game asked to reconnect. */
  redial = $state<{ reason: string | null } | null>(null);
  /** A short message at the bottom of the window. */
  notice = $state<string | null>(null);
  /** Debug: the launch command shown by "preview" (secrets hidden). */
  preview = $state<string | null>(null);

  news = $state<NewsState>({ state: "idle" });
  /** The launcher's own update, as the backend last said. */
  update = $state<UpdateStatus | null>(null);
  /** When the user last asked for a check (ms), to show it. */
  updateCheckedAt = $state<number | null>(null);

  /** Current time in milliseconds, updated every second (for round timers). */
  now = $state(Date.now());

  private saveTimer: ReturnType<typeof setTimeout> | undefined;
  private editedSinceSave = false;
  private noticeTimer: ReturnType<typeof setTimeout> | undefined;

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
      this.servers.filter((s) => matchesSearch(s, this.search) && matchesFilters(s, this.filters)),
      this.sort,
      this.pings,
    ),
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

  readonly busy = $derived(this.play.phase === "preparing" || this.play.phase === "running");

  /** The row for an address: from the hub, from its own answer, or a bare one. */
  rowFor(address: string, name: string | null = null): ServerRow {
    return (
      this.servers.find((s) => s.address === address) ??
      this.extra[address] ??
      (this.adhoc?.address === address ? this.adhoc : null) ??
      placeholderRow(address, name ?? this.history.get(address)?.name ?? null)
    );
  }

  async init() {
    const ui = loadUi();
    if (ui.tab) this.tab = ui.tab;
    if (ui.filters) this.filters = ui.filters;
    if (ui.sort) this.sort = ui.sort;
    if (ui.filtersOpen) this.filtersOpen = true;

    // Listen first, so nothing the backend sends at startup (a redial) is missed.
    await api.onPlay((e) => this.onPlayEvent(e));
    await api.onRedial((e) => this.onRedial(e));
    await api.onOpenLink((address) => this.onOpenLink(address));
    await api.onUpdate((s) => (this.update = s));
    void api.updateStatus().then((s) => (this.update ??= s)).catch(() => {});

    await Promise.all([this.loadInfo(), this.loadSettings(), this.loadAccounts()]);
    this.pickDefaultAccount();

    // Something to look at while the hub answers.
    const cached = await api.cachedServers().catch(() => []);
    if (cached.length > 0) {
      this.servers = cached;
      this.serversFromCache = true;
    }
    void this.refreshServers();

    setInterval(() => (this.now = Date.now()), 1000);
    setInterval(() => {
      if (document.visibilityState === "visible" && !this.serversLoading) {
        void this.refreshServers();
      }
    }, REFRESH_MS);

    // Remember tab, filters and sorting between runs.
    $effect.root(() => {
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
      // The ping column needs pings as soon as it is switched on. Only the switch is watched:
      // asking for pings reads and writes the very data the list is made of.
      $effect(() => {
        if (this.columns.ping) untrack(() => void this.refreshPings());
      });
    });
  }

  private async loadInfo() {
    this.info = await api.appInfo().catch(() => null);
  }

  async loadSettings() {
    this.settings = await api.getSettings().catch(() => this.settings);
    this.applySettings();
  }

  /** Puts the theme, font and language of the settings on screen. */
  applySettings() {
    const s = this.settings;
    applyTheme({
      theme: s?.theme ?? "dark",
      gradient: s?.theme_gradient ?? true,
      decor: s?.theme_decor ?? true,
      custom: s?.custom_theme ?? DEFAULT_CUSTOM,
      font: s?.font ?? "",
    });
    i18n.preference = s?.language ?? "auto";
  }

  /**
   * Changes settings: the screen follows at once, the file is written a moment later (so typing
   * a name does not write on every key).
   */
  updateSettings(patch: Partial<Settings>) {
    if (!this.settings) return;
    this.settings = { ...this.settings, ...patch };
    this.applySettings();
    this.editedSinceSave = true;
    clearTimeout(this.saveTimer);
    this.saveTimer = setTimeout(() => void this.flushSettings(), SAVE_DELAY_MS);
  }

  private async flushSettings() {
    if (!this.settings) return;
    this.editedSinceSave = false;
    try {
      const saved = await api.setSettings($state.snapshot(this.settings) as Settings);
      // Take the cleaned-up values, unless the player changed something meanwhile.
      if (!this.editedSinceSave) {
        this.settings = { ...saved, favorites: this.settings.favorites, history: this.settings.history };
        this.applySettings();
      }
    } catch (e) {
      this.say(errorText(e));
    }
  }

  async loadAccounts() {
    this.accounts = await api.listAccounts().catch(() => []);
    if (this.selectedAccount && !this.accounts.some((a) => a.user_id === this.selectedAccount)) {
      this.selectedAccount = null;
    }
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

  async refreshServers() {
    this.serversLoading = true;
    try {
      this.servers = await api.listServers();
      this.serversError = null;
      this.serversFromCache = false;
    } catch (e) {
      // Keep the old list on screen; just say the refresh failed.
      this.serversError = errorText(e);
    } finally {
      this.serversLoading = false;
    }
    void this.refreshPings();
  }

  /**
   * Asks servers for their status: the ones the hub does not list but the player cares about,
   * and (with the ping column on) the first servers of the list.
   */
  async refreshPings() {
    const inHub = new Set(this.servers.map((s) => s.address));
    const wanted = new Set<string>();
    for (const a of this.settings?.favorites ?? []) if (!inHub.has(a)) wanted.add(a);
    for (const h of this.settings?.history ?? []) if (!inHub.has(h.address)) wanted.add(h.address);
    if (this.columns.ping) {
      for (const s of this.visible.slice(0, PING_VISIBLE)) wanted.add(s.address);
      for (const s of this.favoriteRows) wanted.add(s.address);
    }
    if (wanted.size === 0) return;

    const rows = await api.pingServers([...wanted]).catch(() => []);
    const pings = { ...this.pings };
    const extra = { ...this.extra };
    for (const r of rows) {
      pings[r.address] = r.ms;
      if (r.status && !inHub.has(r.address)) extra[r.address] = r.status;
    }
    this.pings = pings;
    this.extra = extra;
  }

  // ------------------------------------------------------------ favourites

  async toggleFavorite(address: string) {
    const favorites = await api.toggleFavorite(address).catch(() => null);
    if (favorites && this.settings) this.settings = { ...this.settings, favorites };
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

  historyOf(address: string): HistoryEntry | undefined {
    return this.history.get(address);
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
  async connect(server: ServerRow) {
    if (this.busy) return;

    // The account used on this server last time, if it still exists.
    const used = this.history.get(server.address)?.account;
    if (used !== undefined && used !== null) {
      if (used === "") this.selectedAccount = null;
      else if (this.accounts.some((a) => a.user_id === used)) this.selectedAccount = used;
    }

    // Problems we already know about are said at once, in the same box as the others.
    const account = this.selectedAccountRow;
    const details = this.details[server.address];
    const known = typeof details === "object" ? details : null;
    if (account?.state === "expired") return this.fail("account_expired", account.username);
    if (!account && known?.auth === "required") return this.fail("auth_required", "");

    this.redial = null;
    this.play = { phase: "preparing", stage: "account", done: 0, total: 0, unit: "items" };
    try {
      await api.play(server.address, server.name, this.selectedAccount);
    } catch (e) {
      this.play = { phase: "failed", code: "other", message: errorText(e) };
    }
  }

  /** Debug: prepares a server and shows the launch command without starting the game. */
  async launchPreview(address: string) {
    const typed = address.trim();
    if (!typed || this.busy) return;
    this.preview = null;
    this.redial = null;
    this.play = { phase: "preparing", stage: "account", done: 0, total: 0, unit: "items" };
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
      if (!told) return "offline";
    } catch (e) {
      this.say(errorText(e));
    }
    return null;
  }

  // ------------------------------------------------------------ events from the backend

  /** A clicked ss14:// link: show the server and wait for the player to press Connect. */
  private onOpenLink(address: string) {
    if (this.busy) return;
    const typed = address.trim();
    if (!this.servers.some((s) => s.address === typed)) this.adhoc = placeholderRow(typed);
    this.tab = "servers";
    this.focusAddress = typed;
    void this.loadDetails(typed);
    this.say(i18nNotice("link.opened"), 12000);
  }

  private onRedial(e: RedialEvent) {
    if (!this.servers.some((s) => s.address === e.address)) this.adhoc = placeholderRow(e.address);
    this.focusAddress = e.address;
    this.redial = { reason: e.reason };
    this.play = { phase: "preparing", stage: "account", done: 0, total: 0, unit: "items" };
  }

  private onPlayEvent(e: PlayEvent) {
    switch (e.kind) {
      case "stage":
        this.play = { phase: "preparing", stage: e.stage, done: 0, total: 0, unit: "items" };
        break;
      case "progress":
        this.play = {
          phase: "preparing",
          stage: e.stage,
          done: e.done,
          total: e.total,
          unit: e.unit,
        };
        break;
      case "running":
        this.play = { phase: "running", playsAs: e.plays_as };
        this.redial = null;
        // The backend recorded the server and the account for this game.
        void this.loadSettings();
        break;
      case "exited":
        // A clean exit needs no message.
        this.play =
          e.code === 0 ? { phase: "idle" } : { phase: "exited", code: e.code, logTail: e.log_tail };
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
