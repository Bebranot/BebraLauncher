//! Starting the game for the UI: runs `bebra_core::play` on a thread of its own and reports
//! what happens as `play` events.

use std::path::PathBuf;
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
        address: String,
    },
    Exited {
        pid: u32,
        code: Option<i32>,
        log_tail: Vec<String>,
    },
    /// `code` names the problem (the UI translates it), `message` is the technical detail.
    Failed {
        code: &'static str,
        message: String,
    },
    Cancelled,
    /// Debug: what would be started, with secrets hidden. The game is not started.
    Preview {
        command: String,
    },
}

/// Tracks running games and ensures at most one game is being prepared at a time.
#[derive(Default)]
pub struct GameState {
    preparing: Mutex<Option<Arc<Notify>>>,
    running: Mutex<std::collections::HashSet<u32>>,
}

impl GameState {
    /// Takes the preparation slot, or says that another game is currently preparing.
    fn begin_prepare(&self) -> Result<Arc<Notify>, String> {
        let mut prep = self.preparing.lock().unwrap();
        if prep.is_some() {
            return Err(BUSY.into());
        }
        let cancel = Arc::new(Notify::new());
        *prep = Some(cancel.clone());
        Ok(cancel)
    }

    fn end_prepare(&self) {
        *self.preparing.lock().unwrap() = None;
    }

    pub fn is_preparing(&self) -> bool {
        self.preparing.lock().unwrap().is_some()
    }

    pub fn register_running(&self, pid: u32) {
        self.running.lock().unwrap().insert(pid);
    }

    pub fn unregister_running(&self, pid: u32) -> usize {
        let mut r = self.running.lock().unwrap();
        r.remove(&pid);
        r.len()
    }

    pub fn running_count(&self) -> usize {
        self.running.lock().unwrap().len()
    }

    /// Is a game being prepared or running?
    pub fn is_active(&self) -> bool {
        self.is_preparing() || self.running_count() > 0
    }

    /// Stops the preparation (downloads). Does nothing once the game itself is running.
    pub fn cancel(&self) {
        if let Some(c) = self.preparing.lock().unwrap().as_ref() {
            c.notify_one();
        }
    }
}

/// Frees the preparation slot when the preparation ends, however it ends.
struct PrepareRelease(Arc<GameState>);

impl Drop for PrepareRelease {
    fn drop(&mut self) {
        self.0.end_prepare();
    }
}

pub struct PlayParams {
    pub client: Client,
    pub dirs: DataDirs,
    pub addr: ServerAddr,
    pub account: Option<Uuid>,
    pub guest_name: String,
    pub compat_mode: bool,
    pub discord_rpc: bool,
    pub minimize_on_play: bool,
    pub close_on_play: bool,
    pub client_window_mode: String,
    pub client_mute_unfocused: bool,
    pub client_ui_scale: String,
    pub client_max_fps: u32,
    /// The address string the history is keyed by (as the hub lists it).
    pub history_key: String,
    /// The server's name, for the history.
    pub server_name: Option<String>,
    /// Text packs to mount over the game's files.
    pub enabled_packs: Vec<String>,
    /// Prepare everything but do not start the game; report the command instead.
    pub preview: bool,
    /// Log the launch command (secrets hidden).
    pub verbose: bool,
    /// Extra engine arguments / cvars.
    pub extra_args: String,
    /// Client patch assemblies (.dll) to load.
    pub patches: Vec<PathBuf>,
    /// Client performance and optimization settings.
    pub optimization: bebra_core::launch::OptimizationConfig,
}

