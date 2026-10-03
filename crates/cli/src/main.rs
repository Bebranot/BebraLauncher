use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use bebra_core::accounts::{
    self, AccountStatus, CredentialStore, Freshness, LoginResult, StoredAccount, TokenStore,
};
use bebra_core::auth::{AuthClient, DenyCode, Identity};
use bebra_core::content_db::cull::CullReport;
use bebra_core::content_db::ContentDb;
use bebra_core::download::{Progress, ProgressUnit};
use bebra_core::hub::HubClient;
use bebra_core::paths::DataDirs;
use bebra_core::server::AuthMode;
use bebra_core::server::{self, ServerStatus};
use bebra_core::{http, launch, play, storage, updater, ServerAddr};
use clap::{Parser, Subcommand};
use time::OffsetDateTime;
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "bebra", about = "Debug CLI for bebra-core")]
struct Cli {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    /// List servers from the official hub, busiest first.
    Servers {
        /// Case-insensitive filter over name, address and tags.
        #[arg(short, long)]
        search: Option<String>,
        /// Show at most this many servers.
        #[arg(short, long, default_value_t = 30)]
        limit: usize,
        /// Print raw JSON instead of a table.
        #[arg(long)]
        json: bool,
    },
    /// Ask a game server for /status.
    Status { address: String },
    /// Ask a game server for /info (directly, or through the hub with --via-hub).
    Info {
        address: String,
        #[arg(long)]
        via_hub: bool,
    },
    /// Download a server's game files and engine into the local data folder.
    Update {
        address: String,
        /// Where to keep data (default: %LOCALAPPDATA%\BebraLauncher).
        #[arg(long)]
        data_dir: Option<PathBuf>,
        /// Ask the hub for /info instead of the server (for servers unreachable from here).
        #[arg(long)]
        via_hub: bool,
    },
    /// Log in to a Wizards' Den account. The password is asked for here and never stored.
    Login {
        /// Account name (asked for if left out).
        #[arg(long, conflicts_with = "user_id")]
        username: Option<String>,
        /// Log in again to a saved account, by user id.
        #[arg(long)]
        user_id: Option<Uuid>,
    },
    /// List saved accounts.
    Accounts {
        /// Also ask the server whether each login is still valid.
        #[arg(long)]
        check: bool,
    },
    /// Log out and forget a saved account (name or user id).
    Logout { account: String },
    /// Show which stored versions are used by a running game client.
    Running {
        #[arg(long)]
        data_dir: Option<PathBuf>,
    },
    /// Show how much disk space the launcher uses.
    Storage {
        #[arg(long)]
        data_dir: Option<PathBuf>,
    },
    /// Remove old game versions, unused engines and modules, and unfinished downloads.
    Clean {
        #[arg(long)]
        data_dir: Option<PathBuf>,
    },
    /// Install (if needed) and start the game for a server.
    Launch {
        address: String,
        #[arg(long)]
        data_dir: Option<PathBuf>,
        #[arg(long)]
        via_hub: bool,
        /// Path to SS14.Loader.exe (default: found next to this program or in the repo's loader/out).
        #[arg(long)]
        loader: Option<PathBuf>,
        /// Name to play under without an account.
        #[arg(long, default_value = launch::FALLBACK_USERNAME)]
        username: String,
        /// Account to play with (name or user id). Default: the only saved account, if there is one.
        #[arg(long, conflicts_with = "guest")]
        account: Option<String>,
        /// Play as a guest even if an account is saved.
        #[arg(long)]
        guest: bool,
        /// Use the GLES2 fallback renderer.
        #[arg(long)]
        compat: bool,
        /// Start the client at its main menu without connecting to the server.
        #[arg(long)]
        no_connect: bool,
        /// Only print the command that would run (secrets hidden).
        #[arg(long)]
        dry_run: bool,
    },
}

