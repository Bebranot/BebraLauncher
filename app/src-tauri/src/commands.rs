//! Commands the UI can call. Everything the UI learns about accounts goes through here, and
//! tokens never do: the UI sees names, ids and expiry, nothing it could use to log in.

use bebra_core::accounts::{
    self, CredentialStore, Freshness, LoginResult, StoredAccount, TokenStore,
};
use bebra_core::auth::{AuthClient, Identity};
use bebra_core::hub::{HubClient, HubServerEntry};
use bebra_core::launch;
use bebra_core::ServerAddr;
use serde::Serialize;
use tauri::{AppHandle, State};
use time::format_description::well_known::Rfc3339;
use time::OffsetDateTime;
use uuid::Uuid;

use crate::game::{self, PlayParams};
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
#[derive(Serialize)]
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
    Ok(servers.into_iter().map(ServerRow::from).collect())
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
    // The form only edits these two; favourites and history have their own commands.
    let merged = Settings {
        guest_name: settings.guest_name,
        compat_mode: settings.compat_mode,
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
    let addr = ServerAddr::parse(&address).map_err(err)?;
    let account = account
        .map(|a| a.parse::<Uuid>().map_err(|_| "Неверный id аккаунта"))
        .transpose()?;

    let settings = {
        let mut s = state.settings.lock().unwrap();
        s.last_account = account.map(|a| a.to_string());
        let now = OffsetDateTime::now_utc()
            .format(&Rfc3339)
            .unwrap_or_default();
        s.remember_played(&address, name, now);
        let _ = s.save(state.dirs.root());
        s.clone()
    };

    game::start(
        app,
        state.game.clone(),
        PlayParams {
            client: state.client.clone(),
            dirs: state.dirs.clone(),
            addr,
            account,
            guest_name: settings.guest_name,
            compat_mode: settings.compat_mode,
        },
    )
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
