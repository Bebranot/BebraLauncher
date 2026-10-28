//! "Play": everything between picking a server and having a command to start the game.
//! Shared by the CLI and the app (docs/protocol.md, sections 2, 6 and 8).

use std::path::PathBuf;

use crate::accounts::StoredAccount;
use crate::addr::ServerAddr;
use crate::auth::AuthClient;
use crate::content_db::cull::CullReport;
use crate::content_db::ContentDb;
use crate::download::{Progress, ProgressFn};
use crate::http::Client;
use crate::hub::HubClient;
use crate::launch::{self, AuthEnv, ConnectTarget, LaunchCommand, LaunchSpec};
use crate::paths::DataDirs;
use crate::server::{self, AuthMode, ServerInfo};
use crate::updater::{self, DownloadMode, Sources};
use crate::Error;

/// The steps `prepare` goes through, in order.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Stage {
    /// Asking the server (or the hub) about its build.
    Info,
    /// Game files of the server's build.
    Files,
    /// Engine and engine modules.
    Engine,
}

/// Why preparing the game failed. The variant tells the UI what to say; the text inside is the
/// technical detail.
#[derive(Debug, thiserror::Error)]
pub enum PlayError {
    #[error("{url} did not answer: {source}")]
    InfoUnreachable { url: String, source: Error },
    #[error("the server gave no build information")]
    NoBuild,
    #[error("a Wizards' Den account is required to join this server")]
    AuthRequired,
    #[error("not enough free disk space to install the game")]
    DiskFull,
    #[error("failed to install game files: {0}")]
    Files(Error),
    #[error("failed to install the engine: {0}")]
    Engine(Error),
    #[error(transparent)]
    Other(#[from] Error),
}

impl PlayError {
    /// Stable name for the UI to pick a translated message by.
    pub fn code(&self) -> &'static str {
        match self {
            Self::InfoUnreachable { .. } => "info_unreachable",
            Self::NoBuild => "no_build",
            Self::AuthRequired => "auth_required",
            Self::DiskFull => "disk_full",
            Self::Files(_) => "files",
            Self::Engine(_) => "engine",
            Self::Other(_) => "other",
        }
    }

    /// The technical detail without the headline.
    pub fn detail(&self) -> String {
        match self {
            Self::InfoUnreachable { url, source } => format!("{url}: {source}"),
            Self::NoBuild | Self::AuthRequired | Self::DiskFull => String::new(),
            Self::Files(e) | Self::Engine(e) | Self::Other(e) => e.to_string(),
        }
    }
}

/// Called with the current stage; the progress is `None` when a step has no measurable progress.
pub type StageFn<'a> = &'a (dyn Fn(Stage, Option<Progress>) + Send + Sync);

#[derive(Debug, Clone)]
pub struct PlayRequest {
    pub addr: ServerAddr,
    /// Ask the hub for the server's `/info` instead of the server itself.
    pub via_hub: bool,
    /// The account to play with, already checked or renewed. `None` = guest.
    pub account: Option<StoredAccount>,
    /// The name used without an account.
    pub guest_name: String,
    /// GLES2 fallback renderer.
    pub compat_mode: bool,
    /// Start the client's main menu instead of connecting.
    pub no_connect: bool,
    pub loader_exe: PathBuf,
    /// Let the game ask this launcher for a reconnect (see `LaunchSpec::launcher_path`).
    pub launcher_path: Option<PathBuf>,
    /// Overlay zip of text packs to mount over the game's files.
    pub overlay_zip: Option<PathBuf>,
    /// Additional arguments passed to the engine.
    pub extra_args: Vec<String>,
    /// Client patch assemblies (.dll) to load.
    pub patches: Vec<PathBuf>,
    /// Client performance and optimization settings.
    pub optimization: launch::OptimizationConfig,
}

pub struct PreparedGame {
    pub command: LaunchCommand,
    pub info: ServerInfo,
    /// What the server said about accounts, and what we are going to send it.
    pub plays_as: String,
    pub sends_account: bool,
    pub mode: DownloadMode,
    pub cleanup: CullReport,
}

