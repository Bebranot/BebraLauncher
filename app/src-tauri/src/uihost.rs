//! Keeps the WebView2 interface out of memory while it is not needed.
//!
//! Chromium is nearly all of the launcher's memory: the process itself takes about 15 MB. So
//! while a game runs the window and its webview are destroyed, the process stays to watch the
//! game, and a tray icon is the way back. A minimized window keeps its page, but asks WebView2
//! to use as little memory as it can.
//!
//! A destroyed page hears nothing: `PendingActions::page_gone` makes the backend keep events
//! until the next page asks for them, and that page reads the rest of its state as on any start.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use tauri::menu::{Menu, MenuItem};
use tauri::tray::{MouseButton, MouseButtonState, TrayIconBuilder, TrayIconEvent};
use tauri::{AppHandle, Manager, WebviewWindowBuilder, Window, WindowEvent};
use tauri_plugin_window_state::{AppHandleExt, StateFlags};
use webview2_com::Microsoft::Web::WebView2::Win32::{
    ICoreWebView2_19, COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW,
    COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL,
};
use windows_core::Interface;

const WINDOW_LABEL: &str = "main";
const TRAY_ID: &str = "game-running";
const MENU_OPEN: &str = "open";
const MENU_QUIT: &str = "quit";

/// How long the window stays after the game process starts: the page saves its last edits, and a
/// game that dies at once leaves its error on screen instead of a window closing and reopening.
const UNLOAD_DELAY: Duration = Duration::from_secs(3);
/// How long a new window waits for the old one to finish closing: the label must be free.
const OLD_WINDOW_WAIT: Duration = Duration::from_secs(2);

#[derive(Default)]
pub struct UiHost {
    /// Makes unloading and reopening one at a time. The main thread must never wait for it: the
    /// holder waits for the main thread while it creates a window.
    transition: Mutex<()>,
    /// Read by the exit handler on the main thread, so it is not behind `transition`.
    unloaded: AtomicBool,
    memory_low: AtomicBool,
}

/// Tauri exits when the last window goes. That is right when the player closes the launcher, but
/// not when the window was closed on purpose, or when a new one has already replaced it.
fn keep_running_without_window(unloaded: bool, open_windows: usize) -> bool {
    unloaded || open_windows > 0
}

/// Called for `RunEvent::ExitRequested` without an exit code: all windows are gone.
pub fn should_prevent_exit(app: &AppHandle) -> bool {
    let unloaded = app
        .state::<crate::AppState>()
        .ui
        .unloaded
        .load(Ordering::SeqCst);
    keep_running_without_window(unloaded, app.webview_windows().len())
}

/// Unloads the interface a moment after a game starts, if that game is still running then.
pub fn unload_after_game_start(app: &AppHandle) {
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("ui-unload".into())
        .spawn(move || {
            std::thread::sleep(UNLOAD_DELAY);
            unload(&app);
        });
    if let Err(e) = spawned {
        crate::applog::line(&format!("interface stays loaded: no thread ({e})"));
    }
}

/// Destroys the window and its webview, and leaves a tray icon.
pub fn unload(app: &AppHandle) {
    let state = app.state::<crate::AppState>();
    let _transition = crate::lock_or_recover(&state.ui.transition);
    if state.ui.unloaded.load(Ordering::SeqCst) || state.game.running_count() == 0 {
        return;
    }
    let Some(window) = app.get_webview_window(WINDOW_LABEL) else {
        return;
    };
    // The window is gone before the plugin would see it close.
    if let Err(e) = app.save_window_state(StateFlags::all()) {
        crate::applog::line(&format!("window state not saved ({e})"));
    }
    // Set first: the events from here on and the exit request must see an unloaded interface.
    state.ui.unloaded.store(true, Ordering::SeqCst);
    state.pending.page_gone();
    state.ui.memory_low.store(false, Ordering::SeqCst);
    if let Err(e) = show_tray(app) {
        // The window can still be opened by starting the launcher again.
        crate::applog::line(&format!("no tray icon ({e})"));
    }
    if let Err(e) = window.destroy() {
        crate::applog::line(&format!("window not closed ({e})"));
        state.ui.unloaded.store(false, Ordering::SeqCst);
        let _ = app.remove_tray_by_id(TRAY_ID);
        return;
    }
    bebra_core::process::trim_working_set();
    crate::applog::line("interface unloaded while the game runs");
}

/// Brings an unloaded interface back. Returns whether it was unloaded; the window itself appears
/// from a thread of its own, because a webview cannot be created from a window or tray callback.
pub fn reopen(app: &AppHandle) -> bool {
    let state = app.state::<crate::AppState>();
    if !state.ui.unloaded.load(Ordering::SeqCst) {
        return false;
    }
    let app = app.clone();
    let spawned = std::thread::Builder::new()
        .name("ui-reopen".into())
        .spawn(move || reopen_now(&app));
    if let Err(e) = spawned {
        crate::applog::line(&format!("interface not reopened: no thread ({e})"));
    }
    true
}

