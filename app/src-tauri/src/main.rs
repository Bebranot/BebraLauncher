// No console window in release builds on Windows.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod applog;
mod changelog;
mod commands;
mod game;
mod news;
mod pending;
mod presence;
mod redact;
mod redial;
mod selfupdate;
mod sessions;
mod settings;
mod system;
mod uihost;

use std::sync::{Arc, Mutex};

use bebra_core::http::{self, Client};
use bebra_core::paths::DataDirs;
use tauri::Manager;

use crate::game::GameState;
use crate::settings::Settings;

/// Above this much launcher data the start-up cleaning drops old builds (`auto_clean_cache`).
const AUTO_CLEAN_BYTES: u64 = 5 * 1024 * 1024 * 1024;

/// Locks a mutex, ignoring poisoning.
///
/// A panic inside one command poisons its mutex; without this every later command would panic
/// too and the launcher would look frozen (P2-38). The data behind these mutexes is either
/// replaced wholesale or read after a panic is harmless.
pub fn lock_or_recover<T>(mutex: &Mutex<T>) -> std::sync::MutexGuard<'_, T> {
    mutex
        .lock()
        .unwrap_or_else(|poisoned| poisoned.into_inner())
}

/// Shared by every command.
pub struct AppState {
    pub client: Client,
    pub dirs: DataDirs,
    pub settings: Mutex<Settings>,
    /// The settings the UI was last given: `set_settings` uses it to tell a stale copy of a
    /// field from a deliberate edit (P1-20).
    pub settings_seen: Mutex<Settings>,
    pub game: Arc<GameState>,
    /// Events that happened before the UI could listen (a clicked link, a redial, the first
    /// `play` events of a cold start); the UI asks for them with `take_pending_actions`.
    pub pending: pending::PendingActions,
    /// The session journal file: a read-modify-write on it must not interleave.
    pub sessions: sessions::SessionJournal,
    /// The news feed of the last successful read, to not ask for it on every visit of the tab.
    pub news: Mutex<Option<(std::time::Instant, Vec<news::NewsItem>)>>,
    /// Makes the news feed single-flight: one request, the others wait for its result (P2-41).
    pub news_gate: tokio::sync::Mutex<()>,
    /// The launcher's own news, kept the same way.
    pub own_news: Mutex<Option<(std::time::Instant, Vec<news::OwnNewsItem>)>>,
    pub own_news_gate: tokio::sync::Mutex<()>,
    /// Guards the server list cache file: write to a temporary name, then rename (P2-36).
    pub servers_cache: Mutex<()>,
    /// The launcher's own update: what was found, downloaded, started.
    pub update: selfupdate::UpdateState,
    /// Whether the window and its webview are loaded right now.
    pub ui: uihost::UiHost,
}

