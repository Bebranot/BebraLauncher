//! Updating the launcher: state shared by the commands, the background check and the install
//! on exit. The work itself (GitHub, signatures, the installer) is `bebra_core::self_update`.

use std::path::PathBuf;
use std::sync::Mutex;

use bebra_core::accounts;
use bebra_core::secret::Token;
use bebra_core::self_update::{self, Release, Version};
use serde::Serialize;
use tauri::{AppHandle, Emitter, Manager};

use crate::applog;
use crate::AppState;

/// The event the UI listens to; its payload is a [`Status`].
pub const EVENT: &str = "update";

/// Where the GitHub access token (for a private repository) lives.
const TOKEN_TARGET: &str = "BebraLauncher/github-token";

/// How often the background check repeats while the launcher stays open.
const RECHECK: std::time::Duration = std::time::Duration::from_secs(4 * 60 * 60);

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "lowercase")]
pub enum Phase {
    Idle,
    Checking,
    Downloading,
    Ready,
}

#[derive(Default)]
struct Inner {
    phase: Option<Phase>,
    release: Option<Release>,
    ready: Option<PathBuf>,
    done: u64,
    total: u64,
    error: Option<String>,
    /// The installer has been started: nothing else may start it.
    installing: bool,
}

#[derive(Default)]
pub struct UpdateState {
    inner: Mutex<Inner>,
}

/// What the UI sees.
#[derive(Debug, Clone, Serialize)]
pub struct Status {
    phase: Phase,
    /// This launcher, as `a.b.cdd`.
    current: String,
    latest: Option<ReleaseRow>,
    done: u64,
    total: u64,
    error: Option<String>,
    /// Installing is possible now: a downloaded installer, a release build, no game running.
    can_install: bool,
}

#[derive(Debug, Clone, Serialize)]
pub struct ReleaseRow {
    version: String,
    notes: String,
    published: Option<String>,
    page_url: String,
    size: u64,
}

impl From<&Release> for ReleaseRow {
    fn from(r: &Release) -> Self {
        Self {
            version: r.version.to_string(),
            notes: r.notes.clone(),
            published: r.published.clone(),
            page_url: r.page_url.clone(),
            size: r.setup.size,
        }
    }
}

impl UpdateState {
    fn status(&self, playing: bool) -> Status {
        let i = crate::lock_or_recover(&self.inner);
        let phase = i.phase.unwrap_or(Phase::Idle);
        Status {
            phase,
            current: Version::current().to_string(),
            latest: i.release.as_ref().map(ReleaseRow::from),
            done: i.done,
            total: i.total,
            error: i.error.clone(),
            can_install: phase == Phase::Ready && !playing && !cfg!(debug_assertions),
        }
    }

    fn edit(&self, f: impl FnOnce(&mut Inner)) {
        f(&mut crate::lock_or_recover(&self.inner));
    }

    /// Claims the update for a check: unless it is idle, the caller gets no claim.
    ///
    /// Phase and claim are set in one critical section, so a click and the background check
    /// cannot both start a request to GitHub (P2-34).
    fn begin_check(&self) -> bool {
        let mut i = crate::lock_or_recover(&self.inner);
        if i.phase.unwrap_or(Phase::Idle) != Phase::Idle {
            return false;
        }
        i.phase = Some(Phase::Checking);
        i.error = None;
        true
    }
}

fn status_of(app: &AppHandle) -> Status {
    let state = app.state::<AppState>();
    state.update.status(state.game.is_active())
}

fn emit(app: &AppHandle) {
    let _ = app.emit(EVENT, status_of(app));
}

pub fn token() -> Option<Token> {
    accounts::read_secret(TOKEN_TARGET)
        .ok()
        .flatten()
        .filter(|t| !t.trim().is_empty())
        .map(|t| Token::new(t.trim()))
}

pub fn token_present() -> bool {
    token().is_some()
}

/// Saves the token typed by the user; an empty one removes it.
pub fn set_token(value: &str) -> Result<(), String> {
    let value = value.trim();
    if value.is_empty() {
        return accounts::delete_secret(TOKEN_TARGET).map_err(|e| e.to_string());
    }
    // A token is a short run of plain characters; anything else is not one.
    if value.len() > 400 || !value.bytes().all(|b| b.is_ascii_graphic()) {
        return Err("это не похоже на токен GitHub".into());
    }
    accounts::store_secret(TOKEN_TARGET, value).map_err(|e| e.to_string())
}

fn updates_dir(app: &AppHandle) -> PathBuf {
    app.state::<AppState>().dirs.root().join("updates")
}

