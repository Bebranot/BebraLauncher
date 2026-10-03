//! Starting the game for the UI: runs `bebra_core::play` on a thread of its own and reports
//! what happens as `play` events.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Mutex};

use bebra_core::accounts::{
    self, AccountStatus, CredentialStore, Freshness, StoredAccount, TokenStore,
};
use bebra_core::auth::AuthClient;
use bebra_core::download::{Progress, ProgressUnit};
use bebra_core::http::Client;
use bebra_core::launch;
use bebra_core::paths::DataDirs;
use bebra_core::play::{self, PlayRequest, Stage};
use bebra_core::ServerAddr;
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};
use time::OffsetDateTime;
use tokio::sync::Notify;
use uuid::Uuid;

pub const EVENT: &str = "play";

/// Returned by [`start`] while another game is being prepared or run.
pub const BUSY: &str = "Игра уже запускается или запущена";

#[derive(Debug, Clone, Serialize)]
#[serde(tag = "kind", rename_all = "snake_case")]
pub enum PlayEvent {
    /// A new step began. `account` -> `info` -> `files` -> `engine` -> `starting`.
    Stage {
        stage: &'static str,
    },
    Progress {
        stage: &'static str,
        done: u64,
        total: u64,
        unit: &'static str,
    },
    Running {
        pid: u32,
        plays_as: String,
    },
    Exited {
        code: Option<i32>,
        log_tail: Vec<String>,
    },
    Failed {
        message: String,
    },
    Cancelled,
}

/// At most one game is being prepared or run by the launcher at a time.
#[derive(Default)]
pub struct GameState {
    active: Mutex<Option<Arc<Notify>>>,
}

impl GameState {
    /// Takes the slot, or says that someone else holds it.
    fn begin(&self) -> Result<Arc<Notify>, String> {
        let mut active = self.active.lock().unwrap();
        if active.is_some() {
            return Err(BUSY.into());
        }
        let cancel = Arc::new(Notify::new());
        *active = Some(cancel.clone());
        Ok(cancel)
    }

    fn release(&self) {
        *self.active.lock().unwrap() = None;
    }

    /// Stops the preparation (downloads). Does nothing once the game itself is running.
    pub fn cancel(&self) {
        if let Some(c) = self.active.lock().unwrap().as_ref() {
            c.notify_one();
        }
    }
}

/// Frees the slot when the play thread ends, however it ends.
struct Release(Arc<GameState>);

impl Drop for Release {
    fn drop(&mut self) {
        self.0.release();
    }
}

pub struct PlayParams {
    pub client: Client,
    pub dirs: DataDirs,
    pub addr: ServerAddr,
    pub account: Option<Uuid>,
    pub guest_name: String,
    pub compat_mode: bool,
    pub minimize_on_play: bool,
}

pub fn start(app: AppHandle, game: Arc<GameState>, params: PlayParams) -> Result<(), String> {
    // Claim the slot before the thread starts so a second click is refused right away.
    let cancel = game.begin()?;
    let release = Release(game.clone());

    let spawned = std::thread::Builder::new()
        .name("play".into())
        .spawn(move || {
            let _release = release;

            let emit = |event: PlayEvent| {
                let _ = app.emit(EVENT, event);
            };
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    emit(PlayEvent::Failed {
                        message: format!("не удалось запустить рантайм: {e}"),
                    });
                    return;
                }
            };

            let prepared = rt.block_on(async {
                tokio::select! {
                    () = cancel.notified() => Err(None),
                    r = prepare(&app, &params) => r.map_err(Some),
                }
            });
            match prepared {
                Ok(game) => run_game(&app, &emit, &params, game),
                Err(None) => emit(PlayEvent::Cancelled),
                Err(Some(message)) => emit(PlayEvent::Failed { message }),
            }
        });
    spawned.map(drop).map_err(|e| {
        game.release();
        format!("не удалось запустить поток: {e}")
    })
}