fn main() {
    let dirs = DataDirs::default_windows().expect("no local app data folder");
    applog::init(dirs.logs());
    applog::line(&format!(
        "start BebraLauncher {}",
        env!("CARGO_PKG_VERSION")
    ));
    let mut settings = Settings::load(dirs.root());
    let recovered_running = sessions::recover_sessions(dirs.root(), &mut settings);
    let client = http::build_client_with_proxy(Some(&settings.proxy)).unwrap_or_else(|e| {
        // A broken proxy setting must not keep the launcher from starting. The address is not
        // repeated here: it may carry a login and a password (P1-13).
        applog::line(&format!("proxy ignored: {}", redact::text(&e.to_string())));
        http::build_client().expect("failed to set up the http client")
    });
    let game = Arc::new(GameState::default());
    for s in &recovered_running {
        game.register_running(s.pid, &s.image_path);
    }
    let state = AppState {
        client,
        settings_seen: Mutex::new(settings.clone()),
        settings: Mutex::new(settings),
        dirs,
        game,
        pending: pending::PendingActions::default(),
        sessions: sessions::SessionJournal::default(),
        news: Mutex::new(None),
        news_gate: tokio::sync::Mutex::new(()),
        own_news: Mutex::new(None),
        own_news_gate: tokio::sync::Mutex::new(()),
        servers_cache: Mutex::new(()),
        update: selfupdate::UpdateState::default(),
        ui: uihost::UiHost::default(),
    };
    // Installers of versions that are already installed are of no use.
    bebra_core::self_update::clean_installed(&state.dirs.root().join("updates"));

    // Old builds are dropped at start once the data takes more than AUTO_CLEAN_BYTES. The
    // cleaning holds the preparation slot: a launch arriving meanwhile (a redial that started
    // this very process, an ss14:// link) is refused as busy instead of losing its engine or
    // its scratch files to the cleaning.
    if lock_or_recover(&state.settings).auto_clean_cache {
        let dirs = state.dirs.clone();
        let games = state.game.clone();
        std::thread::spawn(move || {
            let Some(_slot) = game::hold_slot(&games) else {
                return;
            };
            if bebra_core::storage::report(&dirs).total() <= AUTO_CLEAN_BYTES {
                return;
            }
            let result = bebra_core::content_db::ContentDb::open(&dirs.content_db())
                .and_then(|db| bebra_core::storage::clean(&dirs, &db));
            match result {
                Ok(r) => applog::line(&format!("auto-clean freed {} bytes", r.bytes_freed)),
                Err(e) => applog::line(&format!("auto-clean failed ({e})")),
            }
        });
    }

    tauri::Builder::default()
        // Must be the first plugin. A second start (or the game's `--commands …` redial) is
        // handed to the running launcher and exits.
        .plugin(tauri_plugin_single_instance::init(|app, argv, _cwd| {
            commands::handle_launcher_args(app, argv.get(1..).unwrap_or_default());
        }))
        .plugin(tauri_plugin_window_state::Builder::default().build())
        .on_window_event(uihost::on_window_event)
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
            commands::optimization_presets,
            commands::list_servers,
            commands::list_accounts,
            commands::select_account,
            commands::login,
            commands::logout,
            commands::get_settings,
            commands::set_settings,
            commands::take_pending_actions,
            commands::toggle_favorite,
            commands::server_info,
            commands::cached_servers,
            commands::open_url,
            commands::get_autostart,
            commands::set_autostart,
            commands::get_links_registered,
            commands::set_links_registered,
            commands::storage_info,
            commands::list_running_clients,
            commands::clean_storage,
            commands::clean_storage_all,
            commands::play,
            commands::cancel_play,
            commands::open_logs_folder,
            commands::open_packs_folder,
            commands::list_packs,
            commands::open_patches_folder,
            commands::list_patches,
            commands::delete_patch,
            commands::install_patch,
            commands::news,
            commands::own_news,
            commands::ping_servers,
            commands::raise_favorite,
            commands::move_favorite,
            commands::edit_favorite,
            commands::set_favorite_name,
            commands::check_intel_warning,
            commands::dismiss_intel_warning,
            commands::get_recommended_hubs,
            commands::set_server_note,
            commands::add_favorite,
            commands::launch_preview,
            commands::dump_locale,
            commands::diagnostics,
            commands::content_versions,
            commands::reset_settings,
            commands::get_hwid,
            commands::set_hwid_auto,
            commands::regenerate_hwid,
            commands::restore_hwid,
            commands::open_data_folder,
            commands::list_backgrounds,
            commands::save_background,
            commands::get_background,
            commands::delete_background,
            commands::open_backgrounds_folder,
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
        .run(|app, event| match event {
            // The last window went because the interface was unloaded, not because of the player.
            tauri::RunEvent::ExitRequested {
                code: None, api, ..
            } if uihost::should_prevent_exit(app) => api.prevent_exit(),
            // A downloaded update is installed as the launcher closes.
            tauri::RunEvent::Exit => selfupdate::install_on_exit(app),
            _ => {}
        });
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A command that panics poisons its mutex; later commands must still work (P2-38).
    #[test]
    fn a_poisoned_mutex_is_recovered() {
        let shared = Arc::new(Mutex::new(1));
        let writer = shared.clone();
        let crashed = std::thread::spawn(move || {
            let _guard = writer.lock().unwrap();
            panic!("a command panicked");
        })
        .join()
        .is_err();
        assert!(crashed, "the worker thread panicked as planned");

        let mut value = lock_or_recover(&shared);
        assert_eq!(*value, 1);
        *value = 2;
        drop(value);
        assert_eq!(*lock_or_recover(&shared), 2);
    }
}
