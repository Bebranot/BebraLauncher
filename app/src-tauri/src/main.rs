// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod commands;
mod game;
mod settings;

use std::sync::{Arc, Mutex};

use bebra_core::http::{self, Client};
use bebra_core::paths::DataDirs;

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
    let state = AppState {
        client: http::build_client().expect("failed to set up the http client"),
        settings: Mutex::new(Settings::load(dirs.root())),
        dirs,
        game: Arc::default(),
    };

    tauri::Builder::default()
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
            commands::play,
            commands::cancel_play,
            commands::open_logs_folder,
        ])
        .run(tauri::generate_context!())
        .expect("error while running the app");
}
