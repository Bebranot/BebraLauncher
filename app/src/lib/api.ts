export interface RecommendedHub {
  url: string;
  name: string;
}
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

export interface PingRow {
  address: string;
  /** Milliseconds for a status request; null if the server did not answer. */
  ms: number | null;
  /** What the server answered (for servers the hub does not list). */
  status: ServerRow | null;
}

export interface NewsItem {
  title: string;
  link: string;
  date: string;
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
  /** User id of the account used here last time; empty = guest. */
  account: string | null;
}

export interface CustomTheme {
  background: string;
  accent: string;
  foreground: string;
  popup: string;
  gradient_start: string;
  gradient_end: string;
}

export interface Columns {
  round_time: boolean;
  players: boolean;
  map: boolean;
  mode: boolean;
  ping: boolean;
}

export interface HubSetting {
  url: string;
  enabled: boolean;
}

export interface Settings {
  /** Settings file format; written back unchanged so an older UI cannot undo a migration. */
  schema_version: number;
  guest_name: string;
  compat_mode: boolean;
  discord_rpc: boolean;
  minimize_on_play: boolean;
  theme: string;
  theme_gradient: boolean;
  theme_decor: boolean;
  custom_theme: CustomTheme;
  font: string;
  columns: Columns;
  enabled_packs: string[];
  ignore_redial: boolean;
  verbose_log: boolean;
  proxy: string;
  language: string;
  update_repo: string;
  update_check: boolean;
  update_auto: boolean;
  last_account: string | null;
  favorites: string[];
  history: HistoryEntry[];
  hubs: HubSetting[];
  extra_args: string;
  enabled_patches: string[];
  opt_profile: string;
  opt_fast_lighting: boolean;
  opt_unlock_fps: boolean;
  opt_high_priority: boolean;
  opt_server_gc: boolean;
  bg_type: "space" | "custom" | "none" | string;
  bg_custom_name: string | null;
  bg_dim: number;
  bg_blur: number;
  client_window_mode: string;
  client_mute_unfocused: boolean;
  client_ui_scale: string;
  client_max_fps: number;
  close_on_play: boolean;
  auto_refresh_interval: number;
  confirm_connect: boolean;
  server_notes?: Record<string, string>;
  favorite_names?: Record<string, string>;
  launcher_ui_scale?: number;
  auto_clean_cache?: boolean;
  dismissed_intel_warning?: boolean;
}

/** The HWID controls from the options tab. */
export interface HwidState {
  /** A new HWID is generated before every launch. */
  auto: boolean;
  /** The value the next launch will report, or null for the computer's own. */
  current: string | null;
  /** The value before the last change, for rolling back. */
  previous: string | null;
  /** This build ships the client patch that applies the value. */
  patch: boolean;
}

export interface BackgroundRow {
  id: string;
  name: string;
  file_size: number;
}

export interface PatchRow {
  id: string;
  name: string;
  file_size: number;
  enabled: boolean;
  /** False when the file changed after installation: the launcher will not load it. */
  verified: boolean;
}

