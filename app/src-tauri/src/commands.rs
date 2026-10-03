//! Commands the UI can call. Everything the UI learns about accounts goes through here, and
//! tokens never do: the UI sees names, ids and expiry, nothing it could use to log in.

use bebra_core::accounts::{
    self, CredentialStore, Freshness, LoginResult, StoredAccount, TokenStore,
};
use bebra_core::auth::{AuthClient, Identity};
use bebra_core::content_db::ContentDb;
use bebra_core::hub::{HubClient, HubServerEntry};
use bebra_core::launch;
use bebra_core::server::{self, AuthMode};
use bebra_core::storage;
use bebra_core::ServerAddr;
use serde::{Deserialize, Serialize};
use tauri::{AppHandle, Emitter, Manager, State};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::game::{self, PlayParams};
use crate::redial::{self, Action};
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
        let s = e.status;
        // The server's own tags, plus the ones the hub worked out; no duplicates.
        let mut tags = s.tags;
        for t in e.inferred_tags {
            if !tags.contains(&t) {
                tags.push(t);
            }
        }
        Self {
            address: e.address,
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
    let servers = HubClient::official(state.client.clone())
        .list_servers()
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

#[derive(Serialize)]
pub struct StorageRow {
    content: u64,
    engines: u64,
    modules: u64,
    other: u64,
    total: u64,
}

#[tauri::command]
pub fn storage_info(state: State<'_, AppState>) -> StorageRow {
    let r = storage::report(&state.dirs);
    StorageRow {
        content: r.content_db,
        engines: r.engines,
        modules: r.modules,
        other: r.scratch + r.logs,
        total: r.total(),
    }
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
pub fn clean_storage(state: State<'_, AppState>) -> CmdResult<CleanRow> {
    if state.game.is_active() {
        return Err("Очистка недоступна, пока идёт запуск или игра".into());
    }
    let db = ContentDb::open(&state.dirs.content_db()).map_err(err)?;
    let r = storage::clean(&state.dirs, &db).map_err(err)?;
    Ok(CleanRow {
        freed: r.bytes_freed,
        versions_removed: r.cull.versions_removed,
        engines_removed: r.engines_removed,
        modules_removed: r.modules_removed,
    })
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
pub fn list_accounts() -> CmdResult<Vec<AccountRow>> {
    let now = OffsetDateTime::now_utc();
    let mut rows: Vec<AccountRow> = CredentialStore::default()
        .list()
        .map_err(err)?
        .iter()
        .map(|a| AccountRow::new(a, now))
        .collect();
    rows.sort_by_key(|a| a.username.to_lowercase());
    Ok(rows)
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
        LoginResult::Success(account) => Ok(LoginOutcome::Ok {
            account: AccountRow::new(&account, OffsetDateTime::now_utc()),
        }),
        LoginResult::Denied { errors, code } => {
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
pub fn get_settings(state: State<'_, AppState>) -> Settings {
    state.settings.lock().unwrap().clone()
}

#[tauri::command]
pub fn set_settings(state: State<'_, AppState>, settings: Settings) -> CmdResult<Settings> {
    let mut current = state.settings.lock().unwrap();
    // The form only edits these five; favourites and history have their own commands.
    let merged = Settings {
        guest_name: settings.guest_name,
        compat_mode: settings.compat_mode,
        minimize_on_play: settings.minimize_on_play,
        theme: settings.theme,
        language: settings.language,
        ..current.clone()
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
    start_play(&app, &state, &address, name, account)
}

/// Starts the game and, if it did start, remembers the server and the account.
pub fn start_play(
    app: &AppHandle,
    state: &AppState,
    address: &str,
    name: Option<String>,
    account: Option<Uuid>,
) -> CmdResult<()> {
    let addr = ServerAddr::parse(address).map_err(err)?;
    let settings = state.settings.lock().unwrap().clone();

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
            minimize_on_play: settings.minimize_on_play,
            history_key: address.to_string(),
        },
    )?;

    let mut s = state.settings.lock().unwrap();
    s.last_account = account.map(|a| a.to_string());
    let now = OffsetDateTime::now_utc()
        .format(&Rfc3339)
        .unwrap_or_default();
    s.remember_played(address, name, now);
    let _ = s.save(state.dirs.root());
    Ok(())
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
            Action::Connect { address, reason } => {
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
        match start_play(app, &state, &address, None, account) {
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

#[tauri::command]
pub fn open_logs_folder(state: State<'_, AppState>) -> CmdResult<()> {
    let dir = state.dirs.logs();
    std::fs::create_dir_all(&dir).map_err(err)?;
    // explorer.exe exits with 1 even when it works, so the exit code means nothing.
    std::process::Command::new("explorer")
        .arg(dir)
        .spawn()
        .map(drop)
        .map_err(err)
}