#[tokio::main]
async fn main() -> Result<()> {
    let cli = Cli::parse();
    let client = http::build_client()?;

    match cli.command {
        Command::Servers {
            search,
            limit,
            json,
        } => {
            let mut list = HubClient::official(client)
                .list_servers()
                .await
                .context("failed to get the server list")?;
            let total = list.len();

            if let Some(q) = search.as_deref().map(str::to_lowercase) {
                list.retain(|e| {
                    let hay = format!(
                        "{} {} {}",
                        e.status.name.as_deref().unwrap_or(""),
                        e.address,
                        e.status.tags.join(" ")
                    )
                    .to_lowercase();
                    hay.contains(&q)
                });
            }
            list.sort_by_key(|e| std::cmp::Reverse(e.status.players));
            let shown = list.len().min(limit);

            if json {
                println!("{}", serde_json::to_string_pretty(&list[..shown])?);
            } else {
                for e in &list[..shown] {
                    print_row(&e.address, &e.status);
                }
                eprintln!("{shown} of {} matching ({total} total)", list.len());
            }
        }
        Command::Status { address } => {
            let addr = ServerAddr::parse(&address)?;
            let status = server::fetch_status(&client, &addr)
                .await
                .with_context(|| format!("{} did not answer /status", addr.status_url()))?;
            println!("{}", serde_json::to_string_pretty(&status)?);
        }
        Command::Info { address, via_hub } => {
            let addr = ServerAddr::parse(&address)?;
            let info = if via_hub {
                HubClient::official(client)
                    .server_info(addr.as_str())
                    .await?
            } else {
                server::fetch_info(&client, &addr)
                    .await
                    .with_context(|| format!("{} did not answer /info", addr.info_url()))?
            };
            println!("{}", serde_json::to_string_pretty(&info)?);
        }
        Command::Update {
            address,
            data_dir,
            via_hub,
        } => {
            let addr = ServerAddr::parse(&address)?;
            let dirs = match data_dir {
                Some(p) => DataDirs::at(p),
                None => DataDirs::default_windows()?,
            };
            dirs.ensure()?;

            let info = if via_hub {
                HubClient::official(client.clone())
                    .server_info(addr.as_str())
                    .await?
            } else {
                server::fetch_info(&client, &addr)
                    .await
                    .with_context(|| format!("{} did not answer /info", addr.info_url()))?
            };
            let mut build = info.build.context("the server gave no build information")?;
            if via_hub {
                build.apply_self_hosting(&addr);
            }
            println!(
                "build: fork {}, version {}, engine {}, {:?}",
                build.fork_id,
                build.version,
                build.engine_version,
                updater::download_mode(&build)?
            );
            println!("data folder: {}", dirs.root().display());

            let db = ContentDb::open(&dirs.content_db())?;
            let printer = ProgressPrinter::new("files");
            let content = updater::install_content(
                &client,
                &db,
                &build,
                &dirs.scratch(),
                Some(&|p| printer.show(p)),
            )
            .await
            .context("failed to install game files")?;
            printer.finish();
            print_cleanup_report(&updater::cleanup(&db).context("cleanup of old versions failed")?);
            println!(
                "game files: version #{} - {} files, {} downloaded{}",
                content.version_id,
                content.files,
                content.downloaded,
                if content.reused {
                    " (already installed)"
                } else {
                    ""
                }
            );

            let printer = ProgressPrinter::new("engine");
            let prepared = updater::prepare_launch(
                &client,
                &db,
                &dirs,
                &updater::Sources::official()?,
                content.version_id,
                Some(&|p| printer.show(p)),
            )
            .await
            .context("failed to install the engine")?;
            printer.finish();
            let engine = &prepared.engine;
            println!(
                "engine {}: {} ({})",
                engine.version,
                engine.zip_path.display(),
                if engine.downloaded {
                    "downloaded, signature ok"
                } else {
                    "already installed"
                }
            );
            for m in &prepared.modules {
                println!(
                    "module {} {}: {} ({})",
                    m.name,
                    m.version,
                    m.dir.display(),
                    if m.downloaded {
                        "downloaded, signature ok"
                    } else {
                        "already installed"
                    }
                );
            }
        }
        Command::Login { username, user_id } => login_command(&client, username, user_id).await?,
        Command::Accounts { check } => accounts_command(&client, check).await?,
        Command::Logout { account } => logout_command(&client, &account).await?,
        Command::Storage { data_dir } => {
            let dirs = match data_dir {
                Some(p) => DataDirs::at(p),
                None => DataDirs::default_windows()?,
            };
            let r = storage::report(&dirs);
            println!("data folder: {}", dirs.root().display());
            for (name, bytes) in [
                ("games (content.db)", r.content_db),
                ("engines", r.engines),
                ("modules", r.modules),
                ("temporary", r.scratch),
                ("logs", r.logs),
                ("total", r.total()),
            ] {
                println!("{name:>20}  {}", human_size(bytes));
            }
        }
        Command::Clean { data_dir } => {
            let dirs = match data_dir {
                Some(p) => DataDirs::at(p),
                None => DataDirs::default_windows()?,
            };
            let db = ContentDb::open(&dirs.content_db())?;
            let r = storage::clean(&dirs, &db)?;
            println!(
                "removed {} old versions, {} engines, {} modules; freed {}",
                r.cull.versions_removed,
                r.engines_removed,
                r.modules_removed,
                human_size(r.bytes_freed)
            );
            if r.cull.versions_in_use > 0 {
                println!(
                    "{} old versions kept because the game is running",
                    r.cull.versions_in_use
                );
            }
        }
        Command::Running { data_dir } => {
            let dirs = match data_dir {
                Some(p) => DataDirs::at(p),
                None => DataDirs::default_windows()?,
            };
            let db = ContentDb::open(&dirs.content_db())?;
            let used = db.running_client_versions(bebra_core::process::is_process_running_from)?;
            if used.is_empty() {
                println!("no game client is running");
            }
            for id in used {
                let v = db.get_version(id)?;
                println!(
                    "version #{id} ({}) is in use",
                    v.and_then(|v| v.fork_version).unwrap_or_default()
                );
            }
        }
        Command::Launch {
            address,
            data_dir,
            via_hub,
            loader,
            username,
            account,
            guest,
            compat,
            no_connect,
            dry_run,
        } => {
            let addr = ServerAddr::parse(&address)?;
            let dirs = match data_dir {
                Some(p) => DataDirs::at(p),
                None => DataDirs::default_windows()?,
            };
            dirs.ensure()?;
            let loader_exe = match loader {
                Some(p) => p,
                None => launch::find_loader()?,
            };

            let auth_client = AuthClient::official(client.clone());
            let account = if no_connect || guest {
                None
            } else {
                pick_account(&auth_client, account.as_deref()).await?
            };
            let req = play::PlayRequest {
                addr,
                via_hub,
                account,
                guest_name: username,
                compat_mode: compat,
                no_connect,
                loader_exe,
                launcher_path: None,
            };
            let printer = ProgressPrinter::new("files");
            let engine_printer = ProgressPrinter::new("engine");
            let game = play::prepare(&client, &dirs, &req, &|stage, p| match (stage, p) {
                (play::Stage::Info, _) => {}
                (play::Stage::Files, Some(p)) => printer.show(p),
                (play::Stage::Engine, Some(p)) => engine_printer.show(p),
                _ => {}
            })
            .await?;
            printer.finish();
            engine_printer.finish();
            print_cleanup_report(&game.cleanup);
            if game.sends_account {
                println!("playing as {}", game.plays_as);
            } else {
                println!("playing as a guest ({})", game.plays_as);
                if !no_connect && game.info.auth.mode == AuthMode::Required {
                    eprintln!(
                        "note: this server requires an account; playing as a guest will be refused"
                    );
                }
            }
            let cmd = game.command;
            println!("{}", cmd.describe());
            if dry_run {
                return Ok(());
            }

            let (out_log, err_log) = (
                dirs.logs().join("client.stdout.log"),
                dirs.logs().join("client.stderr.log"),
            );
            let mut child = launch::spawn(&cmd, &out_log, &err_log)?;
            println!(
                "started (pid {}); client logs in {}",
                child.id(),
                dirs.logs().display()
            );

            let status = child.wait()?;
            println!("client exited: {status}");
            if !status.success() {
                let tail = std::fs::read_to_string(&out_log).unwrap_or_default();
                let tail: Vec<&str> = tail.lines().rev().take(15).collect();
                for line in tail.into_iter().rev() {
                    println!("  | {line}");
                }
                anyhow::bail!("the client did not exit cleanly");
            }
        }
    }
    Ok(())
}

