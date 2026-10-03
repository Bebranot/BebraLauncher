// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod applog;
mod commands;
mod game;
mod redial;
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
}

fn main() {
    let dirs = DataDirs::default_windows().expect("no local app data folder");
    applog::init(dirs.logs());
    applog::line(&format!(
        "start BebraLauncher {}",
        env!("CARGO_PKG_VERSION")
    ));
    let state = AppState {
        client: http::build_client().expect("failed to set up the http client"),
        settings: Mutex::new(Settings::load(dirs.root())),
        dirs,
        game: Arc::default(),
    };

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
        ])
        .run(tauri::generate_context!())
        .expect("error while running the app");
}