pub fn start(app: AppHandle, game: Arc<GameState>, params: PlayParams) -> Result<(), String> {
    // Claim the preparation slot before the thread starts so a second click is refused right away.
    let cancel = game.begin_prepare()?;
    crate::applog::line(&format!(
        "play {} (account: {})",
        params.addr.as_str(),
        if params.account.is_some() {
            "yes"
        } else {
            "guest"
        }
    ));
    let game_clone = game.clone();

    let spawned = std::thread::Builder::new()
        .name("play".into())
        .spawn(move || {
            let release = PrepareRelease(game_clone.clone());

            let emit = |event: PlayEvent| {
                let _ = app.emit(EVENT, event);
            };
            let rt = match tokio::runtime::Builder::new_current_thread()
                .enable_all()
                .build()
            {
                Ok(rt) => rt,
                Err(e) => {
                    emit(PlayEvent::failed("other", format!("runtime: {e}")));
                    return;
                }
            };

            let prepared = rt.block_on(async {
                tokio::select! {
                    () = cancel.notified() => Err(None),
                    r = prepare(&app, &params) => r.map_err(Some),
                }
            });

            // Preparation finished: release the preparation lock so other games can prepare.
            drop(release);

            match prepared {
                Ok(prepared_game) if params.preview => {
                    crate::applog::line("launch preview");
                    emit(PlayEvent::Preview {
                        command: prepared_game.command.describe(),
                    });
                }
                Ok(prepared_game) => {
                    if params.verbose {
                        crate::applog::line(&format!(
                            "launch command: {}",
                            prepared_game.command.describe()
                        ));
                    }
                    run_game(&app, &emit, &params, &game_clone, prepared_game);
                }
                Err(None) => emit(PlayEvent::Cancelled),
                Err(Some(failure)) => emit(PlayEvent::failed(failure.code, failure.message)),
            }
        });
    spawned.map(drop).map_err(|e| {
        game.end_prepare();
        format!("не удалось запустить поток: {e}")
    })
}

/// Why the game could not be started: a code for the UI and the technical detail.
struct Failure {
    code: &'static str,
    message: String,
}

impl Failure {
    fn new(code: &'static str, message: impl Into<String>) -> Self {
        Self {
            code,
            message: message.into(),
        }
    }
}

impl PlayEvent {
    pub fn failed(code: &'static str, message: impl Into<String>) -> Self {
        let message = message.into();
        crate::applog::line(&format!("play failed [{code}]: {message}"));
        Self::Failed { code, message }
    }
}

async fn prepare(app: &AppHandle, params: &PlayParams) -> Result<play::PreparedGame, Failure> {
    let emit = |event: PlayEvent| {
        let _ = app.emit(EVENT, event);
    };

    emit(PlayEvent::Stage { stage: "account" });
    let account = match params.account {
        Some(id) => Some(resolve_account(&params.client, id).await?),
        None => None,
    };

    let loader_exe =
        launch::find_loader().map_err(|e| Failure::new("loader_missing", e.to_string()))?;

    // Text packs are optional: a problem with them must not keep the player from playing.
    let overlay_zip = match bebra_core::packs::build_overlay(&params.dirs, &params.enabled_packs) {
        Ok(Some(overlay)) => {
            crate::applog::line(&format!(
                "text packs: {} ({} files)",
                overlay.used.join(", "),
                overlay.files
            ));
            Some(overlay.path)
        }
        Ok(None) => None,
        Err(e) => {
            crate::applog::line(&format!("text packs skipped: {e}"));
            None
        }
    };
    let mut extra_args = Vec::new();
    if !params.discord_rpc {
        extra_args.push("--cvar".to_string());
        extra_args.push("discord.enabled=false".to_string());
    }
    match params.client_window_mode.as_str() {
        "windowed" => {
            extra_args.push("--cvar".to_string());
            extra_args.push("display.window_mode=0".to_string());
        }
        "fullscreen" => {
            extra_args.push("--cvar".to_string());
            extra_args.push("display.window_mode=1".to_string());
        }
        "borderless" => {
            extra_args.push("--cvar".to_string());
            extra_args.push("display.window_mode=2".to_string());
        }
        _ => {}
    }
    if !params.client_mute_unfocused {
        extra_args.push("--cvar".to_string());
        extra_args.push("audio.mute_when_unfocused=false".to_string());
    }
    if params.client_ui_scale != "auto" && !params.client_ui_scale.is_empty() {
        extra_args.push("--cvar".to_string());
        extra_args.push(format!("display.ui_scale={}", params.client_ui_scale));
    }
    if params.client_max_fps > 0 {
        extra_args.push("--cvar".to_string());
        extra_args.push(format!("display.max_fps={}", params.client_max_fps));
    }
    // User-defined extra args take precedence over UI defaults.
    extra_args.extend(bebra_core::launch::split_args(&params.extra_args));
    let req = PlayRequest {
        addr: params.addr.clone(),
        via_hub: false,
        account,
        guest_name: params.guest_name.clone(),
        compat_mode: params.compat_mode,
        no_connect: false,
        loader_exe,
        launcher_path: std::env::current_exe().ok(),
        overlay_zip,
        extra_args,
        patches: params.patches.clone(),
        optimization: params.optimization.clone(),
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
    .map_err(|e| Failure::new(e.code(), e.detail()))
}

fn stage_name(stage: Stage) -> &'static str {
    match stage {
        Stage::Info => "info",
        Stage::Files => "files",
        Stage::Engine => "engine",
    }
}