export interface PackRow {
  id: string;
  name: string;
  description: string;
  files: number;
  bytes: number;
  cultures: string[];
  problems: string[];
  enabled: boolean;
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

export interface DumpRow {
  files: number;
  bytes: number;
  version_id: number;
  dir: string;
}

export interface VersionRow {
  id: number;
  fork: string;
  version: string;
  engine: string;
  files: number;
  last_used: string;
}

export interface DiagRow {
  name: string;
  ok: boolean;
  ms: number | null;
  detail: string;
}

export type UpdatePhase = "idle" | "checking" | "downloading" | "ready";

/** A launcher release found on GitHub. `notes` is the changelog section of that version. */
export interface UpdateRelease {
  version: string;
  notes: string;
  published: string | null;
  page_url: string;
  size: number;
}

export interface UpdateStatus {
  phase: UpdatePhase;
  /** This launcher, as `a.b.cdd`. */
  current: string;
  latest: UpdateRelease | null;
  done: number;
  total: number;
  error: string | null;
  /** Installing is possible right now (downloaded, a release build, no game running). */
  can_install: boolean;
}

export type PlayStage = "account" | "info" | "files" | "engine" | "starting";

export interface RunningClient {
  pid: number;
  playsAs: string;
  address?: string;
}

export type PlayEvent =
  | { kind: "stage"; stage: PlayStage }
  | {
      kind: "progress";
      stage: PlayStage;
      done: number;
      total: number;
      /** Payload received so far; `done` may count files instead of bytes. */
      bytes: number;
      unit: "items" | "bytes";
    }
  | { kind: "running"; pid: number; plays_as: string; address?: string }
  | { kind: "exited"; pid: number; code: number | null; log_tail: string[] }
  | { kind: "failed"; code: string; message: string }
  | { kind: "cancelled" }
  | { kind: "preview"; command: string };

/** An event that happened before the page could listen, kept by the backend (P1-18). */
export interface PendingAction {
  event: "open-link" | "redial" | "play";
  payload: unknown;
}

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
  pingServers: (addresses: string[]) => invoke<PingRow[]>("ping_servers", { addresses }),
  serverInfo: (address: string) => invoke<ServerDetails>("server_info", { address }),
  news: () => invoke<NewsItem[]>("news"),
  listAccounts: () => invoke<AccountRow[]>("list_accounts"),
  login: (args: { username?: string; userId?: string; password: string; tfaCode?: string }) =>
    invoke<LoginOutcome>("login", args),
  logout: (userId: string) => invoke<boolean>("logout", { userId }),
  selectAccount: (account: string | null) => invoke<void>("select_account", { account }),
  getSettings: () => invoke<Settings>("get_settings"),
  setSettings: (settings: Settings) => invoke<Settings>("set_settings", { settings }),
  getHwid: () => invoke<HwidState>("get_hwid"),
  setHwidAuto: (enabled: boolean) => invoke<HwidState>("set_hwid_auto", { enabled }),
  regenerateHwid: () => invoke<HwidState>("regenerate_hwid"),
  restoreHwid: () => invoke<HwidState>("restore_hwid"),
  toggleFavorite: (address: string) => invoke<string[]>("toggle_favorite", { address }),
  addFavorite: (address: string) => invoke<string[]>("add_favorite", { address }),
  raiseFavorite: (address: string) => invoke<string[]>("raise_favorite", { address }),
  moveFavorite: (address: string, delta: number) => invoke<string[]>("move_favorite", { address, delta }),
  editFavorite: (old_address: string, new_address: string, name?: string | null) =>
    invoke<string[]>("edit_favorite", {
      oldAddress: old_address,
      newAddress: new_address,
      name: name ?? null,
    }),
  setFavoriteName: (address: string, name?: string | null) =>
    invoke<Record<string, string>>("set_favorite_name", { address, name: name ?? null }),
  checkIntelWarning: () => invoke<boolean>("check_intel_warning"),
  dismissIntelWarning: () => invoke<void>("dismiss_intel_warning"),
  getRecommendedHubs: () => invoke<RecommendedHub[]>("get_recommended_hubs"),
  setServerNote: (address: string, note: string) =>
    invoke<Record<string, string>>("set_server_note", { address, note }),
  play: (address: string, name: string | null, account: string | null) =>
    invoke<void>("play", { address, name, account }),
  launchPreview: (address: string, account: string | null) =>
    invoke<void>("launch_preview", { address, account }),
  dumpLocale: () => invoke<DumpRow>("dump_locale"),
  diagnostics: () => invoke<DiagRow[]>("diagnostics"),
  contentVersions: () => invoke<VersionRow[]>("content_versions"),
  resetSettings: () => invoke<Settings>("reset_settings"),
  openDataFolder: () => invoke<void>("open_data_folder"),
  cancelPlay: () => invoke<void>("cancel_play"),
  openLogsFolder: () => invoke<void>("open_logs_folder"),
  openPacksFolder: () => invoke<void>("open_packs_folder"),
  listPacks: () => invoke<PackRow[]>("list_packs"),
  openPatchesFolder: () => invoke<void>("open_patches_folder"),
  listPatches: () => invoke<PatchRow[]>("list_patches"),
  deletePatch: (id: string) => invoke<void>("delete_patch", { id }),
  installPatch: (name: string, dataBase64: string) =>
    invoke<PatchRow>("install_patch", { name, dataBase64 }),
  openBackgroundsFolder: () => invoke<void>("open_backgrounds_folder"),
  listBackgrounds: () => invoke<BackgroundRow[]>("list_backgrounds"),
  saveBackground: (name: string, dataBase64: string) =>
    invoke<BackgroundRow>("save_background", { name, dataBase64 }),
  getBackground: (name: string) => invoke<string>("get_background", { name }),
  deleteBackground: (name: string) => invoke<void>("delete_background", { name }),
  openUrl: (url: string) => invoke<void>("open_url", { url }),
  getAutostart: () => invoke<boolean>("get_autostart"),
  setAutostart: (enabled: boolean) => invoke<boolean>("set_autostart", { enabled }),
  getLinksRegistered: () => invoke<boolean>("get_links_registered"),
  setLinksRegistered: (enabled: boolean) => invoke<boolean>("set_links_registered", { enabled }),
  storageInfo: () => invoke<StorageRow>("storage_info"),
  runningClients: () => invoke<RunningClient[]>("list_running_clients"),
  cleanStorage: () => invoke<CleanRow>("clean_storage"),
  cleanStorageAll: () => invoke<CleanRow>("clean_storage_all"),
  updateStatus: () => invoke<UpdateStatus>("update_status"),
  checkUpdate: () => invoke<UpdateStatus>("check_update"),
  downloadUpdate: () => invoke<UpdateStatus>("download_update"),
  installUpdate: (restart: boolean) => invoke<void>("install_update", { restart }),
  updateTokenState: () => invoke<boolean>("update_token_state"),
  setUpdateToken: (token: string) => invoke<boolean>("set_update_token", { token }),
  changelog: () => invoke<string>("changelog"),
  onUpdate: (handler: (s: UpdateStatus) => void): Promise<UnlistenFn> =>
    listen<UpdateStatus>("update", (e) => handler(e.payload)),
  /** What happened before the UI could listen (a cold-start link, redial, the first play events). */
  takePendingActions: () => invoke<PendingAction[]>("take_pending_actions"),
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
  if (typeof e === "string") return e;
  if (e instanceof Error) return e.message;
  if (e && typeof e === "object") {
    const message = (e as { message?: unknown }).message;
    if (typeof message === "string" && message) return message;
    try {
      const json = JSON.stringify(e);
      if (json && json !== "{}") return json;
    } catch {
      // A value that cannot be serialised falls back to the line below.
    }
  }
  return String(e);
}