fn reopen_now(app: &AppHandle) {
    let state = app.state::<crate::AppState>();
    let _transition = crate::lock_or_recover(&state.ui.transition);
    if !state.ui.unloaded.load(Ordering::SeqCst) {
        return;
    }
    let deadline = Instant::now() + OLD_WINDOW_WAIT;
    while app.get_webview_window(WINDOW_LABEL).is_some() && Instant::now() < deadline {
        std::thread::sleep(Duration::from_millis(50));
    }
    let config = app
        .config()
        .app
        .windows
        .iter()
        .find(|w| w.label == WINDOW_LABEL)
        .cloned();
    let built = config
        .ok_or_else(|| "no window in the config".to_string())
        .and_then(|config| {
            WebviewWindowBuilder::from_config(app, &config)
                .and_then(|builder| builder.build())
                .map_err(|e| e.to_string())
        });
    match built {
        Ok(_) => {
            // After the window exists: an exit request still on its way must find one or the other.
            state.ui.unloaded.store(false, Ordering::SeqCst);
            let _ = app.remove_tray_by_id(TRAY_ID);
            crate::applog::line("interface reopened");
        }
        // The tray icon stays, so there is still a way to try again.
        Err(e) => crate::applog::line(&format!("interface not reopened ({e})")),
    }
}

/// The menu's words. The page and its translations are not loaded when this is shown.
fn tray_labels(language: &str) -> (&'static str, &'static str, &'static str) {
    match language {
        "ru" => (
            "BebraLauncher: идёт игра",
            "Открыть лаунчер",
            "Закрыть лаунчер",
        ),
        "uk" => (
            "BebraLauncher: триває гра",
            "Відкрити лаунчер",
            "Закрити лаунчер",
        ),
        "de" => (
            "BebraLauncher: Spiel läuft",
            "Launcher öffnen",
            "Launcher schließen",
        ),
        _ => (
            "BebraLauncher: game running",
            "Open launcher",
            "Close launcher",
        ),
    }
}

fn show_tray(app: &AppHandle) -> tauri::Result<()> {
    let state = app.state::<crate::AppState>();
    let language =
        crate::presence::resolve_language(&crate::lock_or_recover(&state.settings).language);
    let (tooltip, open, quit) = tray_labels(&language);
    let open = MenuItem::with_id(app, MENU_OPEN, open, true, None::<&str>)?;
    let quit = MenuItem::with_id(app, MENU_QUIT, quit, true, None::<&str>)?;
    let menu = Menu::with_items(app, &[&open, &quit])?;
    let mut builder = TrayIconBuilder::with_id(TRAY_ID)
        .tooltip(tooltip)
        .menu(&menu)
        .show_menu_on_left_click(false)
        .on_menu_event(|app, event| match event.id().as_ref() {
            MENU_OPEN => {
                reopen(app);
            }
            // The game goes on: its time is credited from the journal on the next start.
            MENU_QUIT => app.exit(0),
            _ => {}
        })
        .on_tray_icon_event(|tray, event| {
            if let TrayIconEvent::Click {
                button: MouseButton::Left,
                button_state: MouseButtonState::Up,
                ..
            } = event
            {
                reopen(tray.app_handle());
            }
        });
    if let Some(icon) = app.default_window_icon() {
        builder = builder.icon(icon.clone());
    }
    builder.build(app)?;
    Ok(())
}

/// Follows the window: minimized means WebView2 may give back what it can.
pub fn on_window_event(window: &Window, event: &WindowEvent) {
    if !matches!(event, WindowEvent::Resized(_) | WindowEvent::Focused(_)) {
        return;
    }
    let Ok(minimized) = window.is_minimized() else {
        return;
    };
    let app = window.app_handle();
    let state = app.state::<crate::AppState>();
    if state.ui.memory_low.swap(minimized, Ordering::SeqCst) == minimized {
        return;
    }
    let Some(webview) = app.get_webview_window(window.label()) else {
        return;
    };
    let result = webview.with_webview(move |platform| {
        let level = if minimized {
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_LOW
        } else {
            COREWEBVIEW2_MEMORY_USAGE_TARGET_LEVEL_NORMAL
        };
        // SAFETY: the controller is alive during this callback, and these are plain COM calls on
        // the webview's own thread. An older WebView2 without this interface is simply skipped.
        unsafe {
            if let Ok(core) = platform
                .controller()
                .CoreWebView2()
                .and_then(|core| core.cast::<ICoreWebView2_19>())
            {
                let _ = core.SetMemoryUsageTargetLevel(level);
            }
        }
    });
    if let Err(e) = result {
        crate::applog::line(&format!("webview memory level not set ({e})"));
    }
    if minimized {
        bebra_core::process::trim_working_set();
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn only_closing_the_window_yourself_ends_the_launcher() {
        assert!(
            !keep_running_without_window(false, 0),
            "the player closed it"
        );
        assert!(keep_running_without_window(true, 0), "unloaded for a game");
        assert!(
            keep_running_without_window(false, 1),
            "a new window replaced the old one before the request arrived"
        );
    }

    #[test]
    fn the_tray_speaks_every_interface_language() {
        let english = tray_labels("en");
        for language in ["ru", "uk", "de"] {
            assert_ne!(tray_labels(language), english, "{language}");
        }
        assert_eq!(tray_labels("xx"), english, "unknown means English");
    }
}
