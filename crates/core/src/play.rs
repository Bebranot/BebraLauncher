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
use crate::{Error, Result};

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
) -> Result<PreparedGame> {
    dirs.ensure()?;

    on_stage(Stage::Info, None);
    let info = if req.via_hub {
        HubClient::official(client.clone())
            .server_info(req.addr.as_str())
            .await?
    } else {
        server::fetch_info(client, &req.addr).await.map_err(|e| {
            Error::Other(format!("{} did not answer /info: {e}", req.addr.info_url()))
        })?
    };
    let mut build = info
        .build
        .clone()
        .ok_or_else(|| Error::Other("the server gave no build information".into()))?;
    if req.via_hub {
        build.apply_self_hosting(&req.addr);
    }

    let db = ContentDb::open(&dirs.content_db())?;
    let files = |p: Progress| on_stage(Stage::Files, Some(p));
    on_stage(Stage::Files, None);
    let content = updater::install_content(client, &db, &build, &dirs.scratch(), Some(&files))
        .await
        .map_err(|e| Error::Other(format!("failed to install game files: {e}")))?;
    let cleanup = updater::cleanup(&db)?;

    let engine = |p: Progress| on_stage(Stage::Engine, Some(p));
    on_stage(Stage::Engine, None);
    let engine_progress: ProgressFn<'_> = &engine;
    let prepared = updater::prepare_launch(
        client,
        &db,
        dirs,
        &Sources::official()?,
        content.version_id,
        Some(engine_progress),
    )
    .await
    .map_err(|e| Error::Other(format!("failed to install the engine: {e}")))?;

    let auth_client = AuthClient::official(client.clone());
    let auth = req
        .account
        .as_ref()
        .filter(|_| !req.no_connect && info.auth.mode != AuthMode::Disabled)
        .map(|a| AuthEnv {
            token: a.token.expose().to_string(),
            user_id: a.user_id.to_string(),
            server_public_key: info.auth.public_key.clone(),
            auth_server_url: auth_client.most_successful_url().to_string(),
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
        signing_key_file: launch::ensure_signing_key_file(dirs)?,
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
