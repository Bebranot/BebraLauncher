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
use tauri::{AppHandle, Manager};
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

/// A game the launcher started: enough to tell it from an unrelated process that got the same
/// pid after the game ended.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RunningGame {
    pub pid: u32,
    /// The program that owns the pid. Empty only if it could not be known; then the pid alone
    /// has to be trusted.
    pub image_path: String,
}

impl RunningGame {
    fn is_alive(&self) -> bool {
        self.image_path.is_empty()
            || bebra_core::process::is_process_running_from(self.pid, &self.image_path)
    }
}

/// Drops the games whose process has ended; returns how many were dropped.
fn prune_with(games: &mut Vec<RunningGame>, is_alive: impl Fn(&RunningGame) -> bool) -> usize {
    let before = games.len();
    games.retain(is_alive);
    before - games.len()
}

/// Tracks running games and ensures at most one game is being prepared at a time.
#[derive(Default)]
pub struct GameState {
    preparing: Mutex<Option<Arc<Notify>>>,
    /// Games started by the launcher, including the ones recovered from the journal after a
    /// restart. Dead entries are dropped before every check: a launcher that crashed during
    /// play must not keep `is_active()` true for the rest of the session (P1-17).
    running: Mutex<Vec<RunningGame>>,
    token_refresh_lock: tokio::sync::Mutex<()>,
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

    pub fn register_running(&self, pid: u32, image_path: impl Into<String>) {
        let game = RunningGame {
            pid,
            image_path: image_path.into(),
        };
        let mut running = self.running.lock().unwrap();
        running.retain(|g| g.pid != pid);
        running.push(game);
    }

    pub fn unregister_running(&self, pid: u32) -> usize {
        let mut running = self.running.lock().unwrap();
        running.retain(|g| g.pid != pid);
        prune_with(&mut running, RunningGame::is_alive);
        running.len()
    }

    /// How many games are actually running now.
    pub fn running_count(&self) -> usize {
        let mut running = self.running.lock().unwrap();
        prune_with(&mut running, RunningGame::is_alive);
        running.len()
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
                // Kept when the page is not listening yet (cold start with `--commands`).
                crate::pending::emit(&app, EVENT, event);
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
                    r = prepare(&app, &game_clone, &params) => r.map_err(Some),
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

async fn prepare(
    app: &AppHandle,
    game: &GameState,
    params: &PlayParams,
) -> Result<play::PreparedGame, Failure> {
    let emit = |event: PlayEvent| {
        crate::pending::emit(app, EVENT, event);
    };

    emit(PlayEvent::Stage { stage: "account" });
    let account = match params.account {
        Some(id) => Some(resolve_account(game, &params.client, id).await?),
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
/// Uses single-flight locking to prevent race conditions during parallel game launches.
async fn resolve_account(
    game: &GameState,
    client: &Client,
    id: Uuid,
) -> Result<StoredAccount, Failure> {
    let _guard = game.token_refresh_lock.lock().await;

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
    let image_path = prepared_game.command.program.to_string_lossy().to_string();
    game.register_running(pid, image_path.clone());
    crate::applog::line(&format!("game started, pid {pid}"));
    remember_game(app, params);

    let session = crate::sessions::GameSession {
        id: session_id,
        pid,
        image_path,
        address: params.history_key.clone(),
        server_name: params.server_name.clone(),
        account: params.account.map(|a| a.to_string()),
        plays_as: prepared_game.plays_as.clone(),
        log_path: out_log.clone(),
        started_at_secs: std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_secs(),
    };
    app.state::<crate::AppState>()
        .sessions
        .record_start(dirs.root(), session);

    emit(PlayEvent::Running {
        pid,
        plays_as: prepared_game.plays_as,
        address: params.history_key.clone(),
    });
    if params.close_on_play {
        crate::applog::line("close_on_play enabled: session saved, exiting launcher");
        app.exit(0);
        return;
    }
    if params.minimize_on_play {
        set_minimized(app, true);
    }

    let started = std::time::Instant::now();
    let code = child.wait().ok().and_then(|s| s.code());
    let remaining = game.unregister_running(pid);
    // The journal entry goes first: the time below is credited by this run, and a crash between
    // the two must not let `recover_sessions` count the same time again (P1-22).
    app.state::<crate::AppState>()
        .sessions
        .record_end(dirs.root(), pid);
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

#[cfg(test)]
mod tests {
    use super::*;

    fn game(pid: u32, path: &str) -> RunningGame {
        RunningGame {
            pid,
            image_path: path.into(),
        }
    }

    #[test]
    fn dead_games_are_dropped_and_live_ones_kept() {
        let mut games = vec![
            game(1, r"C:\game\Robust.exe"),
            game(2, r"C:\game\Robust.exe"),
            game(3, ""),
        ];
        let dropped = prune_with(&mut games, |g| g.pid != 2);
        assert_eq!(dropped, 1);
        assert_eq!(games.iter().map(|g| g.pid).collect::<Vec<_>>(), [1, 3]);
    }

    /// A pid recovered after a launcher crash must not keep `is_active()` true after the game
    /// is closed (P1-17): its entry is checked against the live process before every count.
    #[test]
    fn a_recovered_game_that_exited_stops_being_active() {
        let state = GameState::default();
        // A pid that certainly has no process behind it, with the program it used to run.
        state.register_running(u32::MAX - 7, r"C:\game\Robust.exe");
        state.register_running(u32::MAX - 8, "");
        let live = state.running_count();
        assert_eq!(
            live, 1,
            "the entry with an unknown program is kept, the verifiable dead one is dropped"
        );
        assert!(state.is_active(), "an unverifiable game is assumed running");
        assert_eq!(state.unregister_running(u32::MAX - 8), 0);
        assert!(!state.is_active(), "nothing is left, so the game is over");
    }

    #[test]
    fn registering_the_same_pid_twice_keeps_one_entry() {
        let state = GameState::default();
        // A pid without a known program: always considered alive, so the count is stable.
        state.register_running(u32::MAX - 9, "");
        state.register_running(u32::MAX - 9, "");
        assert_eq!(state.running_count(), 1);
    }
}