/// The saved account, with its login checked (and renewed if it is about to expire).
async fn resolve_account(client: &Client, id: Uuid) -> Result<StoredAccount, Failure> {
    let store = CredentialStore::default();
    let saved = store
        .list()
        .map_err(|e| Failure::new("other", e.to_string()))?
        .into_iter()
        .find(|a| a.user_id == id)
        .ok_or_else(|| Failure::new("account_missing", ""))?;

    let now = OffsetDateTime::now_utc();
    let auth = AuthClient::official(client.clone());
    match accounts::ensure_fresh(&auth, &store, saved.clone(), now, true).await {
        Ok((account, AccountStatus::Available)) => Ok(account),
        Ok((account, AccountStatus::Expired)) => {
            Err(Failure::new("account_expired", account.username))
        }
        // The auth server can't be reached right now; the saved login may still work.
        Err(_) if accounts::freshness(saved.expire_time, now) != Freshness::Expired => Ok(saved),
        Err(e) => Err(Failure::new("other", e.to_string())),
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

/// The game is running: this server goes to the top of the history and this account is the
/// one to preselect next time.
fn remember_game(app: &AppHandle, params: &PlayParams) {
    let state = app.state::<crate::AppState>();
    let mut settings = state.settings.lock().unwrap();
    // An empty string means: the player chose to play as a guest.
    settings.last_account = Some(params.account.map_or_else(String::new, |a| a.to_string()));
    let now = time::OffsetDateTime::now_utc()
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    settings.remember_played(&params.history_key, params.server_name.clone(), now);
    let account = params.account.map_or_else(String::new, |a| a.to_string());
    settings.remember_account(&params.history_key, &account);
    let _ = settings.save(state.dirs.root());
}

/// Adds the time of this game to the server's total.
fn record_play_time(app: &AppHandle, key: &str, seconds: u64) {
    let state = app.state::<crate::AppState>();
    let mut settings = state.settings.lock().unwrap();
    settings.add_play_time(key, seconds);
    let _ = settings.save(state.dirs.root());
}

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);

fn run_game(
    app: &AppHandle,
    emit: &impl Fn(PlayEvent),
    params: &PlayParams,
    game: &Arc<GameState>,
    prepared_game: play::PreparedGame,
) {
    let dirs = &params.dirs;
    emit(PlayEvent::Stage { stage: "starting" });

    let session_id = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
    let (out_log, err_log) = if game.running_count() == 0 {
        (
            dirs.logs().join("client.stdout.log"),
            dirs.logs().join("client.stderr.log"),
        )
    } else {
        (
            dirs.logs().join(format!("client-{session_id}.stdout.log")),
            dirs.logs().join(format!("client-{session_id}.stderr.log")),
        )
    };

    let mut child = match launch::spawn(&prepared_game.command, &out_log, &err_log) {
        Ok(c) => c,
        Err(e) => {
            emit(PlayEvent::failed("start", e.to_string()));
            return;
        }
    };
    let pid = child.id();
    game.register_running(pid);
    crate::applog::line(&format!("game started, pid {pid}"));
    remember_game(app, params);
    emit(PlayEvent::Running {
        pid,
        plays_as: prepared_game.plays_as,
        address: params.history_key.clone(),
    });
    if params.close_on_play {
        crate::applog::line("close_on_play enabled: exiting launcher");
        app.exit(0);
        return;
    }
    if params.minimize_on_play {
        set_minimized(app, true);
    }

    let started = std::time::Instant::now();
    let code = child.wait().ok().and_then(|s| s.code());
    let remaining = game.unregister_running(pid);
    record_play_time(app, &params.history_key, started.elapsed().as_secs());
    crate::applog::line(&format!(
        "game pid {pid} exited, code {code:?}, after {} s (remaining active: {remaining})",
        started.elapsed().as_secs()
    ));
    let log_tail = if code == Some(0) {
        Vec::new()
    } else {
        let log = std::fs::read_to_string(&out_log).unwrap_or_default();
        let mut tail: Vec<String> = log.lines().rev().take(15).map(str::to_string).collect();
        tail.reverse();
        tail
    };
    if params.minimize_on_play && remaining == 0 {
        set_minimized(app, false);
    }
    emit(PlayEvent::Exited {
        pid,
        code,
        log_tail,
    });
}