fn read_line(prompt: &str) -> Result<String> {
    eprint!("{prompt}");
    std::io::Write::flush(&mut std::io::stderr())?;
    let mut line = String::new();
    std::io::stdin().read_line(&mut line)?;
    Ok(line.trim().to_string())
}

async fn login_command(
    client: &http::Client,
    username: Option<String>,
    user_id: Option<Uuid>,
) -> Result<()> {
    let auth = AuthClient::official(client.clone());
    let store = CredentialStore::default();

    // Who: a saved account by id, or a name typed here.
    let name;
    let who = match user_id {
        Some(id) => Identity::UserId(id),
        None => {
            name = match username {
                Some(n) => n,
                None => read_line("Account name: ")?,
            };
            if name.is_empty() {
                anyhow::bail!("no account name given");
            }
            Identity::Username(&name)
        }
    };

    let password = rpassword::prompt_password("Password (not shown, not saved): ")?;
    let mut tfa: Option<String> = None;
    for _ in 0..3 {
        match accounts::login(&auth, &store, who, &password, tfa.as_deref()).await? {
            LoginResult::Success(acc) => {
                println!(
                    "logged in as {} (login valid until {})",
                    acc.username,
                    acc.expire_time.date()
                );
                return Ok(());
            }
            LoginResult::Denied {
                code: DenyCode::TfaRequired | DenyCode::TfaInvalid,
                ..
            } => {
                tfa = Some(rpassword::prompt_password("Two-factor code: ")?);
            }
            LoginResult::Denied { errors, code } => {
                for e in &errors {
                    eprintln!("  {e}");
                }
                anyhow::bail!("login refused ({code:?})");
            }
        }
    }
    anyhow::bail!("login refused: the two-factor code was not accepted")
}

