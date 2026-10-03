// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod applog;
mod changelog;
mod commands;
mod game;
mod news;
mod redial;
mod selfupdate;
mod settings;
mod system;

use std::sync::{Arc, Mutex};

use bebra_core::http::{self, Client};
use bebra_core::paths::DataDirs;
use tauri::Manager;

use crate::game::GameState;
use crate::settings::Settings;

/// Shared by every command.
pub struct AppState {
    pub client: Client,
    pub dirs: DataDirs,
    pub settings: Mutex<Settings>,
    pub game: Arc<GameState>,
    /// The news feed of the last successful read, to not ask for it on every visit of the tab.
    pub news: Mutex<Option<(std::time::Instant, Vec<news::NewsItem>)>>,
    /// The launcher's own update: what was found, downloaded, started.
    pub update: selfupdate::UpdateState,
}

fn main() {
    let dirs = DataDirs::default_windows().expect("no local app data folder");
    applog::init(dirs.logs());
    applog::line(&format!(
        "start BebraLauncher {}",
        env!("CARGO_PKG_VERSION")
    ));
    let settings = Settings::load(dirs.root());
    let client = http::build_client_with_proxy(Some(&settings.proxy)).unwrap_or_else(|e| {
        // A broken proxy setting must not keep the launcher from starting.
        applog::line(&format!("proxy ignored: {e}"));
        http::build_client().expect("failed to set up the http client")
    });
    let state = AppState {
        client,
        settings: Mutex::new(settings),
        dirs,
        game: Arc::default(),
        news: Mutex::new(None),
        update: selfupdate::UpdateState::default(),
    };
    // Installers of versions that are already installed are of no use.
    bebra_core::self_update::clean_installed(&state.dirs.root().join("updates"));

    tauri::Builder::default()
        // Must be the first plugin. A second start (or the game's `--commands …` redial) is
        // handed to the running launcher and exits.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            commands::handle_launcher_args(app, argv.get(1..).unwrap_or_default());
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .setup(|app| {
            let mut args: Vec<String> = std::env::args().skip(1).collect();
            // Started by Windows at login: stay out of the way.
            if args.iter().any(|a| a == system::AUTOSTART_ARG) {
                args.retain(|a| a != system::AUTOSTART_ARG);
                if let Some(window) = app.get_webview_window("main") {
                    let _ = window.minimize();
                }
            }
            // Started with commands (the launcher was not running when the game asked to redial).
            if !args.is_empty() {
                commands::handle_launcher_args(app.handle(), &args);
            }
            selfupdate::start_background_checks(app.handle().clone());
            Ok(())
        })
        .manage(state)
        .invoke_handler(tauri::generate_handler![
            commands::app_info,
            commands::list_servers,
            commands::list_accounts,
            commands::login,
            commands::logout,
            commands::get_settings,
            commands::set_settings,
            commands::toggle_favorite,
            commands::server_info,
            commands::cached_servers,
            commands::open_url,
            commands::get_autostart,
            commands::set_autostart,
            commands::get_links_registered,
            commands::set_links_registered,
            commands::storage_info,
            commands::clean_storage,
            commands::play,
            commands::cancel_play,
            commands::open_logs_folder,
            commands::open_packs_folder,
            commands::list_packs,
            commands::news,
            commands::ping_servers,
            commands::raise_favorite,
            commands::add_favorite,
            commands::launch_preview,
            commands::dump_locale,
            commands::diagnostics,
            commands::content_versions,
            commands::reset_settings,
            commands::open_data_folder,
            commands::update_status,
            commands::check_update,
            commands::download_update,
            commands::install_update,
            commands::update_token_state,
            commands::set_update_token,
            commands::changelog,
        ])
        .build(tauri::generate_context!())
        .expect("error while building the app")
        .run(|app, event| {
            // A downloaded update is installed as the launcher closes.
            if let tauri::RunEvent::Exit = event {
                selfupdate::install_on_exit(app);
            }
        });
}