/// Asks GitHub for the newest release. With "automatic" on, a newer one is downloaded at once.
pub async fn check(app: &AppHandle) -> Status {
    let state = app.state::<AppState>();
    if !state.update.begin_check() {
        // Somebody is already checking, downloading or has something ready.
        return status_of(app);
    }
    emit(app);

    let (repo, auto) = {
        let s = crate::lock_or_recover(&state.settings);
        (s.update_repo.clone(), s.update_auto)
    };
    let result = self_update::latest(&state.client, &repo, token().as_ref()).await;
    let mut start_download = false;
    match result {
        Ok(Some(r)) if r.version > Version::current() => {
            applog::line(&format!("update: {} is available", r.version));
            start_download = auto;
            state.update.edit(|i| {
                i.phase = Some(Phase::Idle);
                i.release = Some(r);
            });
        }
        Ok(_) => state.update.edit(|i| {
            i.phase = Some(Phase::Idle);
            i.release = None;
        }),
        Err(e) => {
            applog::line(&format!("update: check failed: {e}"));
            state.update.edit(|i| {
                i.phase = Some(Phase::Idle);
                i.error = Some(e.to_string());
            });
        }
    }
    emit(app);
    if start_download {
        download(app.clone());
    }
    status_of(app)
}

/// Starts downloading the release found by the last check (in the background).
pub fn download(app: AppHandle) {
    let state = app.state::<AppState>();
    let release = {
        let mut i = crate::lock_or_recover(&state.update.inner);
        let phase = i.phase.unwrap_or(Phase::Idle);
        let Some(release) = i.release.clone() else {
            return;
        };
        if phase != Phase::Idle {
            return;
        }
        i.phase = Some(Phase::Downloading);
        i.done = 0;
        i.total = release.setup.size;
        i.error = None;
        release
    };
    emit(&app);

    tauri::async_runtime::spawn(async move {
        let state = app.state::<AppState>();
        let dir = updates_dir(&app);
        let key = match self_update::update_key() {
            Ok(k) => k,
            Err(e) => return failed(&app, e.to_string()),
        };
        let progress_app = app.clone();
        let mut last_emit = std::time::Instant::now();
        let result = self_update::download(
            &state.client,
            &release,
            token().as_ref(),
            &dir,
            &key,
            move |done, total| {
                progress_app.state::<AppState>().update.edit(|i| {
                    i.done = done;
                    i.total = total;
                });
                // About ten updates a second are plenty for a progress bar.
                if last_emit.elapsed() > std::time::Duration::from_millis(100) || done == total {
                    last_emit = std::time::Instant::now();
                    emit(&progress_app);
                }
            },
        )
        .await;
        match result {
            Ok(path) => {
                applog::line(&format!(
                    "update: {} downloaded and verified",
                    release.version
                ));
                // Older installers are of no use now.
                self_update::clean_old(&dir, Some(&path));
                state.update.edit(|i| {
                    i.phase = Some(Phase::Ready);
                    i.ready = Some(path);
                });
                emit(&app);
            }
            Err(e) => failed(&app, e.to_string()),
        }
    });
}

fn failed(app: &AppHandle, message: String) {
    applog::line(&format!("update: download failed: {message}"));
    app.state::<AppState>().update.edit(|i| {
        i.phase = Some(Phase::Idle);
        i.error = Some(message);
    });
    emit(app);
}

/// Runs the downloaded installer now and exits; with `restart`, the launcher starts again.
pub fn install_now(app: &AppHandle, restart: bool) -> Result<(), String> {
    if cfg!(debug_assertions) {
        return Err("в сборке для разработки установка обновлений отключена".into());
    }
    let state = app.state::<AppState>();
    if state.game.is_active() {
        return Err("идёт подготовка или работа игры: закройте её и повторите".into());
    }
    let path = {
        let mut i = crate::lock_or_recover(&state.update.inner);
        let (Some(Phase::Ready), Some(path)) = (i.phase, i.ready.clone()) else {
            return Err("обновление ещё не скачано".into());
        };
        if i.installing {
            return Ok(());
        }
        i.installing = true;
        path
    };
    if let Err(e) = self_update::spawn_installer(&path, restart) {
        state.update.edit(|i| i.installing = false);
        return Err(e.to_string());
    }
    applog::line("update: installer started, exiting");
    app.exit(0);
    Ok(())
}

/// Called when the launcher closes: a downloaded update is installed quietly (no restart),
/// if the user wants updates automatic and no game is still being run by the launcher.
pub fn install_on_exit(app: &AppHandle) {
    if cfg!(debug_assertions) {
        return;
    }
    let state = app.state::<AppState>();
    let auto = crate::lock_or_recover(&state.settings).update_auto;
    if !auto || state.game.is_active() {
        return;
    }
    let path = {
        let mut i = crate::lock_or_recover(&state.update.inner);
        let (Some(Phase::Ready), Some(path)) = (i.phase, i.ready.clone()) else {
            return;
        };
        if i.installing {
            return;
        }
        i.installing = true;
        path
    };
    match self_update::spawn_installer(&path, false) {
        Ok(()) => applog::line("update: installer started on exit"),
        Err(e) => applog::line(&format!("update: installer not started: {e}")),
    }
}

/// The background check: shortly after start, then every few hours while the launcher is open.
pub fn start_background_checks(app: AppHandle) {
    tauri::async_runtime::spawn(async move {
        tokio::time::sleep(std::time::Duration::from_secs(8)).await;
        loop {
            let on = crate::lock_or_recover(&app.state::<AppState>().settings).update_check;
            if on {
                check(&app).await;
            }
            tokio::time::sleep(RECHECK).await;
        }
    });
}

/// The state for the UI.
pub fn status(app: &AppHandle) -> Status {
    status_of(app)
}
