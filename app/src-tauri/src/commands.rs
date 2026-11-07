//! Commands the UI can call. Everything the UI learns about accounts goes through here, and
//! tokens never do: the UI sees names, ids and expiry, nothing it could use to log in.

use bebra_core::accounts::{
    self, CredentialStore, Freshness, LoginResult, StoredAccount, TokenStore,
};
use bebra_core::auth::{AuthClient, Identity};
use bebra_core::content_db::ContentDb;
use bebra_core::dump;
use bebra_core::hub::HubServerEntry;
use bebra_core::launch;
use bebra_core::packs;
use bebra_core::server::{self, AuthMode};
use bebra_core::storage;
use bebra_core::ServerAddr;
use serde::{Deserialize, Serialize};
use std::io::Write;
use tauri::{AppHandle, Manager, State};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::game::{self, PlayParams};
use crate::news::{self, NewsItem, OwnNewsItem};
use crate::redial::{self, Action};
use crate::selfupdate;
use crate::settings::{self, Settings};
use crate::AppState;

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

/// The settings lock, surviving a panic that poisoned it (P2-38).
fn settings_lock(state: &AppState) -> std::sync::MutexGuard<'_, Settings> {
    crate::lock_or_recover(&state.settings)
}

/// A server address in the one spelling favourites, notes and history use (P2-37).
fn server_address(address: &str) -> CmdResult<String> {
    settings::canonical_address(address).ok_or_else(|| "Неверный адрес сервера".to_string())
}

#[derive(Serialize)]
pub struct AppInfo {
    version: &'static str,
    data_dir: String,
    /// `Some(message)` when the game loader cannot be found, so playing is impossible.
    loader_problem: Option<String>,
}

/// The switches every preset stands for on this machine, so the options tab can show them and
/// start "custom" from one of them.
#[tauri::command]
pub fn optimization_presets(
) -> std::collections::BTreeMap<&'static str, bebra_core::launch::OptimizationConfig> {
    let cores = crate::settings::cpu_cores();
    ["standard", "balanced", "optimized"]
        .into_iter()
        .filter_map(|name| {
            bebra_core::launch::OptimizationConfig::preset(name, cores).map(|cfg| (name, cfg))
        })
        .collect()
}

/// What the setup wizard needs to suggest an optimization profile.
#[derive(Serialize)]
pub struct HardwareInfo {
    cores: usize,
    /// `None` when Windows would not say.
    ram_mb: Option<u64>,
    suggested_profile: &'static str,
}

#[tauri::command]
pub fn hardware_info() -> HardwareInfo {
    let cores = launch::logical_cores();
    let ram_mb = launch::total_ram_mb();
    HardwareInfo {
        cores,
        ram_mb,
        suggested_profile: launch::suggested_profile(cores, ram_mb),
    }
}

/// What the official launcher left on this PC, for the wizard to offer.
#[derive(Serialize)]
pub struct OfficialProbe {
    /// False when there is no official launcher data (or it could not be read).
    found: bool,
    favorites: usize,
    hubs: usize,
    compat_mode: Option<bool>,
}

#[tauri::command]
pub fn official_probe() -> OfficialProbe {
    let data =
        bebra_core::official::default_path().and_then(|path| {
            match bebra_core::official::read(&path) {
                Ok(data) => data,
                Err(e) => {
                    crate::applog::line(&format!("official launcher data not readable: {e}"));
                    None
                }
            }
        });
    match data {
        Some(d) => OfficialProbe {
            found: true,
            favorites: d.favorites.len(),
            hubs: d.hubs.len(),
            compat_mode: d.compat_mode,
        },
        None => OfficialProbe {
            found: false,
            favorites: 0,
            hubs: 0,
            compat_mode: None,
        },
    }
}

#[derive(Serialize)]
pub struct OfficialImportResult {
    summary: settings::OfficialImport,
    settings: Settings,
}

/// Takes over the official launcher's favourites, hubs and compatibility mode. Logins are not
/// touched: a token is not moved from one program to another behind the player's back.
#[tauri::command]
pub fn import_official(state: State<'_, AppState>) -> CmdResult<OfficialImportResult> {
    let path = bebra_core::official::default_path()
        .ok_or_else(|| "Не удалось найти папку данных официального лаунчера".to_string())?;
    let data = bebra_core::official::read(&path)
        .map_err(err)?
        .ok_or_else(|| "Данных официального лаунчера на этом компьютере нет".to_string())?;
    let mut s = settings_lock(&state);
    let summary = s.apply_official(&data);
    s.save(state.dirs.root()).map_err(err)?;
    Ok(OfficialImportResult {
        summary,
        settings: s.clone(),
    })
}

/// The player went through the wizard to the end: it is not shown again until a newer one.
#[tauri::command]
pub fn finish_setup(state: State<'_, AppState>) -> CmdResult<Settings> {
    let mut s = settings_lock(&state);
    s.setup_version = settings::CURRENT_SETUP_VERSION;
    s.save(state.dirs.root()).map_err(err)?;
    Ok(s.clone())
}

#[tauri::command]
pub fn app_info(state: State<'_, AppState>) -> AppInfo {
    AppInfo {
        version: env!("CARGO_PKG_VERSION"),
        data_dir: state.dirs.root().display().to_string(),
        loader_problem: launch::find_loader().err().map(err),
    }
}

/// One row of the server list, flattened for the UI.
#[derive(Serialize, Deserialize)]
pub struct ServerRow {
    address: String,
    name: Option<String>,
    players: i32,
    soft_max_players: i32,
    tags: Vec<String>,
    map: Option<String>,
    preset: Option<String>,
    run_level: Option<i32>,
    round_start_time: Option<String>,
    panic_bunker: Option<bool>,
}

impl From<HubServerEntry> for ServerRow {
    fn from(e: HubServerEntry) -> Self {
        Self::from_status(e.address, e.status, e.inferred_tags)
    }
}

impl ServerRow {
    /// A row from a server's `/status`; `inferred_tags` are the extra tags the hub worked out.
    fn from_status(address: String, s: server::ServerStatus, inferred_tags: Vec<String>) -> Self {
        // The server's own tags, plus the ones the hub worked out; no duplicates.
        let mut tags = s.tags;
        for t in inferred_tags {
            if !tags.contains(&t) {
                tags.push(t);
            }
        }
        Self {
            address,
            name: s.name,
            players: s.players,
            soft_max_players: s.soft_max_players,
            tags,
            map: s.map,
            preset: s.preset,
            run_level: s.run_level,
            round_start_time: s.round_start_time,
            panic_bunker: s.panic_bunker,
        }
    }
}

#[tauri::command]
pub async fn list_servers(state: State<'_, AppState>) -> CmdResult<Vec<ServerRow>> {
    let hubs: Vec<String> = {
        let s = settings_lock(&state);
        s.hubs
            .iter()
            .filter(|h| h.enabled)
            .map(|h| h.url.clone())
            .collect()
    };
    let hubs = if hubs.is_empty() {
        vec![bebra_core::hub::OFFICIAL_HUB_URL.to_string()]
    } else {
        hubs
    };
    let servers = bebra_core::hub::list_servers_from_hubs(&state.client, &hubs)
        .await
        .map_err(err)?;
    let rows: Vec<ServerRow> = servers.into_iter().map(ServerRow::from).collect();
    // So the next start has something to show before the hub answers.
    write_server_cache(&state, &rows);
    Ok(rows)
}

fn cache_path(state: &AppState) -> std::path::PathBuf {
    state.dirs.root().join("servers-cache.json")
}