/// Downloads what is missing and builds the command that starts the game.
///
/// The content database is not `Sync`, so the returned future is not `Send`: run it on a thread
/// of its own (a current-thread runtime), not on a shared executor.
pub async fn prepare(
    client: &Client,
    dirs: &DataDirs,
    req: &PlayRequest,
    on_stage: StageFn<'_>,
) -> std::result::Result<PreparedGame, PlayError> {
    dirs.ensure()?;

    on_stage(Stage::Info, None);
    let info = if req.via_hub {
        HubClient::official(client.clone())
            .server_info(req.addr.as_str())
            .await?
    } else {
        server::fetch_info(client, &req.addr)
            .await
            .map_err(|source| PlayError::InfoUnreachable {
                url: req.addr.info_url(),
                source,
            })?
    };

    // Preflight checks: verify auth requirement and free disk space
    // before starting heavy content downloads.
    if info.auth.mode == AuthMode::Required && req.account.is_none() {
        return Err(PlayError::AuthRequired);
    }
    if let Some(free_bytes) = free_disk_space_bytes(dirs.root()) {
        if free_bytes < 500 * 1024 * 1024 {
            return Err(PlayError::DiskFull);
        }
    }
    let mut build = info.build.clone().ok_or(PlayError::NoBuild)?;
    if req.via_hub {
        build.apply_self_hosting(&req.addr);
    }

    let engine_type = build.engine_type();
    let sources = Sources::for_engine(engine_type)?;

    // Optimization: prefetch the engine in parallel with content download
    // if the required engine version is not installed yet.
    let engine_version = build.engine_version.clone();
    let engine_needed = crate::engine::installed(dirs, &engine_version)
        .ok()
        .flatten()
        .is_none();

    let prefetch_task = if engine_needed {
        let engine_client = client.clone();
        let engine_dirs = dirs.clone();
        let engine_sources = sources.clone();
        Some(tokio::spawn(async move {
            crate::engine::install_engine(
                &engine_client,
                &engine_sources.builds,
                &engine_dirs,
                &engine_version,
                &engine_sources.key,
                None,
            )
            .await
        }))
    } else {
        None
    };

    struct AbortOnDrop<T>(Option<tokio::task::JoinHandle<T>>);
    impl<T> Drop for AbortOnDrop<T> {
        fn drop(&mut self) {
            if let Some(h) = self.0.take() {
                h.abort();
            }
        }
    }

    let mut prefetch_guard = AbortOnDrop(prefetch_task);

    let db = ContentDb::open(&dirs.content_db())?;
    let files = |p: Progress| on_stage(Stage::Files, Some(p));
    on_stage(Stage::Files, None);
    let content = updater::install_content(client, &db, &build, &dirs.scratch(), Some(&files))
        .await
        .map_err(PlayError::Files)?;
    let cleanup = updater::cleanup(&db)?;

    if let Some(task) = prefetch_guard.0.take() {
        let _ = task.await;
    }

    let engine = |p: Progress| on_stage(Stage::Engine, Some(p));
    on_stage(Stage::Engine, None);
    let engine_progress: ProgressFn<'_> = &engine;
    let prepared = updater::prepare_launch(
        client,
        &db,
        dirs,
        &sources,
        content.version_id,
        Some(engine_progress),
    )
    .await
    .map_err(PlayError::Engine)?;

    let auth_client = AuthClient::official(client.clone());
    let default_auth_url = auth_client.most_successful_url().to_string();
    let effective_auth_server = if let Some(urls) = &info.auth.login_urls {
        urls.first().cloned().unwrap_or(default_auth_url)
    } else {
        default_auth_url
    };
    let auth = req
        .account
        .as_ref()
        .filter(|_| !req.no_connect && info.auth.mode != AuthMode::Disabled)
        .map(|a| AuthEnv {
            token: a.token.expose().to_string(),
            user_id: a.user_id.to_string(),
            server_public_key: info.auth.public_key.clone(),
            auth_server_url: effective_auth_server,
        });
    let sends_account = auth.is_some();
    let plays_as = req
        .account
        .as_ref()
        .map_or_else(|| req.guest_name.clone(), |a| a.username.clone());

    let spec = LaunchSpec {
        loader_exe: req.loader_exe.clone(),
        dotnet_root: None,
        engine: prepared.engine,
        signing_key_file: launch::ensure_signing_key_file_for_engine(dirs, engine_type)?,
        content_db: dirs.content_db(),
        content_version_id: content.version_id,
        modules: prepared
            .modules
            .into_iter()
            .map(|m| (m.name, m.dir))
            .collect(),
        username: plays_as.clone(),
        auth,
        build,
        connect: (!req.no_connect).then(|| ConnectTarget {
            connect_url: info.connect_url(&req.addr),
            server_address: req.addr.normalized(),
        }),
        compat_mode: req.compat_mode,
        launcher_path: req.launcher_path.clone(),
        overlay_zip: req.overlay_zip.clone(),
        extra_args: req.extra_args.clone(),
        patches: req.patches.clone(),
        optimization: req.optimization.clone(),
    };

    Ok(PreparedGame {
        command: launch::build_command(&spec),
        info,
        plays_as,
        sends_account,
        mode: content.mode,
        cleanup,
    })
}

#[cfg(windows)]
fn free_disk_space_bytes(path: &std::path::Path) -> Option<u64> {
    use std::os::windows::ffi::OsStrExt;
    use windows_sys::Win32::Storage::FileSystem::GetDiskFreeSpaceExW;

    let mut path_u16: Vec<u16> = path.as_os_str().encode_wide().collect();
    path_u16.push(0);

    let mut free_bytes_available: u64 = 0;
    let mut total_bytes: u64 = 0;
    let mut total_free_bytes: u64 = 0;

    unsafe {
        let ok = GetDiskFreeSpaceExW(
            path_u16.as_ptr(),
            &mut free_bytes_available,
            &mut total_bytes,
            &mut total_free_bytes,
        );
        if ok != 0 {
            Some(free_bytes_available)
        } else {
            None
        }
    }
}

#[cfg(not(windows))]
fn free_disk_space_bytes(_path: &std::path::Path) -> Option<u64> {
    None
}
