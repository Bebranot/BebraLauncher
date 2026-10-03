// Typed wrappers over the app's Tauri commands (app/src-tauri/src/commands.rs).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface AppInfo {
  version: string;
  data_dir: string;
  /** `string` when the game loader cannot be found, so playing is impossible. */
  loader_problem: string | null;
}

export interface ServerRow {
  address: string;
  name: string | null;
  players: number;
  soft_max_players: number;
  tags: string[];
  map: string | null;
  preset: string | null;
  run_level: number | null;
  round_start_time: string | null;
  panic_bunker: boolean | null;
}

export interface ServerDetails {
  /** Written by the server's owners: untrusted text, show as plain text. */
  description: string | null;
  links: { name: string; url: string }[];
  auth: "optional" | "required" | "disabled";
}

export type AccountState = "valid" | "renew" | "expired";

export interface AccountRow {
  user_id: string;
  username: string;
  expire_time: string;
  state: AccountState;
}

export type DenyCode =
  | "invalid_credentials"
  | "account_unconfirmed"
  | "tfa_required"
  | "tfa_invalid"
  | "account_locked"
  | "unknown";

export type LoginOutcome =
  | { status: "ok"; account: AccountRow }
  | { status: "denied"; code: DenyCode; errors: string[] };

export interface HistoryEntry {
  address: string;
  name: string | null;
  last_played: string;
  play_seconds: number;
}

export interface Settings {
  guest_name: string;
  compat_mode: boolean;
  minimize_on_play: boolean;
  theme: string;
  language: string;
  last_account: string | null;
  favorites: string[];
  history: HistoryEntry[];
}

export interface StorageRow {
  content: number;
  engines: number;
  modules: number;
  other: number;
  total: number;
}

export interface CleanRow {
  freed: number;
  versions_removed: number;
  engines_removed: number;
  modules_removed: number;
}

export type PlayStage = "account" | "info" | "files" | "engine" | "starting";

export type PlayEvent =
  | { kind: "stage"; stage: PlayStage }
  | { kind: "progress"; stage: PlayStage; done: number; total: number; unit: "items" | "bytes" }
  | { kind: "running"; pid: number; plays_as: string }
  | { kind: "exited"; code: number | null; log_tail: string[] }
  | { kind: "failed"; code: string; message: string }
  | { kind: "cancelled" };

/** The game asked the launcher to connect again (possibly to another server). */
export interface RedialEvent {
  address: string;
  /** Text from the server: untrusted. */
  reason: string | null;
}

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  listServers: () => invoke<ServerRow[]>("list_servers"),
  cachedServers: () => invoke<ServerRow[]>("cached_servers"),
  serverInfo: (address: string) => invoke<ServerDetails>("server_info", { address }),
  listAccounts: () => invoke<AccountRow[]>("list_accounts"),
  login: (args: { username?: string; userId?: string; password: string; tfaCode?: string }) =>
    invoke<LoginOutcome>("login", args),
  logout: (userId: string) => invoke<boolean>("logout", { userId }),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<Settings>("set_settings", { settings }),
  toggleFavorite: (address: string) => invoke<string[]>("toggle_favorite", { address }),
  play: (address: string, name: string | null, account: string | null) =>
    invoke<void>("play", { address, name, account }),
  cancelPlay: () => invoke<void>("cancel_play"),
  openLogsFolder: () => invoke<void>("open_logs_folder"),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  getAutostart: () => invoke<boolean>("get_autostart"),
  setAutostart: (enabled: boolean) => invoke<boolean>("set_autostart", { enabled }),
  getLinksRegistered: () => invoke<boolean>("get_links_registered"),
  setLinksRegistered: (enabled: boolean) => invoke<boolean>("set_links_registered", { enabled }),
  storageInfo: () => invoke<StorageRow>("storage_info"),
  cleanStorage: () => invoke<CleanRow>("clean_storage"),
  onPlay: (handler: (e: PlayEvent) => void): Promise<UnlistenFn> =>
    listen<PlayEvent>("play", (e) => handler(e.payload)),
  onRedial: (handler: (e: RedialEvent) => void): Promise<UnlistenFn> =>
    listen<RedialEvent>("redial", (e) => handler(e.payload)),
  /** An `ss14://` link was opened; the payload is the address. */
  onOpenLink: (handler: (address: string) => void): Promise<UnlistenFn> =>
    listen<string>("open-link", (e) => handler(e.payload)),
};

/** Errors from commands arrive as plain strings. */
export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}
