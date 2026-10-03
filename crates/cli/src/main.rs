use anyhow::{Context, Result};
use bebra_core::hub::HubClient;
use bebra_core::server::{self, ServerStatus};
use bebra_core::{http, ServerAddr};
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
    }
    Ok(())
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
