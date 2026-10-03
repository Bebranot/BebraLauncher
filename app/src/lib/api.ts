// Typed wrappers over the app's Tauri commands (app/src-tauri/src/commands.rs).
import { invoke } from "@tauri-apps/api/core";
import { listen, type UnlistenFn } from "@tauri-apps/api/event";

export interface AppInfo {
  version: string;
  data_dir: string;
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

export interface Settings {
  guest_name: string;
  compat_mode: boolean;
  last_account: string | null;
}

export type PlayStage = "account" | "info" | "files" | "engine" | "starting";

export type PlayEvent =
  | { kind: "stage"; stage: PlayStage }
  | { kind: "progress"; stage: PlayStage; done: number; total: number; unit: "items" | "bytes" }
  | { kind: "running"; pid: number; plays_as: string }
  | { kind: "exited"; code: number | null; log_tail: string[] }
  | { kind: "failed"; message: string }
  | { kind: "cancelled" };

export const api = {
  appInfo: () => invoke<AppInfo>("app_info"),
  listServers: () => invoke<ServerRow[]>("list_servers"),
  listAccounts: () => invoke<AccountRow[]>("list_accounts"),
  login: (args: {
    username?: string;
    userId?: string;
    password: string;
    tfaCode?: string;
  }) => invoke<LoginOutcome>("login", args),
  logout: (userId: string) => invoke<boolean>("logout", { userId }),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<Settings>("set_settings", { settings }),
  play: (address: string, account: string | null) =>
    invoke<void>("play", { address, account }),
  cancelPlay: () => invoke<void>("cancel_play"),
  openLogsFolder: () => invoke<void>("open_logs_folder"),
  onPlay: (handler: (e: PlayEvent) => void): Promise<UnlistenFn> =>
    listen<PlayEvent>("play", (e) => handler(e.payload)),
};

/** Errors from commands arrive as plain strings. */
export function errorText(e: unknown): string {
  return typeof e === "string" ? e : e instanceof Error ? e.message : String(e);
}
