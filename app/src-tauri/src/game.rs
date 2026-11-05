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
pub const BUSY: &str = "РРіСЂР° СѓР¶Рµ Р·Р°РїСѓСЃРєР°РµС‚СЃСЏ РёР»Рё Р·Р°РїСѓС‰РµРЅР°";

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
        /// Payload received so far; the UI derives the speed from consecutive values.
        bytes: u64,
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
        let mut prep = crate::lock_or_recover(&self.preparing);
        if prep.is_some() {
            return Err(BUSY.into());
        }
        let cancel = Arc::new(Notify::new());
        *prep = Some(cancel.clone());
        Ok(cancel)
    }

    fn end_prepare(&self) {
        *crate::lock_or_recover(&self.preparing) = None;
    }

    pub fn is_preparing(&self) -> bool {
        crate::lock_or_recover(&self.preparing).is_some()
    }

    pub fn register_running(&self, pid: u32, image_path: impl Into<String>) {
        let game = RunningGame {
            pid,
            image_path: image_path.into(),
        };
        let mut running = crate::lock_or_recover(&self.running);
        running.retain(|g| g.pid != pid);
        running.push(game);
    }

    pub fn unregister_running(&self, pid: u32) -> usize {
        let mut running = crate::lock_or_recover(&self.running);
        running.retain(|g| g.pid != pid);
        prune_with(&mut running, RunningGame::is_alive);
        running.len()
    }

    /// How many games are actually running now.
    pub fn running_count(&self) -> usize {
        let mut running = crate::lock_or_recover(&self.running);
        prune_with(&mut running, RunningGame::is_alive);
        running.len()
    }

    /// Is a game being prepared or running?
    pub fn is_active(&self) -> bool {
        self.is_preparing() || self.running_count() > 0
    }

    /// Stops the preparation (downloads). Does nothing once the game itself is running.
    pub fn cancel(&self) {
        if let Some(c) = crate::lock_or_recover(&self.preparing).as_ref() {
            c.notify_one();
        }
    }

    /// The lock held while a token is read and written: the game renews one before launch, and
    /// `login`/`logout`/`select_account` must not race that renewal (P2-35).
    pub async fn lock_tokens(&self) -> tokio::sync::MutexGuard<'_, ()> {
        self.token_refresh_lock.lock().await
    }
}

/// Takes the preparation slot for work that must not overlap a launch (cleaning the storage):
/// while the returned guard lives, `start` answers "busy". `None` if a game is being prepared or
/// running.
pub fn hold_slot(game: &Arc<GameState>) -> Option<PrepareRelease> {
    game.begin_prepare().ok()?;
    let release = PrepareRelease(game.clone());
    (game.running_count() == 0).then_some(release)
}

/// Frees the preparation slot when the preparation ends, however it ends.
pub struct PrepareRelease(Arc<GameState>);

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
    /// See `Settings::discord_rpc_mode`.
    pub discord_rpc_mode: String,
    /// The interface language's code, for the launcher's own presence.
    pub language: String,
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
    /// Hardware ID the client patch should report instead of the machine's own.
    pub hwid: Option<bebra_core::hwid::HwidOverride>,
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
            // A panic anywhere below would leave the preparation slot taken (the release guard
            // drops during unwinding, so that part is fine) and the UI waiting forever. Say
            // that the launch failed instead (P2-38).
            let result = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_play(&app, &game_clone, &params, cancel);
            }));
            if result.is_err() {
                crate::applog::line("play: the launch thread panicked");
                crate::pending::emit(
                    &app,
                    EVENT,
                    PlayEvent::failed(
                        "other",
                        "РІРЅСѓС‚СЂРµРЅРЅСЏСЏ РѕС€РёР±РєР° РїСЂРё Р·Р°РїСѓСЃРєРµ РёРіСЂС‹",
                    ),
                );
            }
        });
    spawned.map(drop).map_err(|e| {
        game.end_prepare();
        format!("РЅРµ СѓРґР°Р»РѕСЃСЊ Р·Р°РїСѓСЃС‚РёС‚СЊ РїРѕС‚РѕРє: {e}")
    })
}

