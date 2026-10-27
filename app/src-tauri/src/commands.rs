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
use tauri::{AppHandle, Emitter, Manager, State};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::game::{self, PlayParams};
use crate::news::{self, NewsItem};
use crate::redial::{self, Action};
use crate::selfupdate;
use crate::settings::Settings;
use crate::AppState;

type CmdResult<T> = Result<T, String>;

fn err(e: impl std::fmt::Display) -> String {
    e.to_string()
}

#[derive(Serialize)]
pub struct AppInfo {
    version: &'static str,
    data_dir: String,
    /// `Some(message)` when the game loader cannot be found, so playing is impossible.
    loader_problem: Option<String>,
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
        let s = state.settings.lock().unwrap();
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
    if let Ok(json) = serde_json::to_vec(&rows) {
        let _ = std::fs::write(cache_path(&state), json);
    }
    Ok(rows)
}

fn cache_path(state: &AppState) -> std::path::PathBuf {
    state.dirs.root().join("servers-cache.json")
}

/// The server list of the last successful refresh (maybe long ago); empty if there is none.
#[tauri::command]
pub fn cached_servers(state: State<'_, AppState>) -> Vec<ServerRow> {
    std::fs::read(cache_path(&state))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
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
    let store = CredentialStore::default();
    let accounts_list = store.list().map_err(err)?;
    let account = accounts_list
        .iter()
        .find(|a| a.user_id == id)
        .ok_or("Аккаунт не найден")?;
    let told = accounts::logout(&AuthClient::official(state.client.clone()), &store, account)
        .await
        .map_err(err)?;

    let mut settings = state.settings.lock().unwrap();
    if settings.last_account.as_deref() == Some(user_id.as_str()) {
        settings.last_account = None;
        let _ = settings.save(state.dirs.root());
    }
    Ok(told)
}

#[tauri::command]
pub fn select_account(state: State<'_, AppState>, account: Option<String>) -> CmdResult<()> {
    let acc_str = account.unwrap_or_default();
    if !acc_str.is_empty() {
        acc_str
            .parse::<Uuid>()
            .map_err(|_| "Неверный id аккаунта")?;
    }
    let mut settings = state.settings.lock().unwrap();
    settings.last_account = Some(acc_str);
    settings.save(state.dirs.root()).map_err(err)?;
    Ok(())
}

#[tauri::command]
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
pub fn set_settings(state: State<'_, AppState>, settings: Settings) -> CmdResult<Settings> {
    let mut current = state.settings.lock().unwrap();
    // The UI edits everything except what the launcher keeps itself: favourites (own
    // commands), history and the last used account (written when a game starts).
    let merged = Settings {
        favorites: current.favorites.clone(),
        history: current.history.clone(),
        last_account: current.last_account.clone(),
        ..settings
    }
    .sanitized();
    merged.save(state.dirs.root()).map_err(err)?;
    *current = merged.clone();
    Ok(merged)
}

/// Adds or removes a favourite server. Returns the new list of favourites.
#[tauri::command]
pub fn toggle_favorite(state: State<'_, AppState>, address: String) -> CmdResult<Vec<String>> {
    let mut s = state.settings.lock().unwrap();
    s.toggle_favorite(&address);
    s.save(state.dirs.root()).map_err(err)?;
    Ok(s.favorites.clone())
}

/// Adds a favourite server by address (one the hub may not list). Returns the new list.
#[tauri::command]
pub fn add_favorite(state: State<'_, AppState>, address: String) -> CmdResult<Vec<String>> {
    let address = address.trim().to_string();
    ServerAddr::parse(&address).map_err(err)?;
    let mut s = state.settings.lock().unwrap();
    if !s.favorites.contains(&address) {
        s.favorites.push(address);
    }
    s.save(state.dirs.root()).map_err(err)?;
    Ok(s.favorites.clone())
}

/// Moves a favourite server to the top of the list. Returns the new list of favourites.
#[tauri::command]
pub fn raise_favorite(state: State<'_, AppState>, address: String) -> CmdResult<Vec<String>> {
    let mut s = state.settings.lock().unwrap();
    s.raise_favorite(&address);
    s.save(state.dirs.root()).map_err(err)?;
    Ok(s.favorites.clone())
}

/// Moves a favourite server up (delta = -1) or down (delta = 1).
#[tauri::command]
pub fn move_favorite(state: State<'_, AppState>, address: String, delta: i32) -> CmdResult<Vec<String>> {
    let mut s = state.settings.lock().unwrap();
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
    let new_address = new_address.trim().to_string();
    ServerAddr::parse(&new_address).map_err(err)?;
    let mut s = state.settings.lock().unwrap();
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
    let mut s = state.settings.lock().unwrap();
    s.set_favorite_name(&address, name);
    s.save(state.dirs.root()).map_err(err)?;
    Ok(s.favorite_names.clone())
}

