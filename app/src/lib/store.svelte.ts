// App-wide state. One instance, shared by the components.
import {
  api,
  errorText,
  type AccountRow,
  type AppInfo,
  type HistoryEntry,
  type PlayEvent,
  type PlayStage,
  type RedialEvent,
  type ServerDetails,
  type ServerRow,
  type Settings,
} from "./api";
import { serverTitle } from "./format";
import { i18n } from "./i18n.svelte";
import { applyTheme } from "./themes";

export type SortKey = "players" | "name";
export type ListFilter = "all" | "favorites" | "recent";

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

const REFRESH_MS = 60_000;

function isFull(s: ServerRow): boolean {
  return s.soft_max_players > 0 && s.players >= s.soft_max_players;
}

/// A server that is not in the hub list (typed in, or named by a redial).
function adhocRow(address: string): ServerRow {
  return {
    address,
    name: null,
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

class Store {
  info = $state<AppInfo | null>(null);
  settings = $state<Settings | null>(null);

  servers = $state<ServerRow[]>([]);
  serversLoading = $state(false);
  serversError = $state<string | null>(null);
  /** The list on screen is the saved one: the hub has not answered yet (or at all). */
  serversFromCache = $state(false);

  accounts = $state<AccountRow[]>([]);
  /** User id of the account to play with; `null` = guest. */
  selectedAccount = $state<string | null>(null);

  search = $state("");
  sort = $state<SortKey>("players");
  hideEmpty = $state(false);
  hideFull = $state(false);
  filter = $state<ListFilter>("all");
  /** Language tag value (the part after `lang:`); empty = any. */
  language = $state("");

  selectedAddress = $state<string | null>(null);
  /** The selected server when it is not in the list. */
  adhoc = $state<ServerRow | null>(null);
  details = $state<Record<string, DetailsState>>({});

  play = $state<PlayState>({ phase: "idle" });
  /** Shown above the Play button: the game asked to reconnect. */
  redial = $state<{ reason: string | null } | null>(null);
  /** The selected server was opened from a link: the player has not agreed to connect yet. */
  fromLink = $state(false);

  readonly favorites = $derived(new Set(this.settings?.favorites ?? []));
  readonly history = $derived(new Map((this.settings?.history ?? []).map((h) => [h.address, h])));

  /** Languages the servers announce with `lang:xx` tags, most common first. */
  readonly languages = $derived.by(() => {
    const counts = new Map<string, number>();
    for (const s of this.servers) {
      for (const t of s.tags) {
        if (t.startsWith("lang:")) {
          const l = t.slice(5);
          counts.set(l, (counts.get(l) ?? 0) + 1);
        }
      }
    }
    return [...counts.entries()].sort((a, b) => b[1] - a[1]).map(([l]) => l);
  });

  readonly visible = $derived.by(() => {
    const q = this.search.trim().toLowerCase();
    let list = this.servers;
    if (this.filter === "favorites") {
      list = list.filter((s) => this.favorites.has(s.address));
    } else if (this.filter === "recent") {
      list = list.filter((s) => this.history.has(s.address));
    }
    if (this.language) {
      const tag = `lang:${this.language}`;
      list = list.filter((s) => s.tags.includes(tag));
    }
    if (q) {
      list = list.filter((s) =>
        `${s.name ?? ""} ${s.address} ${s.tags.join(" ")} ${s.map ?? ""} ${s.preset ?? ""}`
          .toLowerCase()
          .includes(q),
      );
    }
    if (this.hideEmpty) list = list.filter((s) => s.players > 0);
    if (this.hideFull) list = list.filter((s) => !isFull(s));

    if (this.filter === "recent") {
      const order = new Map((this.settings?.history ?? []).map((h, i) => [h.address, i]));
      return [...list].sort((a, b) => order.get(a.address)! - order.get(b.address)!);
    }
    return [...list].sort((a, b) =>
      this.sort === "players"
        ? b.players - a.players || serverTitle(a).localeCompare(serverTitle(b))
        : serverTitle(a).localeCompare(serverTitle(b), "ru"),
    );
  });

  readonly selected = $derived(
    this.servers.find((s) => s.address === this.selectedAddress) ??
      (this.adhoc?.address === this.selectedAddress ? this.adhoc : null),
  );

  readonly selectedDetails = $derived(
    this.selectedAddress ? (this.details[this.selectedAddress] ?? null) : null,
  );

  readonly busy = $derived(this.play.phase === "preparing" || this.play.phase === "running");

  async init() {
    // Listen first, so nothing the backend sends at startup (a redial) is missed.
    await api.onPlay((e) => this.onPlayEvent(e));
    await api.onRedial((e) => this.onRedial(e));
    await api.onOpenLink((address) => this.onOpenLink(address));

    await Promise.all([this.loadInfo(), this.loadSettings(), this.loadAccounts()]);
    this.pickDefaultAccount();

    // Something to look at while the hub answers.
    const cached = await api.cachedServers().catch(() => []);
    if (cached.length > 0) {
      this.servers = cached;
      this.serversFromCache = true;
    }
    void this.refreshServers();
    setInterval(() => {
      if (document.visibilityState === "visible" && !this.serversLoading) {
        void this.refreshServers();
      }
    }, REFRESH_MS);
  }

  private async loadInfo() {
    this.info = await api.appInfo().catch(() => null);
  }

  async loadSettings() {
    this.settings = await api.getSettings().catch(() => this.settings);
    this.applySettings();
  }

  private applySettings() {
    applyTheme(this.settings?.theme);
    i18n.preference = this.settings?.language ?? "auto";
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
  }

  async saveSettings(next: Settings) {
    this.settings = await api.setSettings(next);
    this.applySettings();
  }

  async toggleFavorite(address: string) {
    if (!this.settings) return;
    const favorites = await api.toggleFavorite(address).catch(() => null);
    if (favorites) this.settings = { ...this.settings, favorites };
  }

  historyOf(address: string): HistoryEntry | undefined {
    return this.history.get(address);
  }

  select(address: string | null) {
    this.selectedAddress = address;
    if (address) void this.loadDetails(address);
  }

  /** Moves the selection through the visible list (arrow keys). */
  moveSelection(delta: number) {
    const list = this.visible;
    if (list.length === 0) return;
    const i = list.findIndex((s) => s.address === this.selectedAddress);
    const next =
      i < 0 ? (delta > 0 ? 0 : list.length - 1) : Math.min(list.length - 1, Math.max(0, i + delta));
    this.select(list[next].address);
  }

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

  /** Selects a server that may not be in the list, and starts the game there. */
  async connectTo(address: string) {
    const typed = address.trim();
    if (!typed || this.busy) return;
    if (!this.servers.some((s) => s.address === typed)) this.adhoc = adhocRow(typed);
    this.select(typed);
    await this.startPlay();
  }

  async startPlay() {
    const server = this.selected;
    if (!server || this.busy) return;
    this.redial = null;
    this.fromLink = false;
    this.play = { phase: "preparing", stage: "account", done: 0, total: 0, unit: "items" };
    try {
      await api.play(server.address, server.name, this.selectedAccount);
      // The backend put the server on top of the history; take it from there.
      await this.loadSettings();
    } catch (e) {
      this.play = { phase: "failed", code: "other", message: errorText(e) };
    }
  }

  cancelPlay() {
    void api.cancelPlay();
  }

  dismissPlayResult() {
    if (!this.busy) this.play = { phase: "idle" };
  }

  /** A clicked ss14:// link: show the server and wait for the player to press Play. */
  private onOpenLink(address: string) {
    if (this.busy) return;
    const typed = address.trim();
    if (!this.servers.some((s) => s.address === typed)) this.adhoc = adhocRow(typed);
    this.select(typed);
    this.redial = null;
    this.fromLink = true;
  }

  private onRedial(e: RedialEvent) {
    this.fromLink = false;
    if (!this.servers.some((s) => s.address === e.address)) this.adhoc = adhocRow(e.address);
    this.select(e.address);
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
        break;
    }
  }
}

export const app = new Store();