/// The body of the launch thread: prepare, then run (or report what would be run).
fn run_play(
    app: &AppHandle,
    game_clone: &Arc<GameState>,
    params: &PlayParams,
    cancel: Arc<Notify>,
) {
    let release = PrepareRelease(game_clone.clone());

    let emit = |event: PlayEvent| {
        // Kept when the page is not listening yet (cold start with `--commands`).
        crate::pending::emit(app, EVENT, event);
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
            r = prepare(app, game_clone, params) => r.map_err(Some),
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
            // A panic while starting or watching the game must also be reported.
            let run = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                run_game(app, &emit, params, game_clone, prepared_game);
            }));
            if run.is_err() {
                crate::applog::line("play: run_game panicked");
                emit(PlayEvent::failed(
                    "other",
                    "РІРЅСѓС‚СЂРµРЅРЅСЏСЏ РѕС€РёР±РєР° РїСЂРё Р·Р°РїСѓСЃРєРµ РёРіСЂС‹",
                ));
            }
        }
        Err(None) => emit(PlayEvent::Cancelled),
        Err(Some(failure)) => emit(PlayEvent::failed(failure.code, failure.message)),
    }
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
    // Both ways: the game saves this cvar in its own config.
    extra_args.push("--cvar".to_string());
    extra_args.push(format!(
        "discord.enabled={}",
        game_keeps_presence(&params.discord_rpc_mode, params.close_on_play)
    ));
    // The engine only knows two modes (`WindowMode`: 0 = windowed, 1 = fullscreen); there is no
    // borderless mode, so "borderless" gets fullscreen, which SDL3 creates as a desktop-fullscreen
    // window. The names used to be `display.window_mode`, `display.ui_scale` and
    // `audio.mute_when_unfocused` — none of them exist, so the engine silently ignored the settings.
    match params.client_window_mode.as_str() {
        "windowed" => {
            extra_args.push("--cvar".to_string());
            extra_args.push("display.windowmode=0".to_string());
        }
        "fullscreen" | "borderless" => {
            extra_args.push("--cvar".to_string());
            extra_args.push("display.windowmode=1".to_string());
        }
        _ => {}
    }
    // Both ways: the option is the launcher's, so "off" must turn muting off even when the
    // game's own config has it on.
    extra_args.push("--cvar".to_string());
    extra_args.push(format!(
        "audio.mute_unfocused={}",
        params.client_mute_unfocused
    ));
    if params.client_ui_scale != "auto" && !params.client_ui_scale.is_empty() {
        extra_args.push("--cvar".to_string());
        extra_args.push(format!("display.uiScale={}", params.client_ui_scale));
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
        hwid: params.hwid.clone(),
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
            Some(Progress {
                done,
                total,
                bytes,
                unit,
            }) => {
                let permille = (done * 1000).checked_div(total).unwrap_or(0);
                if last.swap(permille, Ordering::Relaxed) != permille {
                    emit(PlayEvent::Progress {
                        stage: name,
                        done,
                        total,
                        bytes,
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

/// Does the game client keep the Discord presence? A launcher that closes after the start cannot
/// keep it, so the game does then: its presence is better than none.
fn game_keeps_presence(mode: &str, close_on_play: bool) -> bool {
    match mode {
        "off" => false,
        "launcher" => close_on_play,
        _ => true,
    }
}

/// Does the launcher keep the Discord presence? Exactly one of the two does, unless it is off.
pub fn launcher_keeps_presence(mode: &str, close_on_play: bool) -> bool {
    mode == "launcher" && !close_on_play
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
    let mut settings = crate::lock_or_recover(&state.settings);
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
    let mut settings = crate::lock_or_recover(&state.settings);
    settings.add_play_time(key, seconds);
    let _ = settings.save(state.dirs.root());
}

static SESSION_COUNTER: AtomicU64 = AtomicU64::new(1);

/// The log files for one game: always a name of its own.
///
/// A fixed name (`client.stdout.log`) is shared by every game, so a second game started while
/// the first was still closing overwrote its log and the tail shown for a crash belonged to
/// somebody else (P2-40).
fn client_log_paths(dirs: &DataDirs, session_id: u64, started_at: u64) -> (PathBuf, PathBuf) {
    (
        dirs.logs()
            .join(format!("client-{started_at}-{session_id}.stdout.log")),
        dirs.logs()
            .join(format!("client-{started_at}-{session_id}.stderr.log")),
    )
}

fn run_game(
    app: &AppHandle,
    emit: &impl Fn(PlayEvent),
    params: &PlayParams,
    game: &Arc<GameState>,
    mut prepared_game: play::PreparedGame,
) {
    let dirs = &params.dirs;
    emit(PlayEvent::Stage { stage: "starting" });

    let session_id = SESSION_COUNTER.fetch_add(1, Ordering::Relaxed);
    let started_at_secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    let (out_log, err_log) = client_log_paths(dirs, session_id, started_at_secs);
    let keeps_presence = launcher_keeps_presence(&params.discord_rpc_mode, params.close_on_play);
    // Next to the logs, which `launch::spawn` creates; of its own for the same reason they are.
    let client_file = keeps_presence.then(|| {
        dirs.logs()
            .join(format!("client-{started_at_secs}-{session_id}.presence"))
    });
    if let Some(path) = &client_file {
        prepared_game.command.env.push((
            launch::PRESENCE_FILE_ENV.to_string(),
            path.display().to_string(),
        ));
    }

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
        started_at_secs,
    };
    app.state::<crate::AppState>()
        .sessions
        .record_start(dirs.root(), session);

    emit(PlayEvent::Running {
        pid,
        plays_as: prepared_game.plays_as.clone(),
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
    let presence = keeps_presence
        .then(|| {
            crate::presence::start(crate::presence::PresenceRequest {
                client: params.client.clone(),
                addr: params.addr.clone(),
                server_name: params.server_name.clone(),
                language: params.language.clone(),
                username: prepared_game.plays_as.clone(),
                client_file: client_file.clone(),
            })
        })
        .flatten();
    bebra_core::process::trim_working_set();

    let started = std::time::Instant::now();
    let code = child.wait().ok().and_then(|s| s.code());
    drop(presence);
    if let Some(path) = &client_file {
        let _ = std::fs::remove_file(path);
    }
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
        // Only the end of the file is needed: read it backwards, not the whole log (P2-40).
        crate::applog::tail(&out_log, 15)
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
    fn exactly_one_side_keeps_the_presence_unless_it_is_off() {
        for close in [false, true] {
            assert!(!game_keeps_presence("off", close));
            assert!(!launcher_keeps_presence("off", close));
            assert!(game_keeps_presence("vanilla", close));
            assert!(!launcher_keeps_presence("vanilla", close));
        }
        assert!(launcher_keeps_presence("launcher", false));
        assert!(!game_keeps_presence("launcher", false));
        assert!(
            game_keeps_presence("launcher", true),
            "a launcher that closes hands the presence to the game"
        );
        assert!(!launcher_keeps_presence("launcher", true));
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

    /// Every game writes a log of its own, whatever else is running (P2-40).
    #[test]
    fn every_game_gets_its_own_log_files() {
        let dirs = DataDirs::at(std::env::temp_dir());
        let (out1, err1) = client_log_paths(&dirs, 1, 1_700_000_000);
        let (out2, err2) = client_log_paths(&dirs, 2, 1_700_000_000);
        let (out3, err3) = client_log_paths(&dirs, 1, 1_700_000_001);
        let all = [&out1, &err1, &out2, &err2, &out3, &err3];
        let unique: std::collections::HashSet<_> = all.iter().collect();
        assert_eq!(unique.len(), all.len(), "no two games share a log file");
        assert!(out1.to_string_lossy().contains("client-1700000000-1"));
        assert!(err1.to_string_lossy().ends_with("stderr.log"));
    }
}