async fn prepare(app: &AppHandle, params: &PlayParams) -> Result<play::PreparedGame, String> {
    let emit = |event: PlayEvent| {
        let _ = app.emit(EVENT, event);
    };

    emit(PlayEvent::Stage { stage: "account" });
    let account = match params.account {
        Some(id) => Some(resolve_account(&params.client, id).await?),
        None => None,
    };

    let loader_exe = launch::find_loader().map_err(|e| e.to_string())?;
    let req = PlayRequest {
        addr: params.addr.clone(),
        via_hub: false,
        account,
        guest_name: params.guest_name.clone(),
        compat_mode: params.compat_mode,
        no_connect: false,
        loader_exe,
        launcher_path: std::env::current_exe().ok(),
    };

    // Progress callbacks come once per file; only tell the UI when the percentage moves.
    let last = AtomicU64::new(u64::MAX);
    play::prepare(&params.client, &params.dirs, &req, &|stage, progress| {
        let name = stage_name(stage);
        match progress {
            None => {
                last.store(u64::MAX, Ordering::Relaxed);
                emit(PlayEvent::Stage { stage: name });
            }
            Some(Progress { done, total, unit }) => {
                let permille = (done * 1000).checked_div(total).unwrap_or(0);
                if last.swap(permille, Ordering::Relaxed) != permille {
                    emit(PlayEvent::Progress {
                        stage: name,
                        done,
                        total,
                        unit: match unit {
                            ProgressUnit::Items => "items",
                            ProgressUnit::Bytes => "bytes",
                        },
                    });
                }
            }
        }
    })
    .await
    .map_err(|e| e.to_string())
}

fn stage_name(stage: Stage) -> &'static str {
    match stage {
        Stage::Info => "info",
        Stage::Files => "files",
        Stage::Engine => "engine",
    }
}

/// The saved account, with its login checked (and renewed if it is about to expire).
async fn resolve_account(client: &Client, id: Uuid) -> Result<StoredAccount, String> {
    let store = CredentialStore::default();
    let saved = store
        .list()
        .map_err(|e| e.to_string())?
        .into_iter()
        .find(|a| a.user_id == id)
        .ok_or("Аккаунт не найден: войдите заново")?;

    let now = OffsetDateTime::now_utc();
    let auth = AuthClient::official(client.clone());
    match accounts::ensure_fresh(&auth, &store, saved.clone(), now, true).await {
        Ok((account, AccountStatus::Available)) => Ok(account),
        Ok((account, AccountStatus::Expired)) => Err(format!(
            "Вход в аккаунт {} истёк: войдите заново",
            account.username
        )),
        // The auth server can't be reached right now; the saved login may still work.
        Err(_) if accounts::freshness(saved.expire_time, now) != Freshness::Expired => Ok(saved),
        Err(e) => Err(e.to_string()),
    }
}

/// Brings the launcher window back if it was minimized.
pub fn restore_window(app: &AppHandle) {
    set_minimized(app, false);
}

/// Minimizes or restores the launcher window. Failures don't matter: it is only a convenience.
fn set_minimized(app: &AppHandle, minimized: bool) {
    let Some(window) = app.get_webview_window("main") else {
        return;
    };
    if minimized {
        let _ = window.minimize();
    } else {
        let _ = window.unminimize();
        let _ = window.set_focus();
    }
}

fn run_game(
    app: &AppHandle,
    emit: &impl Fn(PlayEvent),
    params: &PlayParams,
    game: play::PreparedGame,
) {
    let dirs = &params.dirs;
    emit(PlayEvent::Stage { stage: "starting" });
    let (out_log, err_log) = (
        dirs.logs().join("client.stdout.log"),
        dirs.logs().join("client.stderr.log"),
    );
    let mut child = match launch::spawn(&game.command, &out_log, &err_log) {
        Ok(c) => c,
        Err(e) => {
            emit(PlayEvent::Failed {
                message: e.to_string(),
            });
            return;
        }
    };
    emit(PlayEvent::Running {
        pid: child.id(),
        plays_as: game.plays_as,
    });
    if params.minimize_on_play {
        set_minimized(app, true);
    }

    let code = child.wait().ok().and_then(|s| s.code());
    let log_tail = if code == Some(0) {
        Vec::new()
    } else {
        let log = std::fs::read_to_string(&out_log).unwrap_or_default();
        let mut tail: Vec<String> = log.lines().rev().take(15).map(str::to_string).collect();
        tail.reverse();
        tail
    };
    if params.minimize_on_play {
        set_minimized(app, false);
    }
    emit(PlayEvent::Exited { code, log_tail });
}