fn describe_freshness(acc: &StoredAccount, now: OffsetDateTime) -> &'static str {
    match accounts::freshness(acc.expire_time, now) {
        Freshness::Fine => "valid",
        Freshness::NeedsRefresh => "valid, will be renewed",
        Freshness::Expired => "expired: log in again",
    }
}

async fn accounts_command(client: &http::Client, check: bool) -> Result<()> {
    let store = CredentialStore::default();
    let list = store.list()?;
    if list.is_empty() {
        println!("no saved accounts (use `bebra login`)");
        return Ok(());
    }
    let auth = AuthClient::official(client.clone());
    let now = OffsetDateTime::now_utc();
    for acc in list {
        let status = if check {
            match accounts::ensure_fresh(&auth, &store, acc.clone(), now, true).await {
                Ok((_, AccountStatus::Available)) => "accepted by the server".to_string(),
                Ok((_, AccountStatus::Expired)) => "no longer accepted: log in again".to_string(),
                Err(e) => format!("could not check ({e})"),
            }
        } else {
            describe_freshness(&acc, now).to_string()
        };
        println!(
            "{}  {}  until {}  {status}",
            acc.username,
            acc.user_id,
            acc.expire_time.date()
        );
    }
    Ok(())
}

async fn logout_command(client: &http::Client, selector: &str) -> Result<()> {
    let store = CredentialStore::default();
    let list = store.list()?;
    let acc = accounts::find_account(&list, selector)?;
    let told = accounts::logout(&AuthClient::official(client.clone()), &store, acc).await?;
    println!(
        "{} forgotten{}",
        acc.username,
        if told {
            ""
        } else {
            " (the server could not be reached; the old login stays valid there until it expires)"
        }
    );
    Ok(())
}

