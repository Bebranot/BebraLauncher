use std::path::PathBuf;
use std::sync::atomic::{AtomicU64, Ordering};

use anyhow::{Context, Result};
use bebra_core::content_db::ContentDb;
use bebra_core::download::{Progress, ProgressUnit};
use bebra_core::http::UrlFallbackSet;
use bebra_core::hub::HubClient;
use bebra_core::paths::DataDirs;
use bebra_core::server::AuthMode;
use bebra_core::server::{self, ServerStatus};
use bebra_core::{engine, http, launch, signature, updater, ServerAddr};
use clap::{Parser, Subcommand};

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
            let content =
                updater::install_content(&client, &db, &build, Some(&|p| printer.show(p)))
                    .await
                    .context("failed to install game files")?;
            printer.finish();
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
            let installed = engine::install_engine(
                &client,
                &UrlFallbackSet::new(engine::BUILDS_URLS),
                &dirs,
                &build.engine_version,
                &signature::official_key()?,
                Some(&|p| printer.show(p)),
            )
            .await
            .context("failed to install the engine")?;
            printer.finish();
            println!(
                "engine {}: {} ({})",
                installed.version,
                installed.zip_path.display(),
                if installed.downloaded {
                    "downloaded, signature ok"
                } else {
                    "already installed"
                }
            );
        }
        Command::Launch {
            address,
            data_dir,
            via_hub,
            loader,
            username,
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

            let info = if via_hub {
                HubClient::official(client.clone())
                    .server_info(addr.as_str())
                    .await?
            } else {
                server::fetch_info(&client, &addr)
                    .await
                    .with_context(|| format!("{} did not answer /info", addr.info_url()))?
            };
            let mut build = info
                .build
                .clone()
                .context("the server gave no build information")?;
            if via_hub {
                build.apply_self_hosting(&addr);
            }
            if !no_connect && info.auth.mode == AuthMode::Required {
                eprintln!(
                    "note: this server requires an account; playing as a guest will be refused (accounts come later)"
                );
            }

            let db = ContentDb::open(&dirs.content_db())?;
            let printer = ProgressPrinter::new("files");
            let content =
                updater::install_content(&client, &db, &build, Some(&|p| printer.show(p)))
                    .await
                    .context("failed to install game files")?;
            printer.finish();
            let printer = ProgressPrinter::new("engine");
            let installed = engine::install_engine(
                &client,
                &UrlFallbackSet::new(engine::BUILDS_URLS),
                &dirs,
                &build.engine_version,
                &signature::official_key()?,
                Some(&|p| printer.show(p)),
            )
            .await
            .context("failed to install the engine")?;
            printer.finish();

            let spec = launch::LaunchSpec {
                loader_exe,
                dotnet_root: None,
                engine: installed,
                signing_key_file: launch::ensure_signing_key_file(&dirs)?,
                content_db: dirs.content_db(),
                content_version_id: content.version_id,
                modules: vec![],
                username,
                auth: None,
                build,
                connect: (!no_connect).then(|| launch::ConnectTarget {
                    connect_url: info.connect_url(&addr),
                    server_address: addr.normalized(),
                }),
                compat_mode: compat,
            };
            let cmd = launch::build_command(&spec);
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