/// Writes bytes next to the target, flushes them, then renames: a half-written file is never
/// seen by the next start (P2-36).
fn store_cache_file(path: &std::path::Path, json: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("json.tmp");
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(json)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

/// Writes the list in one go: a temporary file under the lock, then a rename (P2-36).
fn write_server_cache(state: &AppState, rows: &[ServerRow]) {
    let json = match serde_json::to_vec(rows) {
        Ok(json) => json,
        Err(e) => {
            crate::applog::line(&format!("servers cache: cannot serialize: {e}"));
            return;
        }
    };
    let _guard = crate::lock_or_recover(&state.servers_cache);
    if let Err(e) = store_cache_file(&cache_path(state), &json) {
        crate::applog::line(&format!("servers cache: cannot save: {e}"));
    }
}

/// The server list of the last successful refresh (maybe long ago); empty if there is none.
#[tauri::command]
pub fn cached_servers(state: State<'_, AppState>) -> Vec<ServerRow> {
    read_server_cache(&state)
}

/// The cached list, logging anything that is wrong with the file (P2-36).
pub(crate) fn read_server_cache(state: &AppState) -> Vec<ServerRow> {
    let path = cache_path(state);
    let _guard = crate::lock_or_recover(&state.servers_cache);
    let bytes = match std::fs::read(&path) {
        Ok(bytes) => bytes,
        Err(e) if e.kind() == std::io::ErrorKind::NotFound => return Vec::new(),
        Err(e) => {
            crate::applog::line(&format!("servers cache: cannot read: {e}"));
            return Vec::new();
        }
    };
    match parse_cached_rows(&bytes) {
        Ok(rows) => rows,
        Err(e) => {
            crate::applog::line(&format!("servers cache: ignoring a damaged file: {e}"));
            Vec::new()
        }
    }
}

fn parse_cached_rows(bytes: &[u8]) -> serde_json::Result<Vec<ServerRow>> {
    serde_json::from_slice(bytes)
}

#[derive(Serialize)]
pub struct InfoLinkRow {
    name: String,
    url: String,
}

#[derive(Serialize)]
pub struct ServerDetails {
    /// Description written by the server's owners: untrusted text.
    description: Option<String>,
    links: Vec<InfoLinkRow>,
    /// `optional`, `required` or `disabled`.
    auth: &'static str,
}

/// What the server says about itself (`/info`). Shown next to the Play button.
#[tauri::command]
pub async fn server_info(state: State<'_, AppState>, address: String) -> CmdResult<ServerDetails> {
    let addr = ServerAddr::parse(&address).map_err(err)?;
    let info = server::fetch_info_preview(&state.client, &addr)
        .await
        .map_err(err)?;
    Ok(ServerDetails {
        description: info.desc.filter(|d| !d.trim().is_empty()),
        links: info
            .links
            .into_iter()
            .filter(|l| crate::system::is_safe_web_url(&l.url))
            .map(|l| InfoLinkRow {
                name: l.name,
                url: l.url,
            })
            .collect(),
        auth: match info.auth.mode {
            AuthMode::Optional => "optional",
            AuthMode::Required => "required",
            AuthMode::Disabled => "disabled",
            // A mode this launcher does not know yet behaves like the permissive default.
            AuthMode::Unknown => "optional",
        },
    })
}

#[tauri::command]
pub fn open_url(url: String) -> CmdResult<()> {
    crate::system::open_url(&url)
}

#[tauri::command]
pub fn get_autostart() -> bool {
    crate::system::autostart_enabled()
}

#[tauri::command]
pub fn set_autostart(enabled: bool) -> CmdResult<bool> {
    crate::system::set_autostart(enabled)?;
    Ok(crate::system::autostart_enabled())
}

/// Is this launcher the one that opens `ss14://` links?
#[tauri::command]
pub fn get_links_registered() -> bool {
    crate::system::links_registered()
}

#[tauri::command]
pub fn set_links_registered(enabled: bool) -> CmdResult<bool> {
    crate::system::set_links_registered(enabled)?;
    Ok(crate::system::links_registered())
}

#[derive(Serialize)]
pub struct StorageRow {
    content: u64,
    engines: u64,
    modules: u64,
    other: u64,
    total: u64,
}

#[tauri::command]
pub async fn storage_info(state: State<'_, AppState>) -> CmdResult<StorageRow> {
    let dirs = state.dirs.clone();
    tokio::task::spawn_blocking(move || {
        let r = storage::report(&dirs);
        Ok(StorageRow {
            content: r.content_db,
            engines: r.engines,
            modules: r.modules,
            other: r.scratch + r.logs,
            total: r.total(),
        })
    })
    .await
    .map_err(err)?
}

#[derive(Serialize)]
pub struct CleanRow {
    freed: u64,
    versions_removed: usize,
    engines_removed: usize,
    modules_removed: usize,
}

/// Removes old game versions and engines nobody needs any more. Refused while a game is
/// being prepared or running.
#[tauri::command]
pub async fn clean_storage(state: State<'_, AppState>) -> CmdResult<CleanRow> {
    if state.game.is_active() {
        return Err("Очистка недоступна, пока идёт запуск или игра".into());
    }
    let dirs = state.dirs.clone();
    tokio::task::spawn_blocking(move || {
        let db = ContentDb::open(&dirs.content_db()).map_err(err)?;
        let r = storage::clean(&dirs, &db).map_err(err)?;
        Ok(CleanRow {
            freed: r.bytes_freed,
            versions_removed: r.cull.versions_removed,
            engines_removed: r.engines_removed,
            modules_removed: r.modules_removed,
        })
    })
    .await
    .map_err(err)?
}

/// Removes all game versions, engines and modules. Refused while a game is
/// being prepared or running.
#[tauri::command]
pub async fn clean_storage_all(state: State<'_, AppState>) -> CmdResult<CleanRow> {
    if state.game.is_active() {
        return Err("Очистка недоступна, пока идёт запуск или игра".into());
    }
    let dirs = state.dirs.clone();
    tokio::task::spawn_blocking(move || {
        let db = ContentDb::open(&dirs.content_db()).map_err(err)?;
        let r = storage::clean_all(&dirs, &db).map_err(err)?;
        Ok(CleanRow {
            freed: r.bytes_freed,
            versions_removed: r.cull.versions_removed,
            engines_removed: r.engines_removed,
            modules_removed: r.modules_removed,
        })
    })
    .await
    .map_err(err)?
}

#[derive(Serialize)]
pub struct AccountRow {
    user_id: String,
    username: String,
    /// RFC 3339.
    expire_time: String,
    /// `valid`, `renew` (valid, will be renewed when used) or `expired` (log in again).
    state: &'static str,
}

impl AccountRow {
    fn new(a: &StoredAccount, now: OffsetDateTime) -> Self {
        Self {
            user_id: a.user_id.to_string(),
            username: a.username.clone(),
            expire_time: a.expire_time.format(&Rfc3339).unwrap_or_default(),
            state: match accounts::freshness(a.expire_time, now) {
                Freshness::Fine => "valid",
                Freshness::NeedsRefresh => "renew",
                Freshness::Expired => "expired",
            },
        }
    }
}

#[tauri::command]
pub async fn list_accounts() -> CmdResult<Vec<AccountRow>> {
    tokio::task::spawn_blocking(|| {
        let now = OffsetDateTime::now_utc();
        let mut rows: Vec<AccountRow> = CredentialStore::default()
            .list()
            .map_err(err)?
            .iter()
            .map(|a| AccountRow::new(a, now))
            .collect();
        rows.sort_by_key(|a| a.username.to_lowercase());
        Ok(rows)
    })
    .await
    .map_err(err)?
}

#[derive(Serialize)]
#[serde(tag = "status", rename_all = "snake_case")]
pub enum LoginOutcome {
    Ok {
        account: AccountRow,
    },
    /// The server said no. `code` is `invalid_credentials`, `account_unconfirmed`,
    /// `tfa_required`, `tfa_invalid`, `account_locked` or `unknown`.
    Denied {
        code: &'static str,
        errors: Vec<String>,
    },
}

/// Logs in with a name (new account) or a user id (a saved account that has to log in again).
/// The password is only passed on to the account server; it is not stored or logged.
#[tauri::command]
pub async fn login(
    state: State<'_, AppState>,
    username: Option<String>,
    user_id: Option<String>,
    password: String,
    tfa_code: Option<String>,
) -> CmdResult<LoginOutcome> {
    // The same lock the game uses to renew a token: a login must not race a renewal (P2-35).
    let _tokens = state.game.lock_tokens().await;
    let auth = AuthClient::official(state.client.clone());
    let store = CredentialStore::default();
    let tfa = tfa_code.as_deref().map(str::trim).filter(|c| !c.is_empty());

    let id;
    let name;
    let who = match (&user_id, &username) {
        (Some(i), _) => {
            id = i.parse::<Uuid>().map_err(|_| "Неверный id аккаунта")?;
            Identity::UserId(id)
        }
        (None, Some(n)) if !n.trim().is_empty() => {
            name = n.trim().to_string();
            Identity::Username(&name)
        }
        _ => return Err("Введите имя аккаунта".into()),
    };

    match accounts::login(&auth, &store, who, &password, tfa)
        .await
        .map_err(err)?
    {
        LoginResult::Success(account) => {
            crate::applog::line(&format!("login ok: {}", account.username));
            Ok(LoginOutcome::Ok {
                account: AccountRow::new(&account, OffsetDateTime::now_utc()),
            })
        }
        LoginResult::Denied { errors, code } => {
            crate::applog::line(&format!("login denied: {code:?}"));
            use bebra_core::auth::DenyCode::*;
            Ok(LoginOutcome::Denied {
                code: match code {
                    InvalidCredentials => "invalid_credentials",
                    AccountUnconfirmed => "account_unconfirmed",
                    TfaRequired => "tfa_required",
                    TfaInvalid => "tfa_invalid",
                    AccountLocked => "account_locked",
                    None | Unknown => "unknown",
                },
                errors,
            })
        }
    }
}

/// Logs out and forgets the account. Returns whether the account server was told.
#[tauri::command]
pub async fn logout(state: State<'_, AppState>, user_id: String) -> CmdResult<bool> {
    let id: Uuid = user_id.parse().map_err(|_| "Неверный id аккаунта")?;
    // A concurrent token renewal would write the login back after the logout (P2-35).
    let _tokens = state.game.lock_tokens().await;
    let store = CredentialStore::default();
    let accounts_list = store.list().map_err(err)?;
    let account = accounts_list
        .iter()
        .find(|a| a.user_id == id)
        .ok_or("Аккаунт не найден")?;
    let told = accounts::logout(&AuthClient::official(state.client.clone()), &store, account)
        .await
        .map_err(err)?;

    let mut settings = settings_lock(&state);
    if settings.last_account.as_deref() == Some(user_id.as_str()) {
        settings.last_account = None;
        settings.save(state.dirs.root()).map_err(err)?;
    }
    Ok(told)
}

#[tauri::command]
pub async fn select_account(state: State<'_, AppState>, account: Option<String>) -> CmdResult<()> {
    let acc_str = account.unwrap_or_default();
    if !acc_str.is_empty() {
        acc_str
            .parse::<Uuid>()
            .map_err(|_| "Неверный id аккаунта")?;
    }
    let _tokens = state.game.lock_tokens().await;
    let mut settings = settings_lock(&state);
    settings.last_account = Some(acc_str);
    settings.save(state.dirs.root()).map_err(err)?;
    Ok(())
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    let settings = settings_lock(&state).clone();
    // Remember what the UI was told, to tell a stale copy from an edit in `set_settings`.
    *crate::lock_or_recover(&state.settings_seen) = settings.clone();
    settings
}

#[tauri::command]
pub fn set_settings(state: State<'_, AppState>, settings: Settings) -> CmdResult<Settings> {
    let mut current = settings_lock(&state);
    let seen = crate::lock_or_recover(&state.settings_seen).clone();
    // The UI edits its own options, but sends the whole settings object back. What only the
    // launcher writes — favourites and their names, notes, history, the last account, enabled
    // patches, the dismissed Intel warning, the chosen background, the schema version — must
    // not be undone by its older copy (P1-20).
    let merged = current.merged_from_ui(&seen, settings).sanitized();
    merged.save(state.dirs.root()).map_err(err)?;
    *current = merged.clone();
    *crate::lock_or_recover(&state.settings_seen) = merged.clone();
    Ok(merged)
}

/// Everything that happened before the UI could listen: a clicked `ss14://` link, a redial
/// asked for by the running game, the first `play` events of a cold start.
///
/// The UI calls this once, after subscribing to the events, and replays every item through the
/// same handler as the live event: `{ event: "open-link" | "redial" | "play", payload: … }`,
/// oldest first. After the call events are emitted live and nothing is repeated.
#[tauri::command]
pub fn take_pending_actions(state: State<'_, AppState>) -> Vec<crate::pending::PendingAction> {
    state.pending.take()
}

/// Adds or removes a favourite server. Returns the new list of favourites.
#[tauri::command]
pub fn toggle_favorite(state: State<'_, AppState>, address: String) -> CmdResult<Vec<String>> {
    let address = server_address(&address)?;
    let mut s = settings_lock(&state);
    s.toggle_favorite(&address);
    s.save(state.dirs.root()).map_err(err)?;
    Ok(s.favorites.clone())
}

/// Adds a favourite server by address (one the hub may not list). Returns the new list.
#[tauri::command]
pub fn add_favorite(state: State<'_, AppState>, address: String) -> CmdResult<Vec<String>> {
    let address = server_address(&address)?;
    let mut s = settings_lock(&state);
    s.add_favorite(&address);
    s.save(state.dirs.root()).map_err(err)?;
    Ok(s.favorites.clone())
}

/// Moves a favourite server to the top of the list. Returns the new list of favourites.
#[tauri::command]
pub fn raise_favorite(state: State<'_, AppState>, address: String) -> CmdResult<Vec<String>> {
    let address = server_address(&address)?;
    let mut s = settings_lock(&state);
    s.raise_favorite(&address);
    s.save(state.dirs.root()).map_err(err)?;
    Ok(s.favorites.clone())
}

/// Moves a favourite server up (delta = -1) or down (delta = 1).
#[tauri::command]
pub fn move_favorite(
    state: State<'_, AppState>,
    address: String,
    delta: i32,
) -> CmdResult<Vec<String>> {
    let address = server_address(&address)?;
    let mut s = settings_lock(&state);
    s.move_favorite(&address, delta);
    s.save(state.dirs.root()).map_err(err)?;
    Ok(s.favorites.clone())
}

/// Changes the address and/or custom name of an existing favourite.
#[tauri::command]
pub fn edit_favorite(
    state: State<'_, AppState>,
    old_address: String,
    new_address: String,
    name: Option<String>,
) -> CmdResult<Vec<String>> {
    let old_address = server_address(&old_address)?;
    let new_address = server_address(&new_address)?;
    let mut s = settings_lock(&state);
    s.edit_favorite(&old_address, &new_address, name);
    s.save(state.dirs.root()).map_err(err)?;
    Ok(s.favorites.clone())
}

/// Sets or removes a custom alias for a favourite server.
#[tauri::command]
pub fn set_favorite_name(
    state: State<'_, AppState>,
    address: String,
    name: Option<String>,
) -> CmdResult<std::collections::HashMap<String, String>> {
    let address = server_address(&address)?;
    let mut s = settings_lock(&state);
    s.set_favorite_name(&address, name);
    s.save(state.dirs.root()).map_err(err)?;
    Ok(s.favorite_names.clone())
}

/// Returns true if a desktop 13th/14th Gen Intel CPU is detected and the warning was not yet dismissed.
#[tauri::command]
pub fn check_intel_warning(state: State<'_, AppState>) -> bool {
    let s = settings_lock(&state);
    if s.dismissed_intel_warning {
        return false;
    }
    crate::system::is_vulnerable_intel_cpu()
}

/// Dismisses the Intel 13th/14th Gen CPU degradation advisory.
#[tauri::command]
pub fn dismiss_intel_warning(state: State<'_, AppState>) -> CmdResult<()> {
    let mut s = settings_lock(&state);
    s.dismissed_intel_warning = true;
    s.save(state.dirs.root()).map_err(err)?;
    Ok(())
}

/// Returns the built-in recommended hub servers.
#[tauri::command]
pub fn get_recommended_hubs() -> Vec<crate::settings::RecommendedHub> {
    crate::settings::recommended_hubs()
}

/// Sets or clears a personal note for a server address.
#[tauri::command]
pub fn set_server_note(
    state: State<'_, AppState>,
    address: String,
    note: String,
) -> CmdResult<std::collections::HashMap<String, String>> {
    let address = server_address(&address)?;
    let mut s = settings_lock(&state);
    s.set_server_note(&address, &note);
    s.save(state.dirs.root()).map_err(err)?;
    Ok(s.server_notes.clone())
}

/// Starts preparing and running the game. Progress and the outcome arrive as `play` events.
/// `account` is a user id; without it the game starts as a guest.
#[tauri::command]
pub fn play(
    app: AppHandle,
    state: State<'_, AppState>,
    address: String,
    name: Option<String>,
    account: Option<String>,
) -> CmdResult<()> {
    let account = account
        .map(|a| a.parse::<Uuid>().map_err(|_| "Неверный id аккаунта"))
        .transpose()?;
    start_play(&app, &state, &address, name, account, false)
}

/// Debug: prepares everything for a server and reports the command that would start the game,
/// without starting it. Secrets in the command are hidden. The result arrives as a
/// `play` event of kind `preview`.
#[tauri::command]
pub fn launch_preview(
    app: AppHandle,
    state: State<'_, AppState>,
    address: String,
    account: Option<String>,
) -> CmdResult<()> {
    let account = account
        .filter(|a| !a.is_empty())
        .map(|a| a.parse::<Uuid>().map_err(|_| "Неверный id аккаунта"))
        .transpose()?;
    start_play(&app, &state, &address, None, account, true)
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RunningClientDto {
    pub pid: u32,
    #[serde(rename = "playsAs")]
    pub plays_as: String,
    pub address: String,
}

/// Lists games that are currently running.
#[tauri::command]
pub fn list_running_clients(state: State<'_, AppState>) -> Vec<RunningClientDto> {
    let sessions = state.sessions.load(state.dirs.root());
    sessions
        .into_iter()
        .filter(|s| bebra_core::process::is_process_running_from(s.pid, &s.image_path))
        .map(|s| RunningClientDto {
            pid: s.pid,
            plays_as: s.plays_as,
            address: s.address,
        })
        .collect()
}

/// Starts the game. The server and the account are remembered once the game is running.
pub fn start_play(
    app: &AppHandle,
    state: &AppState,
    address: &str,
    name: Option<String>,
    account: Option<Uuid>,
    preview: bool,
) -> CmdResult<()> {
    let addr = ServerAddr::parse(address).map_err(err)?;
    let mut settings = settings_lock(state).clone();

    // A new HWID before every launch, or the one the player set by hand. The value the previous
    // launch used is kept so a bad ID can be rolled back.
    if settings.hwid_auto {
        let fresh = bebra_core::hwid::HwidOverride::random();
        let previous = settings.hwid_current.replace(fresh.legacy.clone());
        settings.hwid_previous = previous;
        let mut saved = settings_lock(state);
        saved.hwid_current = settings.hwid_current.clone();
        saved.hwid_previous = settings.hwid_previous.clone();
        if let Err(e) = saved.save(state.dirs.root()) {
            crate::applog::line(&format!("cannot save the generated HWID ({e})"));
        }
    }

    let resolved = bebra_core::patches::resolve_verified(&state.dirs, &settings.enabled_patches);
    if !resolved.rejected.is_empty() {
        // Something replaced an installed patch: never load it without saying so.
        crate::applog::line(&format!(
            "patches not loaded, the file changed after installation: {}",
            resolved.rejected.join(", ")
        ));
    }
    let hwid = settings
        .hwid_current
        .as_deref()
        .and_then(bebra_core::hwid::HwidOverride::from_legacy_hex);
    let (patches, optimization) = {
        let mut paths = resolved.paths;
        if hwid.is_some() {
            match bebra_core::launch::bundled_hwid_patch() {
                Some(path) if !paths.contains(&path) => paths.push(path),
                Some(_) => {}
                None => crate::applog::line(
                    "HWID is set, but Bebra.HwidPatch.dll is missing next to the Loader; \
                     the client will report the machine's own ID",
                ),
            }
        }
        let optimization = settings.optimization_config();
        if optimization.needs_perf_patch() {
            match bebra_core::launch::bundled_perf_patch() {
                Some(path) if !paths.contains(&path) => paths.push(path),
                Some(_) => {}
                None => crate::applog::line(
                    "client patches are on, but Bebra.PerfPatch.dll is missing next to the                      Loader; the game runs without them",
                ),
            }
        }
        if game::launcher_keeps_presence(&settings.discord_rpc_mode, settings.close_on_play) {
            match bebra_core::launch::bundled_presence_patch() {
                Some(path) if !paths.contains(&path) => paths.push(path),
                Some(_) => {}
                None => crate::applog::line(
                    "Bebra.PresencePatch.dll is missing next to the Loader; \
                     the Discord status has no character name or server image",
                ),
            }
        }
        (paths, optimization)
    };

    game::start(
        app.clone(),
        state.game.clone(),
        PlayParams {
            client: state.client.clone(),
            dirs: state.dirs.clone(),
            addr,
            account,
            guest_name: settings.guest_name,
            compat_mode: settings.compat_mode,
            discord_rpc_mode: settings.discord_rpc_mode,
            language: crate::presence::resolve_language(&settings.language),
            unload_on_play: settings.unload_on_play,
            minimize_on_play: settings.minimize_on_play,
            close_on_play: settings.close_on_play,
            client_window_mode: settings.client_window_mode,
            client_mute_unfocused: settings.client_mute_unfocused,
            client_ui_scale: settings.client_ui_scale,
            client_max_fps: settings.client_max_fps,
            enabled_packs: settings.enabled_packs,
            extra_args: settings.extra_args,
            patches,
            hwid,
            optimization,
            preview,
            verbose: settings.verbose_log,
            history_key: address.to_string(),
            server_name: name,
        },
    )
}

/// What the UI is told when the game asks for a reconnect.
#[derive(Serialize, Clone)]
struct RedialEvent {
    address: String,
    /// Text from the server: untrusted, show it as plain text only.
    reason: Option<String>,
}

/// How long a reconnect request waits for the old game to finish closing.
const REDIAL_WAIT: std::time::Duration = std::time::Duration::from_secs(20);

/// Handles the command line of a launcher process: our own start, a second start (forwarded by
/// the single-instance plugin) or the game asking for a reconnect (`--commands …`).
pub fn handle_launcher_args(app: &AppHandle, args: &[String]) {
    let actions = redial::actions(args);
    if actions.is_empty() {
        game::restore_window(app);
        return;
    }
    for action in actions {
        match action {
            Action::Activate => game::restore_window(app),
            Action::Open { address } => {
                // A clicked link is untrusted text: keep a possible `user:pass@` out of the log.
                crate::applog::line(&format!("link opened: {}", crate::redact::text(&address)));
                game::restore_window(app);
                // The page may not be listening yet (cold start): it asks for it after that.
                crate::pending::emit(app, "open-link", address);
            }
            Action::Connect { address, reason } => {
                let state = app.state::<AppState>();
                if settings_lock(&state).ignore_redial {
                    crate::applog::line(&format!(
                        "redial to {} ignored (setting)",
                        crate::redact::text(&address)
                    ));
                    continue;
                }
                let app = app.clone();
                // The old game may still be closing: wait for it on a thread of its own.
                let spawned = std::thread::Builder::new()
                    .name("redial".into())
                    .spawn(move || {
                        redial_connect(&app, address, reason);
                    });
                let _ = spawned;
            }
        }
    }
}

fn redial_connect(app: &AppHandle, address: String, reason: Option<String>) {
    let state = app.state::<AppState>();
    if let Err(e) = ServerAddr::parse(&address) {
        crate::pending::emit(
            app,
            game::EVENT,
            game::PlayEvent::failed("invalid_address", e.to_string()),
        );
        return;
    }
    // Same account as the last game; no account means a guest.
    let account = state
        .settings
        .lock()
        .unwrap()
        .last_account
        .as_deref()
        .and_then(|a| a.parse::<Uuid>().ok());

    crate::applog::line(&format!("redial to {}", crate::redact::text(&address)));
    game::restore_window(app);
    crate::pending::emit(
        app,
        "redial",
        RedialEvent {
            address: address.clone(),
            reason,
        },
    );

    let deadline = std::time::Instant::now() + REDIAL_WAIT;
    loop {
        match start_play(app, &state, &address, None, account, false) {
            Err(e) if e == game::BUSY && std::time::Instant::now() < deadline => {
                std::thread::sleep(std::time::Duration::from_millis(500));
            }
            Err(e) => {
                crate::pending::emit(app, game::EVENT, game::PlayEvent::failed("other", e));
                return;
            }
            Ok(()) => return,
        }
    }
}

#[tauri::command]
pub fn cancel_play(state: State<'_, AppState>) {
    state.game.cancel();
}

fn open_folder(dir: std::path::PathBuf) -> CmdResult<()> {
    std::fs::create_dir_all(&dir).map_err(err)?;
    // explorer.exe exits with 1 even when it works, so the exit code means nothing.
    std::process::Command::new("explorer")
        .arg(dir)
        .spawn()
        .map(drop)
        .map_err(err)
}

#[tauri::command]
pub fn open_logs_folder(state: State<'_, AppState>) -> CmdResult<()> {
    open_folder(state.dirs.logs())
}

#[tauri::command]
pub fn open_packs_folder(state: State<'_, AppState>) -> CmdResult<()> {
    open_folder(state.dirs.packs())
}

#[tauri::command]
pub fn open_patches_folder(state: State<'_, AppState>) -> CmdResult<()> {
    open_folder(state.dirs.patches())
}

#[tauri::command]
pub fn open_data_folder(state: State<'_, AppState>) -> CmdResult<()> {
    open_folder(state.dirs.root().to_path_buf())
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct BackgroundRow {
    pub id: String,
    pub name: String,
    pub file_size: u64,
}

const SUPPORTED_BG_EXTENSIONS: &[&str] = &["png", "jpg", "jpeg", "webp", "gif"];

/// Largest background the UI may send. The base64 text is measured before it is decoded, so an
/// oversized request costs nothing (P2-31).
const MAX_BG_BYTES: usize = 30 * 1024 * 1024;
/// Largest client patch the UI may send.
const MAX_PATCH_BYTES: usize = 100 * 1024 * 1024;

/// Names Windows keeps for devices: a file `CON.png` cannot be created or opened (P2-32).
const RESERVED_NAMES: &[&str] = &[
    "CON", "PRN", "AUX", "NUL", "COM1", "COM2", "COM3", "COM4", "COM5", "COM6", "COM7", "COM8",
    "COM9", "LPT1", "LPT2", "LPT3", "LPT4", "LPT5", "LPT6", "LPT7", "LPT8", "LPT9",
];

fn is_supported_bg_extension(ext: &str) -> bool {
    let lower = ext.to_ascii_lowercase();
    SUPPORTED_BG_EXTENSIONS.iter().any(|&e| e == lower)
}

fn is_reserved_windows_name(stem: &str) -> bool {
    let upper = stem.trim().to_ascii_uppercase();
    RESERVED_NAMES.iter().any(|r| upper == *r)
}

/// Drops everything a file name cannot hold, keeping letters, digits, spaces, `-`, `_` and `.`.
fn clean_file_name(name: &str) -> String {
    name.chars()
        .filter(|c| c.is_alphanumeric() || matches!(*c, '-' | '_' | '.' | ' '))
        .collect::<String>()
        .trim()
        .to_string()
}

/// The one check used for saving, reading and deleting a background (P2-32).
///
/// Returns the name to use on disk. Rejects path separators, `..`, reserved device names and
/// names Windows would silently change (a trailing dot or space).
fn validate_bg_filename(name: &str) -> CmdResult<String> {
    // Windows drops a trailing space or dot silently; such a name would be saved under another
    // name than the one asked for.
    if name != name.trim() || name.ends_with('.') {
        return Err("Недопустимое имя файла".into());
    }
    let clean = name;
    if clean.is_empty() {
        return Err("Имя файла не может быть пустым".into());
    }
    if clean.len() > 200
        || clean.contains(['/', '\\', ':'])
        || clean.contains("..")
        || clean.ends_with(' ')
    {
        return Err("Недопустимое имя файла".into());
    }
    let p = std::path::Path::new(clean);
    if p.file_name() != Some(std::ffi::OsStr::new(clean)) {
        return Err("Недопустимое имя файла".into());
    }
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if stem.is_empty() || is_reserved_windows_name(stem) {
        return Err("Недопустимое имя файла".into());
    }
    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
    if !is_supported_bg_extension(ext) {
        return Err("Недопустимый тип файла изображения".into());
    }
    Ok(clean.to_string())
}

/// The name a saved background gets: cleaned, with `.png` added when the name has no known
/// image extension, then validated like every other background name.
fn background_file_name(name: &str) -> CmdResult<String> {
    let clean = clean_file_name(name);
    if clean.is_empty() {
        return Err("Имя файла не может быть пустым".into());
    }
    let ext = std::path::Path::new(&clean)
        .extension()
        .and_then(|s| s.to_str());
    let candidate = match ext {
        Some(e) if is_supported_bg_extension(e) => clean,
        _ => format!("{clean}.png"),
    };
    validate_bg_filename(&candidate)
}

/// The part of a data URL after `;base64,` (or the whole text).
fn base64_payload(data: &str) -> &str {
    match data.find(";base64,") {
        Some(i) => data[i + 8..].trim(),
        None => data.trim(),
    }
}

/// The longest base64 text that can hold `max_bytes` bytes, with room for the data-URL header.
fn base64_limit(max_bytes: usize) -> usize {
    // 3 bytes per 4 characters.
    (max_bytes / 3 + 1) * 4 + 4096
}

fn decode_base64(payload: &str, max_bytes: usize, what: &str) -> CmdResult<Vec<u8>> {
    if payload.len() > base64_limit(max_bytes) {
        return Err(format!(
            "Файл слишком большой (максимум {} МБ)",
            max_bytes / (1024 * 1024)
        ));
    }
    use base64::Engine;
    let bytes = base64::prelude::BASE64_STANDARD
        .decode(payload)
        .map_err(|e| format!("Не удалось декодировать {what}: {e}"))?;
    if bytes.is_empty() {
        return Err("Файл пуст".into());
    }
    if bytes.len() > max_bytes {
        return Err(format!(
            "Файл слишком большой (максимум {} МБ)",
            max_bytes / (1024 * 1024)
        ));
    }
    Ok(bytes)
}

/// Writes bytes next to their final place, flushes them, then renames: a crash leaves either
/// the old file or the complete new one.
fn write_file_atomic(path: &std::path::Path, bytes: &[u8]) -> std::io::Result<()> {
    let tmp = path.with_extension("part");
    {
        let mut file = std::fs::File::create(&tmp)?;
        file.write_all(bytes)?;
        file.sync_all()?;
    }
    std::fs::rename(&tmp, path)
}

#[tauri::command]
pub fn list_backgrounds(state: State<'_, AppState>) -> CmdResult<Vec<BackgroundRow>> {
    let dir = state.dirs.backgrounds();
    if !dir.is_dir() {
        return Ok(Vec::new());
    }
    let mut rows = Vec::new();
    let entries = std::fs::read_dir(&dir).map_err(err)?;
    for entry in entries.flatten() {
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                if is_supported_bg_extension(ext) {
                    let file_size = entry.metadata().map(|m| m.len()).unwrap_or(0);
                    let id = entry.file_name().to_string_lossy().to_string();
                    let name = path
                        .file_stem()
                        .map(|s| s.to_string_lossy().to_string())
                        .unwrap_or_else(|| id.clone());
                    rows.push(BackgroundRow {
                        id,
                        name,
                        file_size,
                    });
                }
            }
        }
    }
    rows.sort_by_key(|a| a.name.to_lowercase());
    Ok(rows)
}

#[tauri::command]
pub async fn save_background(
    state: State<'_, AppState>,
    name: String,
    data_base64: String,
) -> CmdResult<BackgroundRow> {
    let filename = background_file_name(&name)?;
    // Checked before the text is even copied or decoded (P2-31).
    if base64_payload(&data_base64).len() > base64_limit(MAX_BG_BYTES) {
        return Err(format!(
            "Файл слишком большой (максимум {} МБ)",
            MAX_BG_BYTES / (1024 * 1024)
        ));
    }
    let dir = state.dirs.backgrounds();
    // Decoding and writing 30 MB must not block the thread that serves IPC.
    tokio::task::spawn_blocking(move || -> CmdResult<BackgroundRow> {
        let bytes = decode_base64(base64_payload(&data_base64), MAX_BG_BYTES, "изображение")?;
        std::fs::create_dir_all(&dir).map_err(err)?;
        write_file_atomic(&dir.join(&filename), &bytes).map_err(err)?;

        let name_stem = std::path::Path::new(&filename)
            .file_stem()
            .map(|s| s.to_string_lossy().to_string())
            .unwrap_or_else(|| filename.clone());
        Ok(BackgroundRow {
            id: filename,
            name: name_stem,
            file_size: bytes.len() as u64,
        })
    })
    .await
    .map_err(err)?
}

#[tauri::command]
pub async fn get_background(state: State<'_, AppState>, name: String) -> CmdResult<String> {
    let name = validate_bg_filename(&name)?;
    let path = state.dirs.backgrounds().join(&name);
    tokio::task::spawn_blocking(move || -> CmdResult<String> {
        let bytes = std::fs::read(&path).map_err(err)?;
        let ext = path
            .extension()
            .and_then(|s| s.to_str())
            .unwrap_or("png")
            .to_ascii_lowercase();
        let mime = match ext.as_str() {
            "jpg" | "jpeg" => "image/jpeg",
            "webp" => "image/webp",
            "gif" => "image/gif",
            _ => "image/png",
        };
        use base64::Engine;
        let b64 = base64::prelude::BASE64_STANDARD.encode(&bytes);
        Ok(format!("data:{mime};base64,{b64}"))
    })
    .await
    .map_err(err)?
}

#[tauri::command]
pub fn delete_background(state: State<'_, AppState>, name: String) -> CmdResult<()> {
    let name = validate_bg_filename(&name)?;
    let path = state.dirs.backgrounds().join(&name);
    // A background that is already gone is fine; anything else is the user's problem to hear.
    if let Err(e) = std::fs::remove_file(&path) {
        if e.kind() != std::io::ErrorKind::NotFound {
            return Err(format!("Не удалось удалить файл: {e}"));
        }
    }

    let mut settings = settings_lock(&state);
    if settings.bg_custom_name.as_deref() == Some(name.as_str()) {
        settings.bg_custom_name = None;
        settings.save(state.dirs.root()).map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
pub fn open_backgrounds_folder(state: State<'_, AppState>) -> CmdResult<()> {
    let dir = state.dirs.backgrounds();
    let _ = std::fs::create_dir_all(&dir);
    open_folder(dir)
}

#[derive(Serialize)]
pub struct DumpRow {
    files: usize,
    bytes: u64,
    version_id: i64,
    dir: String,
}

/// Writes the texts of the newest installed game version to a folder, as a start for a
/// translation (a text pack). Then shows the folder.
#[tauri::command]
pub async fn dump_locale(state: State<'_, AppState>) -> CmdResult<DumpRow> {
    let dirs = state.dirs.clone();
    tokio::task::spawn_blocking(move || {
        let db = ContentDb::open(&dirs.content_db()).map_err(err)?;
        let out = dirs.root().join("dump");
        let r = dump::dump_locale(&db, &out).map_err(err)?;
        let _ = open_folder(r.dir.clone());
        Ok(DumpRow {
            files: r.files,
            bytes: r.bytes,
            version_id: r.version_id,
            dir: r.dir.display().to_string(),
        })
    })
    .await
    .map_err(err)?
}

#[derive(Serialize)]
pub struct VersionRow {
    id: i64,
    fork: String,
    version: String,
    engine: String,
    files: usize,
    last_used: String,
}

/// The game versions stored on this computer (debug view).
#[tauri::command]
pub async fn content_versions(state: State<'_, AppState>) -> CmdResult<Vec<VersionRow>> {
    let dirs = state.dirs.clone();
    tokio::task::spawn_blocking(move || {
        let db = ContentDb::open(&dirs.content_db()).map_err(err)?;
        let mut rows = Vec::new();
        for v in db.list_versions().map_err(err)? {
            let engine = db
                .engine_dependencies(v.id)
                .map_err(err)?
                .into_iter()
                .find(|(m, _)| m == "Robust")
                .map(|(_, ver)| ver)
                .unwrap_or_default();
            rows.push(VersionRow {
                id: v.id,
                fork: v.fork_id.unwrap_or_default(),
                version: v.fork_version.unwrap_or_default(),
                engine,
                files: db.count_manifest_entries(v.id).map_err(err)?,
                last_used: v.last_used,
            });
        }
        Ok(rows)
    })
    .await
    .map_err(err)?
}

/// The HWID part of the settings, as the options tab shows it.
#[derive(Serialize)]
pub struct HwidState {
    /// A new HWID is generated before every launch.
    pub auto: bool,
    /// The value the next launch will report (hex), or `None` for the machine's own.
    pub current: Option<String>,
    /// The value before the last change, for rolling back.
    pub previous: Option<String>,
    /// This build ships the client patch that applies the value.
    pub patch: bool,
}

fn hwid_state(s: &Settings) -> HwidState {
    HwidState {
        auto: s.hwid_auto,
        current: s.hwid_current.clone(),
        previous: s.hwid_previous.clone(),
        patch: bebra_core::launch::bundled_hwid_patch().is_some(),
    }
}

#[tauri::command]
pub fn get_hwid(state: State<'_, AppState>) -> HwidState {
    hwid_state(&settings_lock(&state))
}

/// Turns the automatic change on or off. The current value stays as it is.
#[tauri::command]
pub fn set_hwid_auto(state: State<'_, AppState>, enabled: bool) -> CmdResult<HwidState> {
    let mut settings = settings_lock(&state);
    settings.hwid_auto = enabled;
    settings.save(state.dirs.root()).map_err(err)?;
    Ok(hwid_state(&settings))
}

/// Generates a new HWID now. The old one is kept so it can be restored.
#[tauri::command]
pub fn regenerate_hwid(state: State<'_, AppState>) -> CmdResult<HwidState> {
    let mut settings = settings_lock(&state);
    let fresh = bebra_core::hwid::HwidOverride::random();
    settings.hwid_previous = settings.hwid_current.replace(fresh.legacy);
    settings.save(state.dirs.root()).map_err(err)?;
    Ok(hwid_state(&settings))
}

/// Puts back the HWID that was used before the last change.
#[tauri::command]
pub fn restore_hwid(state: State<'_, AppState>) -> CmdResult<HwidState> {
    let mut settings = settings_lock(&state);
    let Some(previous) = settings.hwid_previous.take() else {
        return Err("Предыдущего HWID нет".into());
    };
    settings.hwid_current = Some(previous);
    settings.save(state.dirs.root()).map_err(err)?;
    Ok(hwid_state(&settings))
}

/// Puts every option back to its default. Favourites, their custom names, server notes,
/// history, the chosen account and the saved logins stay (P1-21).
#[tauri::command]
pub fn reset_settings(state: State<'_, AppState>) -> CmdResult<Settings> {
    let mut current = settings_lock(&state);
    let fresh = current.reset_keeping_user_data();
    fresh.save(state.dirs.root()).map_err(err)?;
    *current = fresh.clone();
    *crate::lock_or_recover(&state.settings_seen) = fresh.clone();
    Ok(fresh)
}

#[derive(Serialize)]
pub struct DiagRow {
    name: &'static str,
    ok: bool,
    /// How long the check took, for network checks.
    ms: Option<u32>,
    detail: String,
}

/// Checks the things the launcher depends on, and says what is wrong (debug view).
#[tauri::command]
pub async fn diagnostics(app: AppHandle, state: State<'_, AppState>) -> CmdResult<Vec<DiagRow>> {
    let client = state.client.clone();
    // Any answer, even an error status, shows the server is reachable; only a failure to
    // get an answer at all is a problem.
    async fn reach(client: &bebra_core::http::Client, name: &'static str, url: &str) -> DiagRow {
        let started = std::time::Instant::now();
        let result = client
            .get(url)
            .timeout(std::time::Duration::from_secs(10))
            .send()
            .await;
        let ms = started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32;
        match result {
            Ok(resp) => DiagRow {
                name,
                ok: resp.status().as_u16() < 500,
                ms: Some(ms),
                detail: format!("HTTP {}", resp.status().as_u16()),
            },
            Err(e) => DiagRow {
                name,
                ok: false,
                ms: None,
                detail: e.to_string(),
            },
        }
    }

    let (hub, auth, builds) = tokio::join!(
        reach(&client, "Hub", "https://hub.spacestation14.com/api/servers"),
        reach(
            &client,
            "Auth",
            "https://auth.spacestation14.com/api/auth/ping"
        ),
        reach(
            &client,
            "Builds",
            "https://robust-builds.cdn.spacestation14.com/manifest.json"
        ),
    );

    let loader = match launch::find_loader() {
        Ok(path) => DiagRow {
            name: "Loader",
            ok: true,
            ms: None,
            detail: path.display().to_string(),
        },
        Err(e) => DiagRow {
            name: "Loader",
            ok: false,
            ms: None,
            detail: e.to_string(),
        },
    };

    // Can we write to the data folder?
    let probe = state.dirs.root().join(".write-test");
    let data = match std::fs::create_dir_all(state.dirs.root())
        .and_then(|()| std::fs::write(&probe, b"x"))
        .and_then(|()| std::fs::remove_file(&probe))
    {
        Ok(()) => DiagRow {
            name: "Data folder",
            ok: true,
            ms: None,
            detail: state.dirs.root().display().to_string(),
        },
        Err(e) => DiagRow {
            name: "Data folder",
            ok: false,
            ms: None,
            detail: e.to_string(),
        },
    };

    let accounts = match CredentialStore::default().list() {
        Ok(list) => DiagRow {
            name: "Saved logins",
            ok: true,
            ms: None,
            detail: format!("{}", list.len()),
        },
        Err(e) => DiagRow {
            name: "Saved logins",
            ok: false,
            ms: None,
            detail: e.to_string(),
        },
    };

    let webview = DiagRow {
        name: "WebView2",
        ok: tauri::webview_version().is_ok(),
        ms: None,
        detail: tauri::webview_version().unwrap_or_else(|e| e.to_string()),
    };
    let _ = app;

    Ok(vec![hub, auth, builds, loader, data, accounts, webview])
}

/// A text pack found in the packs folder.
#[derive(Serialize)]
pub struct PackRow {
    id: String,
    name: String,
    description: String,
    files: usize,
    bytes: u64,
    cultures: Vec<String>,
    /// Why the pack cannot be used; empty if it can.
    problems: Vec<String>,
    enabled: bool,
}

#[tauri::command]
pub async fn list_packs(state: State<'_, AppState>) -> CmdResult<Vec<PackRow>> {
    let dirs = state.dirs.clone();
    let enabled = settings_lock(&state).enabled_packs.clone();
    tokio::task::spawn_blocking(move || {
        Ok(packs::scan(&dirs)
            .into_iter()
            .map(|p| PackRow {
                enabled: enabled.contains(&p.id),
                id: p.id,
                name: p.name,
                description: p.description,
                files: p.files,
                bytes: p.bytes,
                cultures: p.cultures,
                problems: p.problems,
            })
            .collect())
    })
    .await
    .map_err(err)?
}

#[derive(Serialize)]
pub struct PatchRow {
    id: String,
    name: String,
    #[allow(dead_code)]
    file_size: u64,
    enabled: bool,
    /// False when the file no longer matches the hash recorded at installation (P1-2).
    verified: bool,
}

fn validate_patch_id(id: &str) -> CmdResult<String> {
    let clean = id.trim();
    if clean.is_empty()
        || clean.len() > 200
        || clean.contains(['/', '\\', ':'])
        || clean.contains("..")
        || clean.ends_with('.')
        || clean.ends_with(' ')
    {
        return Err("Недопустимое имя файла мода".into());
    }
    let p = std::path::Path::new(clean);
    if p.file_name() != Some(std::ffi::OsStr::new(clean)) {
        return Err("Недопустимое имя файла мода".into());
    }
    let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
    if stem.is_empty() || is_reserved_windows_name(stem) {
        return Err("Недопустимое имя файла мода".into());
    }
    if !p
        .extension()
        .and_then(|e| e.to_str())
        .map(|e| e.eq_ignore_ascii_case("dll"))
        .unwrap_or(false)
    {
        return Err("Мод должен иметь расширение .dll".into());
    }
    Ok(clean.to_string())
}

/// The file name a patch gets: cleaned, `.dll` added when the name has no extension, then
/// validated.
fn patch_file_name(name: &str) -> CmdResult<String> {
    let mut clean = clean_file_name(name);
    if clean.is_empty() {
        return Err("Имя файла не может быть пустым".into());
    }
    let has_extension = std::path::Path::new(&clean)
        .extension()
        .and_then(|e| e.to_str())
        .is_some();
    if !has_extension {
        clean.push_str(".dll");
    }
    validate_patch_id(&clean)
}

#[tauri::command]
pub fn delete_patch(state: State<'_, AppState>, id: String) -> CmdResult<()> {
    let clean = validate_patch_id(&id)?;
    let path = state.dirs.patches().join(&clean);
    if path.is_file() {
        std::fs::remove_file(&path).map_err(err)?;
    }
    bebra_core::patches::forget(&state.dirs, &clean).map_err(err)?;
    let mut settings = settings_lock(&state);
    if settings.enabled_patches.contains(&clean) {
        settings.enabled_patches.retain(|p| p != &clean);
        settings.save(state.dirs.root()).map_err(err)?;
    }
    Ok(())
}

#[tauri::command]
pub async fn install_patch(
    state: State<'_, AppState>,
    name: String,
    data_base64: String,
) -> CmdResult<PatchRow> {
    let filename = patch_file_name(&name)?;
    // Checked before decoding a 100 MB request (P2-31).
    if base64_payload(&data_base64).len() > base64_limit(MAX_PATCH_BYTES) {
        return Err(format!(
            "Файл слишком большой (максимум {} МБ)",
            MAX_PATCH_BYTES / (1024 * 1024)
        ));
    }
    let dirs = state.dirs.clone();
    let dir = dirs.patches();
    let row = tokio::task::spawn_blocking(move || -> CmdResult<PatchRow> {
        let bytes = decode_base64(base64_payload(&data_base64), MAX_PATCH_BYTES, "файл мода")?;
        std::fs::create_dir_all(&dir).map_err(err)?;
        write_file_atomic(&dir.join(&filename), &bytes).map_err(err)?;
        // Remember what was installed, so a changed file is not loaded later (P1-2).
        bebra_core::patches::record_installed(&dirs, &filename, &bytes).map_err(err)?;
        let patch_name = std::path::Path::new(&filename)
            .file_stem()
            .unwrap_or_default()
            .to_string_lossy()
            .to_string();
        Ok(PatchRow {
            id: filename,
            name: patch_name,
            file_size: bytes.len() as u64,
            enabled: true,
            verified: true,
        })
    })
    .await
    .map_err(err)??;

    let mut settings = settings_lock(&state);
    if !settings.enabled_patches.contains(&row.id) {
        settings.enabled_patches.push(row.id.clone());
        settings.save(state.dirs.root()).map_err(err)?;
    }
    Ok(row)
}

#[tauri::command]
pub async fn list_patches(state: State<'_, AppState>) -> CmdResult<Vec<PatchRow>> {
    let dirs = state.dirs.clone();
    let enabled = settings_lock(&state).enabled_patches.clone();
    tokio::task::spawn_blocking(move || {
        let patches = bebra_core::patches::scan_patches(&dirs).map_err(err)?;
        Ok(patches
            .into_iter()
            .map(|p| PatchRow {
                enabled: enabled.contains(&p.id),
                id: p.id,
                name: p.name,
                file_size: p.file_size,
                verified: p.verified,
            })
            .collect())
    })
    .await
    .map_err(err)?
}

/// How long the news feed is kept before it is read again.
const NEWS_TTL: std::time::Duration = std::time::Duration::from_secs(10 * 60);

/// Most of the feed that is read. The blog's feed is a few hundred kilobytes; a hostile or
/// broken one must not be able to fill memory (P2-41).
const NEWS_MAX_BYTES: usize = 2 * 1024 * 1024;
const NEWS_TIMEOUT: std::time::Duration = std::time::Duration::from_secs(10);

/// The feed kept from the last success, fresh or not.
fn cached_news(state: &AppState) -> Option<Vec<NewsItem>> {
    crate::lock_or_recover(&state.news)
        .as_ref()
        .map(|(_, items)| items.clone())
}

/// The feed kept from the last success, if it is still young enough.
fn fresh_news(state: &AppState) -> Option<Vec<NewsItem>> {
    match crate::lock_or_recover(&state.news).as_ref() {
        Some((at, items)) if at.elapsed() < NEWS_TTL => Some(items.clone()),
        _ => None,
    }
}

/// Reads at most [`NEWS_MAX_BYTES`] of the feed, giving up when the server stalls.
async fn fetch_news(state: &AppState) -> CmdResult<Vec<NewsItem>> {
    let resp = state
        .client
        .get(news::FEED_URL)
        .timeout(NEWS_TIMEOUT)
        .send()
        .await
        .map_err(err)?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let mut resp = resp;
    let mut body: Vec<u8> = Vec::new();
    loop {
        let chunk = tokio::time::timeout(NEWS_TIMEOUT, resp.chunk())
            .await
            .map_err(|_| "Сервер новостей не ответил вовремя".to_string())?
            .map_err(err)?;
        let Some(chunk) = chunk else {
            break;
        };
        if body.len() + chunk.len() > NEWS_MAX_BYTES {
            crate::applog::line("news: the feed is larger than 2 MiB, reading no more");
            break;
        }
        body.extend_from_slice(&chunk);
    }
    Ok(news::parse_feed(&String::from_utf8_lossy(&body)))
}

#[tauri::command]
pub async fn news(state: State<'_, AppState>) -> CmdResult<Vec<NewsItem>> {
    if let Some(items) = fresh_news(&state) {
        return Ok(items);
    }
    // One request at a time: two calls while the feed is stale would both go to the network.
    let _gate = state.news_gate.lock().await;
    if let Some(items) = fresh_news(&state) {
        return Ok(items);
    }
    match fetch_news(&state).await {
        Ok(items) => {
            *crate::lock_or_recover(&state.news) = Some((std::time::Instant::now(), items.clone()));
            Ok(items)
        }
        // An old feed is better than an empty tab.
        Err(e) => match cached_news(&state) {
            Some(items) => {
                crate::applog::line(&format!("news: cannot refresh, showing the old feed ({e})"));
                Ok(items)
            }
            None => Err(e),
        },
    }
}

/// The launcher's own news from its repository. An absent file, or a closed repository without a
/// token, means no news yet: not an error for the player.
async fn fetch_own_news(state: &AppState) -> CmdResult<Vec<OwnNewsItem>> {
    let token = crate::selfupdate::token();
    let body = bebra_core::self_update::read_repo_file(
        &state.client,
        bebra_core::self_update::DEFAULT_REPO,
        news::OWN_PATH,
        token.as_ref(),
        news::OWN_MAX_BYTES,
    )
    .await
    .map_err(err)?;
    match body {
        Some(body) => news::parse_own(&String::from_utf8_lossy(&body)),
        None => Ok(Vec::new()),
    }
}

#[tauri::command]
pub async fn own_news(state: State<'_, AppState>) -> CmdResult<Vec<OwnNewsItem>> {
    let fresh = |state: &AppState| match crate::lock_or_recover(&state.own_news).as_ref() {
        Some((at, items)) if at.elapsed() < NEWS_TTL => Some(items.clone()),
        _ => None,
    };
    if let Some(items) = fresh(&state) {
        return Ok(items);
    }
    let _gate = state.own_news_gate.lock().await;
    if let Some(items) = fresh(&state) {
        return Ok(items);
    }
    match fetch_own_news(&state).await {
        Ok(items) => {
            *crate::lock_or_recover(&state.own_news) =
                Some((std::time::Instant::now(), items.clone()));
            Ok(items)
        }
        Err(e) => {
            let old = crate::lock_or_recover(&state.own_news)
                .as_ref()
                .map(|(_, items)| items.clone());
            match old {
                Some(items) => {
                    crate::applog::line(&format!("own news: showing the old list ({e})"));
                    Ok(items)
                }
                None => Err(e),
            }
        }
    }
}

#[derive(Serialize)]
pub struct PingRow {
    address: String,
    /// Round trip of a status request in milliseconds; `None` if the server did not answer.
    ms: Option<u32>,
    /// What the server answered: a row for servers the hub does not list (favourites by address).
    status: Option<ServerRow>,
}

/// Most servers measured in one call, and most requests in flight at once.
const PING_MAX_SERVERS: usize = 200;
const PING_PARALLEL: usize = 12;
/// After this the call returns what answered so far; the UI paints the column at once.
const PING_DEADLINE: std::time::Duration = std::time::Duration::from_secs(15);

/// Is this a name or address on this machine or in a local network?
fn is_local_or_private_host(host: &str) -> bool {
    let h = host.trim().trim_start_matches('[').trim_end_matches(']');
    if h.is_empty() {
        return true;
    }
    if let Ok(ip) = h.parse::<std::net::IpAddr>() {
        return !is_public_ip(ip);
    }
    let lower = h.to_ascii_lowercase();
    // `localhost.` is the same name with the root label written out.
    let lower = lower.trim_end_matches('.');
    lower == "localhost"
        || lower.ends_with(".localhost")
        || lower.ends_with(".local")
        || lower.ends_with(".localdomain")
        || lower.ends_with(".internal")
        || lower.ends_with(".lan")
        || lower.ends_with(".home.arpa")
}

fn is_public_ip(ip: std::net::IpAddr) -> bool {
    match ip {
        std::net::IpAddr::V4(v4) => {
            let o = v4.octets();
            !(v4.is_unspecified()
                || v4.is_loopback()
                || v4.is_private()
                || v4.is_link_local()
                || v4.is_broadcast()
                || v4.is_documentation()
                || v4.is_multicast()
                // 100.64.0.0/10: carrier-grade NAT, never a public game server.
                || (o[0] == 100 && (64..128).contains(&o[1]))
                // 0.0.0.0/8: "this network", not routable.
                || o[0] == 0)
        }
        std::net::IpAddr::V6(v6) => {
            !(v6.is_unspecified()
                || v6.is_loopback()
                || v6.is_multicast()
                || v6.is_unique_local()
                || v6.is_unicast_link_local()
                || v6
                    .to_ipv4_mapped()
                    .is_some_and(|v4| !is_public_ip(std::net::IpAddr::V4(v4))))
        }
    }
}

/// The servers a ping may ask about: what the last hub answer listed, the favourites and the
/// history. Anything else is refused, so the command cannot be used to scan a network (P2-33).
fn ping_allowed(state: &AppState) -> std::collections::HashSet<String> {
    let mut allowed = std::collections::HashSet::new();
    for row in read_server_cache(state) {
        if let Some(a) = settings::canonical_address(&row.address) {
            allowed.insert(a);
        }
    }
    let s = settings_lock(state);
    for address in s
        .favorites
        .iter()
        .chain(s.history.iter().map(|h| &h.address))
    {
        if let Some(a) = settings::canonical_address(address) {
            allowed.insert(a);
        }
    }
    allowed
}

/// Asks each server for its `/status` and times the answer. This is what the list's ping
/// column shows, and how favourite servers that are not on the hub get their numbers.
#[tauri::command]
pub async fn ping_servers(
    state: State<'_, AppState>,
    addresses: Vec<String>,
) -> CmdResult<Vec<PingRow>> {
    let allowed = ping_allowed(&state);
    let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(PING_PARALLEL));
    let mut tasks = tokio::task::JoinSet::new();
    let mut seen = std::collections::HashSet::new();
    let mut skipped = 0usize;
    for address in addresses.into_iter().take(PING_MAX_SERVERS) {
        let Ok(addr) = ServerAddr::parse(&address) else {
            continue;
        };
        let Some(canonical) = settings::canonical_address(&address) else {
            continue;
        };
        if !allowed.contains(&canonical) || is_local_or_private_host(addr.host()) {
            skipped += 1;
            continue;
        }
        if !seen.insert(canonical) {
            continue;
        }
        let client = state.client.clone();
        let gate = gate.clone();
        tasks.spawn(async move {
            // The semaphore is never closed, but do not panic if it ever is.
            let Ok(_slot) = gate.acquire().await else {
                return None;
            };
            let started = std::time::Instant::now();
            let status = server::fetch_status(&client, &addr).await.ok();
            let ms = status
                .is_some()
                .then(|| started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32);
            let status = status.map(|s| ServerRow::from_status(address.clone(), s, Vec::new()));
            Some(PingRow {
                address,
                ms,
                status,
            })
        });
    }
    let deadline = tokio::time::Instant::now() + PING_DEADLINE;
    if skipped > 0 {
        // Details are not logged: a caller could otherwise spray address text into the log.
        crate::applog::line(&format!(
            "ping: {skipped} address(es) refused (unknown server or a local network)"
        ));
    }
    let mut rows = Vec::new();
    loop {
        match tokio::time::timeout_at(deadline, tasks.join_next()).await {
            Ok(Some(Ok(Some(row)))) => rows.push(row),
            Ok(Some(Ok(None) | Err(_))) => {}
            Ok(None) => break,
            Err(_) => {
                crate::applog::line("ping: the deadline passed, returning partial results");
                tasks.abort_all();
                break;
            }
        }
    }
    Ok(rows)
}

// ---------------------------------------------------------------- updating the launcher

/// Where the update stands: nothing found, found, downloading, ready to install.
#[tauri::command]
pub fn update_status(app: AppHandle) -> selfupdate::Status {
    selfupdate::status(&app)
}

/// Asks GitHub for a newer version (and downloads it at once when updates are automatic).
#[tauri::command]
pub async fn check_update(app: AppHandle) -> selfupdate::Status {
    selfupdate::check(&app).await
}

/// Downloads the version found by the last check; progress comes as `update` events.
#[tauri::command]
pub fn download_update(app: AppHandle) -> selfupdate::Status {
    selfupdate::download(app.clone());
    selfupdate::status(&app)
}

/// Installs the downloaded version and closes the launcher; with `restart` it opens again.
#[tauri::command]
pub fn install_update(app: AppHandle, restart: bool) -> CmdResult<()> {
    selfupdate::install_now(&app, restart)
}

/// Is a GitHub access token saved? The token itself never reaches the UI.
#[tauri::command]
pub fn update_token_state() -> bool {
    selfupdate::token_present()
}

/// Saves the access token for a private repository; an empty text removes it.
#[tauri::command]
pub fn set_update_token(token: String) -> CmdResult<bool> {
    selfupdate::set_token(&token)?;
    Ok(selfupdate::token_present())
}

/// The whole changelog, as written in CHANGELOG.md.
#[tauri::command]
pub fn changelog() -> &'static str {
    crate::changelog::TEXT
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn background_names_are_checked_before_use() {
        for ok in ["wall.png", "my photo.JPG", "a_b-c.webp", "image.v2.gif"] {
            assert!(validate_bg_filename(ok).is_ok(), "{ok} should be accepted");
        }
        for bad in [
            "",
            "  ",
            "a..b.png",
            "dir/wall.png",
            "C:\\wall.png",
            "wall.txt",
            "CON.png",
            "nul.jpg",
            "com1.gif",
            "wall.png ",
            "wall.png.",
            "lpt9.webp",
        ] {
            assert!(
                validate_bg_filename(bad).is_err(),
                "{bad:?} should be refused"
            );
        }
    }

    /// A name without a known image extension gets `.png`, and the result is validated like
    /// any other name (P2-32).
    #[test]
    fn saved_background_names_get_an_image_extension() {
        assert_eq!(background_file_name("wall").unwrap(), "wall.png");
        assert_eq!(background_file_name("wall.JPG").unwrap(), "wall.JPG");
        assert_eq!(background_file_name(" my wall ").unwrap(), "my wall.png");
        assert!(background_file_name("NUL").is_err());
        assert!(background_file_name("a..b").is_err());
        assert!(background_file_name("").is_err());
    }

    #[test]
    fn patch_names_must_be_plain_dll_names() {
        assert_eq!(patch_file_name("mod").unwrap(), "mod.dll");
        assert_eq!(patch_file_name("my.mod.dll").unwrap(), "my.mod.dll");
        assert!(patch_file_name("con").is_err());
        assert!(patch_file_name("../mod").is_err());
        assert!(patch_file_name("mod.exe").is_err());
    }

    #[test]
    fn a_data_url_payload_is_split_off() {
        assert_eq!(base64_payload("data:image/png;base64,AAAA"), "AAAA");
        assert_eq!(base64_payload("  AAAA  "), "AAAA");
        assert_eq!(base64_payload(""), "");
    }

    /// An oversized body is refused by its text length, before anything is decoded (P2-31).
    #[test]
    fn an_oversized_base64_text_is_refused() {
        let limit = base64_limit(MAX_BG_BYTES);
        assert!(limit > MAX_BG_BYTES);
        let too_long = "A".repeat(limit + 1);
        let error = decode_base64(&too_long, MAX_BG_BYTES, "изображение").unwrap_err();
        assert!(error.contains("слишком большой"));
    }

    #[test]
    fn a_small_base64_body_is_decoded() {
        use base64::Engine;
        assert_eq!(decode_base64("aGk=", 1024, "файл").unwrap(), b"hi");
        assert!(decode_base64("", 1024, "файл").is_err());
        assert!(decode_base64("!!!!", 1024, "файл").is_err());
        // Valid base64 that decodes to more than the limit.
        let over = base64::prelude::BASE64_STANDARD.encode(vec![0u8; 64]);
        assert!(decode_base64(&over, 10, "файл").is_err());
    }

    /// The cached server list is written whole and read back; a damaged file gives nothing
    /// instead of a panic (P2-36).
    #[test]
    fn the_server_cache_roundtrips_and_survives_damage() {
        let dir = std::env::temp_dir().join(format!("bebra-cache-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("servers-cache.json");

        let rows = vec![ServerRow {
            address: "ss14://a.test".into(),
            name: Some("A".into()),
            players: 1,
            soft_max_players: 2,
            tags: vec!["lang:ru".into()],
            map: None,
            preset: None,
            run_level: None,
            round_start_time: None,
            panic_bunker: None,
        }];
        let json = serde_json::to_vec(&rows).unwrap();
        store_cache_file(&path, &json).unwrap();
        assert_eq!(std::fs::read(&path).unwrap(), json);
        assert!(
            !path.with_extension("json.tmp").exists(),
            "the temporary file is gone after the rename"
        );
        assert_eq!(
            parse_cached_rows(&std::fs::read(&path).unwrap())
                .unwrap()
                .len(),
            1
        );

        assert!(parse_cached_rows(b"{ not json").is_err());
        assert!(parse_cached_rows(b"").is_err());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn addresses_are_canonicalized() {
        assert_eq!(
            server_address(" http://a.test:1212 ").unwrap(),
            "ss14://a.test:1212"
        );
        assert_eq!(
            server_address("https://b.test/").unwrap(),
            "ss14s://b.test/"
        );
        assert_eq!(server_address("c.test").unwrap(), "ss14://c.test");
        assert!(server_address("not a server").is_err());
    }

    /// Ping may not be turned into a port scanner (P2-33).
    #[test]
    fn local_and_private_addresses_are_not_pinged() {
        for host in [
            "127.0.0.1",
            "10.1.2.3",
            "192.168.0.1",
            "172.16.5.4",
            "169.254.1.1",
            "0.0.0.0",
            "100.64.1.1",
            "::1",
            "fd00::1",
            "fe80::1",
            "localhost",
            "hub.local",
            "box.internal",
            "printer.lan",
        ] {
            assert!(is_local_or_private_host(host), "{host} must be refused");
        }
        for host in [
            "hub.spacestation14.com",
            "8.8.8.8",
            "2a00:1450:4001:80f::200e",
        ] {
            assert!(!is_local_or_private_host(host), "{host} should be allowed");
        }
    }
}