/// The account to play with: the one asked for, or the only saved one. Its login is renewed or
/// checked first; an account that has to log in again is an error.
async fn pick_account(auth: &AuthClient, selector: Option<&str>) -> Result<Option<StoredAccount>> {
    let store = CredentialStore::default();
    let list = store.list()?;
    let chosen = match selector {
        Some(s) => accounts::find_account(&list, s)?.clone(),
        None => match list.as_slice() {
            [] => return Ok(None),
            [one] => one.clone(),
            _ => anyhow::bail!(
                "several accounts are saved: pick one with --account NAME, or use --guest"
            ),
        },
    };

    let now = OffsetDateTime::now_utc();
    match accounts::ensure_fresh(auth, &store, chosen.clone(), now, true).await {
        Ok((acc, AccountStatus::Available)) => Ok(Some(acc)),
        Ok((acc, AccountStatus::Expired)) => anyhow::bail!(
            "the login of {} has expired: run `bebra login --user-id {}`",
            acc.username,
            acc.user_id
        ),
        Err(e) if accounts::freshness(chosen.expire_time, now) != Freshness::Expired => {
            // The auth server can't be reached right now; the saved token may still work.
            eprintln!(
                "warning: could not check the login of {} ({e}); using the saved one",
                chosen.username
            );
            Ok(Some(chosen))
        }
        Err(e) => Err(e.into()),
    }
}

fn human_size(bytes: u64) -> String {
    const UNITS: [&str; 4] = ["B", "KB", "MB", "GB"];
    let mut v = bytes as f64;
    let mut i = 0;
    while v >= 1024.0 && i < UNITS.len() - 1 {
        v /= 1024.0;
        i += 1;
    }
    if i == 0 {
        format!("{bytes} B")
    } else {
        format!("{v:.1} {}", UNITS[i])
    }
}

fn print_cleanup_report(r: &CullReport) {
    if r.versions_removed > 0 || r.interrupted_removed > 0 || r.versions_in_use > 0 {
        println!(
            "cleanup: removed {} old versions, {} unfinished downloads, {} unused files; {} old versions kept because the game is running",
            r.versions_removed, r.interrupted_removed, r.blobs_removed, r.versions_in_use
        );
    }
}

/// Prints a progress line, but only when the percentage changes.
struct ProgressPrinter {
    label: &'static str,
    last_percent: AtomicU64,
}

impl ProgressPrinter {
    fn new(label: &'static str) -> Self {
        Self {
            label,
            last_percent: AtomicU64::new(u64::MAX),
        }
    }

    fn show(&self, p: Progress) {
        let percent = (p.done * 100).checked_div(p.total).unwrap_or(0);
        if self.last_percent.swap(percent, Ordering::Relaxed) == percent {
            return;
        }
        match p.unit {
            ProgressUnit::Items => {
                eprint!("\r  {}: {}/{} ({percent}%)   ", self.label, p.done, p.total)
            }
            ProgressUnit::Bytes => eprint!(
                "\r  {}: {:.1}/{:.1} MB ({percent}%)   ",
                self.label,
                p.done as f64 / 1e6,
                p.total as f64 / 1e6
            ),
        }
    }

    fn finish(&self) {
        if self.last_percent.load(Ordering::Relaxed) != u64::MAX {
            eprintln!();
        }
    }
}

fn print_row(address: &str, s: &ServerStatus) {
    let name: String = s
        .name
        .as_deref()
        .unwrap_or("<no name>")
        .chars()
        .take(60)
        .collect();
    println!(
        "{:>3}/{:<3}  {:<45} {}",
        s.players, s.soft_max_players, address, name
    );
}