/// Returns true if a desktop 13th/14th Gen Intel CPU is detected and the warning was not yet dismissed.
#[tauri::command]
pub fn check_intel_warning(state: State<'_, AppState>) -> bool {
    let s = state.settings.lock().unwrap();
    if s.dismissed_intel_warning {
        return false;
    }
    crate::system::is_vulnerable_intel_cpu()
}

/// Dismisses the Intel 13th/14th Gen CPU degradation advisory.
#[tauri::command]
pub fn dismiss_intel_warning(state: State<'_, AppState>) -> CmdResult<()> {
    let mut s = state.settings.lock().unwrap();
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
    let mut s = state.settings.lock().unwrap();
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
    let settings = state.settings.lock().unwrap().clone();

    let patches = bebra_core::patches::resolve_patches(&state.dirs, &settings.enabled_patches);
    let optimization = settings.optimization_config();

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
            discord_rpc: settings.discord_rpc,
            minimize_on_play: settings.minimize_on_play,
            close_on_play: settings.close_on_play,
            client_window_mode: settings.client_window_mode,
            client_mute_unfocused: settings.client_mute_unfocused,
            client_ui_scale: settings.client_ui_scale,
            client_max_fps: settings.client_max_fps,
            enabled_packs: settings.enabled_packs,
            extra_args: settings.extra_args,
            patches,
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
    for action in redial::actions(args) {
        match action {
            Action::Activate => game::restore_window(app),
            Action::Open { address } => {
                crate::applog::line(&format!("link opened: {address}"));
                game::restore_window(app);
                let _ = app.emit("open-link", address);
            }
            Action::Connect { address, reason } => {
                let state = app.state::<AppState>();
                if state.settings.lock().unwrap().ignore_redial {
                    crate::applog::line(&format!("redial to {address} ignored (setting)"));
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
        let _ = app.emit(
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

    crate::applog::line(&format!("redial to {address}"));
    game::restore_window(app);
    let _ = app.emit(
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
                let _ = app.emit(game::EVENT, game::PlayEvent::failed("other", e));
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

fn is_supported_bg_extension(ext: &str) -> bool {
    let lower = ext.to_ascii_lowercase();
    SUPPORTED_BG_EXTENSIONS.iter().any(|&e| e == lower)
}

fn validate_bg_filename(name: &str) -> CmdResult<()> {
    let clean = name.trim();
    if clean.is_empty()
        || clean.len() > 200
        || clean.contains(['/', '\\', ':'])
        || clean.contains("..")
    {
        return Err("Недопустимое имя файла".into());
    }
    let p = std::path::Path::new(clean);
    if p.file_name() != Some(std::ffi::OsStr::new(clean)) {
        return Err("Недопустимое имя файла".into());
    }
    let ext = p.extension().and_then(|s| s.to_str()).unwrap_or("");
    if !is_supported_bg_extension(ext) {
        return Err("Недопустимый тип файла изображения".into());
    }
    Ok(())
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
                    rows.push(BackgroundRow { id, name, file_size });
                }
            }
        }
    }
    rows.sort_by_key(|a| a.name.to_lowercase());
    Ok(rows)
}

#[tauri::command]
pub fn save_background(
    state: State<'_, AppState>,
    name: String,
    data_base64: String,
) -> CmdResult<BackgroundRow> {
    let dir = state.dirs.backgrounds();
    std::fs::create_dir_all(&dir).map_err(err)?;

    let raw_name: String = name
        .chars()
        .filter(|c| c.is_alphanumeric() || matches!(*c, '-' | '_' | '.' | ' '))
        .collect();
    let clean = raw_name.trim();
    if clean.is_empty() {
        return Err("Имя файла не может быть пустым".into());
    }

    let ext = std::path::Path::new(clean)
        .extension()
        .and_then(|s| s.to_str())
        .map(|s| s.to_ascii_lowercase());

    let final_filename = match ext {
        Some(e) if is_supported_bg_extension(&e) => clean.to_string(),
        _ => format!("{clean}.png"),
    };

    let payload = if let Some(idx) = data_base64.find(";base64,") {
        &data_base64[idx + 8..]
    } else {
        &data_base64
    };

    use base64::Engine;
    let bytes = base64::prelude::BASE64_STANDARD
        .decode(payload.trim())
        .map_err(|e| format!("Не удалось декодировать изображение: {e}"))?;

    if bytes.is_empty() {
        return Err("Файл пуст".into());
    }
    if bytes.len() > 30 * 1024 * 1024 {
        return Err("Файл слишком большой (максимум 30 МБ)".into());
    }

    let file_path = dir.join(&final_filename);
    std::fs::write(&file_path, &bytes).map_err(err)?;

    let file_size = bytes.len() as u64;
    let name_stem = std::path::Path::new(&final_filename)
        .file_stem()
        .map(|s| s.to_string_lossy().to_string())
        .unwrap_or_else(|| final_filename.clone());

    Ok(BackgroundRow {
        id: final_filename,
        name: name_stem,
        file_size,
    })
}

#[tauri::command]
pub fn get_background(state: State<'_, AppState>, name: String) -> CmdResult<String> {
    validate_bg_filename(&name)?;
    let path = state.dirs.backgrounds().join(&name);
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
}

#[tauri::command]
pub fn delete_background(state: State<'_, AppState>, name: String) -> CmdResult<()> {
    validate_bg_filename(&name)?;
    let path = state.dirs.backgrounds().join(&name);
    if path.is_file() {
        let _ = std::fs::remove_file(&path);
    }

    let mut settings = state.settings.lock().unwrap();
    if settings.bg_custom_name.as_deref() == Some(&name) {
        settings.bg_custom_name = None;
        let _ = settings.save(state.dirs.root());
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

/// Puts every option back to its default. Favourites, history, the chosen account and the
/// saved logins stay.
#[tauri::command]
pub fn reset_settings(state: State<'_, AppState>) -> CmdResult<Settings> {
    let mut current = state.settings.lock().unwrap();
    let fresh = Settings {
        favorites: current.favorites.clone(),
        history: current.history.clone(),
        last_account: current.last_account.clone(),
        ..Settings::default()
    };
    fresh.save(state.dirs.root()).map_err(err)?;
    *current = fresh.clone();
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
    let enabled = state.settings.lock().unwrap().enabled_packs.clone();
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
    file_size: u64,
    enabled: bool,
}

#[tauri::command]
pub async fn list_patches(state: State<'_, AppState>) -> CmdResult<Vec<PatchRow>> {
    let dirs = state.dirs.clone();
    let enabled = state.settings.lock().unwrap().enabled_patches.clone();
    tokio::task::spawn_blocking(move || {
        let patches = bebra_core::patches::scan_patches(&dirs).map_err(err)?;
        Ok(patches
            .into_iter()
            .map(|p| PatchRow {
                enabled: enabled.contains(&p.id),
                id: p.id,
                name: p.name,
                file_size: p.file_size,
            })
            .collect())
    })
    .await
    .map_err(err)?
}

/// How long the news feed is kept before it is read again.
const NEWS_TTL: std::time::Duration = std::time::Duration::from_secs(10 * 60);

#[tauri::command]
pub async fn news(state: State<'_, AppState>) -> CmdResult<Vec<NewsItem>> {
    if let Some((at, items)) = state.news.lock().unwrap().as_ref() {
        if at.elapsed() < NEWS_TTL {
            return Ok(items.clone());
        }
    }
    let resp = state
        .client
        .get(news::FEED_URL)
        .timeout(std::time::Duration::from_secs(10))
        .send()
        .await
        .map_err(err)?;
    if !resp.status().is_success() {
        return Err(format!("HTTP {}", resp.status().as_u16()));
    }
    let text = resp.text().await.map_err(err)?;
    let items = news::parse_feed(&text);
    *state.news.lock().unwrap() = Some((std::time::Instant::now(), items.clone()));
    Ok(items)
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

/// Asks each server for its `/status` and times the answer. This is what the list's ping
/// column shows, and how favourite servers that are not on the hub get their numbers.
#[tauri::command]
pub async fn ping_servers(
    state: State<'_, AppState>,
    addresses: Vec<String>,
) -> CmdResult<Vec<PingRow>> {
    let gate = std::sync::Arc::new(tokio::sync::Semaphore::new(PING_PARALLEL));
    let mut tasks = tokio::task::JoinSet::new();
    for address in addresses.into_iter().take(PING_MAX_SERVERS) {
        let Ok(addr) = ServerAddr::parse(&address) else {
            continue;
        };
        let client = state.client.clone();
        let gate = gate.clone();
        tasks.spawn(async move {
            let _slot = gate.acquire().await;
            let started = std::time::Instant::now();
            let status = server::fetch_status(&client, &addr).await.ok();
            let ms = status
                .is_some()
                .then(|| started.elapsed().as_millis().min(u128::from(u32::MAX)) as u32);
            let status = status.map(|s| ServerRow::from_status(address.clone(), s, Vec::new()));
            PingRow {
                address,
                ms,
                status,
            }
        });
    }
    let mut rows = Vec::new();
    while let Some(done) = tasks.join_next().await {
        if let Ok(row) = done {
            rows.push(row);
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
