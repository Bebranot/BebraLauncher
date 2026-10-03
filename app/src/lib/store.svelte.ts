// App-wide state. One instance, shared by the components.
import {
  api,
  errorText,
  type AccountRow,
  type AppInfo,
  type PlayEvent,
  type PlayStage,
  type ServerRow,
  type Settings,
} from "./api";
import { serverTitle } from "./format";

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
  | { phase: "failed"; message: string }
  | { phase: "exited"; code: number | null; logTail: string[] };

const REFRESH_MS = 60_000;

class Store {
  info = $state<AppInfo | null>(null);
  settings = $state<Settings | null>(null);

  servers = $state<ServerRow[]>([]);
  serversLoading = $state(false);
  serversError = $state<string | null>(null);
  /** When the list was last loaded (ms), for "updated N s ago". */
  serversLoadedAt = $state<number | null>(null);

  accounts = $state<AccountRow[]>([]);
  /** User id of the account to play with; `null` = guest. */
  selectedAccount = $state<string | null>(null);

  search = $state("");
  sort = $state<SortKey>("players");
  hideEmpty = $state(false);
  filter = $state<ListFilter>("all");
  /** Language tag value (the part after `lang:`); empty = any. */
  language = $state("");

  selectedAddress = $state<string | null>(null);
  play = $state<PlayState>({ phase: "idle" });

  readonly favorites = $derived(new Set(this.settings?.favorites ?? []));

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
      const order = new Map((this.settings?.history ?? []).map((h, i) => [h.address, i]));
      return list
        .filter((s) => order.has(s.address))
        .sort((a, b) => order.get(a.address)! - order.get(b.address)!);
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
    return [...list].sort((a, b) =>
      this.sort === "players"
        ? b.players - a.players || serverTitle(a).localeCompare(serverTitle(b))
        : serverTitle(a).localeCompare(serverTitle(b), "ru"),
    );
  });

  readonly selected = $derived(
    this.servers.find((s) => s.address === this.selectedAddress) ?? null,
  );

  readonly busy = $derived(this.play.phase === "preparing" || this.play.phase === "running");

  async init() {
    await Promise.all([this.loadInfo(), this.loadSettings(), this.loadAccounts()]);
    this.pickDefaultAccount();
    void this.refreshServers();
    setInterval(() => {
      if (document.visibilityState === "visible" && !this.serversLoading) {
        void this.refreshServers();
      }
    }, REFRESH_MS);
    await api.onPlay((e) => this.onPlayEvent(e));
  }

  private async loadInfo() {
    this.info = await api.appInfo().catch(() => null);
  }

  private async loadSettings() {
    this.settings = await api.getSettings().catch(() => null);
  }

  async loadAccounts() {
    this.accounts = await api.listAccounts().catch(() => []);
    if (this.selectedAccount && !this.accounts.some((a) => a.user_id === this.selectedAccount)) {
      this.selectedAccount = null;
    }
  }

  private pickDefaultAccount() {
    const last = this.settings?.last_account;
    if (last && this.accounts.some((a) => a.user_id === last)) {
      this.selectedAccount = last;
    } else if (this.accounts.length === 1) {
      this.selectedAccount = this.accounts[0].user_id;
    }
  }

  async refreshServers() {
    this.serversLoading = true;
    try {
      this.servers = await api.listServers();
      this.serversError = null;
      this.serversLoadedAt = Date.now();
    } catch (e) {
      // Keep the old list on screen; just say the refresh failed.
      this.serversError = errorText(e);
    } finally {
      this.serversLoading = false;
    }
  }

  async saveSettings(next: Settings) {
    this.settings = await api.setSettings(next);
  }

  async toggleFavorite(address: string) {
    if (!this.settings) return;
    const favorites = await api.toggleFavorite(address).catch(() => null);
    if (favorites) this.settings = { ...this.settings, favorites };
  }

  async startPlay() {
    const server = this.selected;
    if (!server || this.busy) return;
    this.play = { phase: "preparing", stage: "account", done: 0, total: 0, unit: "items" };
    try {
      await api.play(server.address, server.name, this.selectedAccount);
      // The backend put the server on top of the history; take it from there.
      this.settings = await api.getSettings().catch(() => this.settings);
    } catch (e) {
      this.play = { phase: "failed", message: errorText(e) };
    }
  }

  cancelPlay() {
    void api.cancelPlay();
  }

  dismissPlayResult() {
    if (!this.busy) this.play = { phase: "idle" };
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
        break;
      case "exited":
        // A clean exit needs no message.
        this.play =
          e.code === 0
            ? { phase: "idle" }
            : { phase: "exited", code: e.code, logTail: e.log_tail };
        break;
      case "failed":
        this.play = { phase: "failed", message: e.message };
        break;
      case "cancelled":
        this.play = { phase: "idle" };
        break;
    }
  }
}

export const app = new Store();
