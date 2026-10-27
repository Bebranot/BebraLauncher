//! User settings: one small JSON file in the launcher's data folder.

use std::path::{Path, PathBuf};

use bebra_core::launch::FALLBACK_USERNAME;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// The name used when playing without an account.
    pub guest_name: String,
    /// Start the game in the GLES2 fallback renderer.
    pub compat_mode: bool,
    /// Whether Discord Rich Presence is enabled in the game client.
    pub discord_rpc: bool,
    /// Minimize the launcher while the game runs, and bring it back when the game closes.
    pub minimize_on_play: bool,
    /// Id of the colour theme; the UI knows the list and falls back to the default.
    pub theme: String,
    /// Gradient background (instead of a flat colour).
    pub theme_gradient: bool,
    /// Decorative striped pattern behind the lists.
    pub theme_decor: bool,
    /// Colours of the `custom` theme.
    pub custom_theme: CustomTheme,
    /// Font family for the interface; empty = the built-in one. Must be installed in Windows.
    pub font: String,
    /// Which columns the server list shows.
    pub columns: Columns,
    /// Text packs to use (file names in the packs folder), the first one wins on conflicts.
    pub enabled_packs: Vec<String>,
    /// Do not follow the game's requests to reconnect (redial).
    pub ignore_redial: bool,
    /// Write more detail to the launcher log (the launch command, with secrets hidden).
    pub verbose_log: bool,
    /// Proxy for the launcher's own requests (`http://`, `https://`, `socks5://`); empty = none.
    pub proxy: String,
    /// `auto` (follow Windows) or the code of an interface language (`ru`, `en`, `uk`, `de`).
    pub language: String,
    /// The GitHub repository whose releases update the launcher (`owner/name`).
    pub update_repo: String,
    /// Look for a new launcher version at start and every few hours.
    pub update_check: bool,
    /// Download a new version by itself and install it when the launcher is closed.
    pub update_auto: bool,
    /// The account chosen last (user id); preselected next time.
    /// An empty string means the player chose to play as a guest; `None`: never chosen.
    pub last_account: Option<String>,
    /// Addresses of favourite servers.
    pub favorites: Vec<String>,
    /// Servers played recently, newest first.
    pub history: Vec<HistoryEntry>,
    /// Server hubs used to populate the server list.
    pub hubs: Vec<HubSetting>,
    /// Additional arguments passed to the game client / engine.
    pub extra_args: String,
    /// Client patches (.dll files in patches folder) enabled by the user.
    pub enabled_patches: Vec<String>,
    /// Client optimization profile: "off", "balanced", "max_fps", "potato", or "custom".
    pub opt_profile: String,
    /// Fast lighting: disable soft shadows and lighting blur.
    pub opt_fast_lighting: bool,
    /// Unlock framerate: disable VSync and set max FPS to uncapped.
    pub opt_unlock_fps: bool,
    /// Run game process with elevated Windows priority.
    pub opt_high_priority: bool,
    /// Enable .NET Server Garbage Collection and DATAS to prevent frame stutters.
    pub opt_server_gc: bool,
    /// Background mode: "space" (default space canvas), "custom" (user image), "none" (solid/theme).
    pub bg_type: String,
    /// Filename of selected custom background in backgrounds folder.
    pub bg_custom_name: Option<String>,
    /// Background darkening tint overlay percentage (0..95, default 40).
    pub bg_dim: u32,
    /// Background blur filter in px (0..30, default 0).
    pub bg_blur: u32,
    /// Game client window mode: "default", "windowed", "fullscreen", "borderless".
    pub client_window_mode: String,
    /// Mute game audio when client is unfocused / minimized.
    pub client_mute_unfocused: bool,
    /// Game client UI scale: "auto", "0.75", "1.0", "1.25", "1.5", "1.75", "2.0".
    pub client_ui_scale: String,
    /// Maximum client FPS limit (0 = uncapped / profile default).
    pub client_max_fps: u32,
    /// Close the launcher completely when the game starts.
    pub close_on_play: bool,
    /// Server list auto-refresh interval in seconds (0 = off, 15, 30, 60, 120).
    pub auto_refresh_interval: u32,
    /// Confirm connect with server details modal before launching.
    pub confirm_connect: bool,
    /// Personal user notes per server address.
    #[serde(default)]
    pub server_notes: std::collections::HashMap<String, String>,
    /// Custom user-assigned names for favourite servers.
    #[serde(default)]
    pub favorite_names: std::collections::HashMap<String, String>,
    /// Scale factor for the launcher's own UI (percent, 75..=200).
    #[serde(default = "default_ui_scale")]
    pub launcher_ui_scale: u32,
    /// Whether the user has dismissed the Intel 13th/14th Gen CPU degradation advisory.
    #[serde(default)]
    pub dismissed_intel_warning: bool,
}

fn default_ui_scale() -> u32 {
    100
}

/// Configured server hub.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct HubSetting {
    pub url: String,
    pub enabled: bool,
}

impl Default for HubSetting {
    fn default() -> Self {
        Self {
            url: bebra_core::hub::OFFICIAL_HUB_URL.to_string(),
            enabled: true,
        }
    }
}

pub fn default_hubs() -> Vec<HubSetting> {
    vec![HubSetting {
        url: bebra_core::hub::OFFICIAL_HUB_URL.to_string(),
        enabled: true,
    }]
}

/// Colours of the user's own theme, as `#RRGGBB`.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct CustomTheme {
    pub background: String,
    pub accent: String,
    pub foreground: String,
    pub popup: String,
    pub gradient_start: String,
    pub gradient_end: String,
}

impl Default for CustomTheme {
    fn default() -> Self {
        Self {
            background: "#25252A".into(),
            accent: "#ADA24B".into(),
            foreground: "#EEEEEE".into(),
            popup: "#202025".into(),
            gradient_start: "#2A2A31".into(),
            gradient_end: "#2F3A36".into(),
        }
    }
}

impl CustomTheme {
    fn sanitized(self) -> Self {
        let d = Self::default();
        let fix = |value: String, fallback: String| {
            if is_hex_color(&value) {
                value.to_uppercase()
            } else {
                fallback
            }
        };
        Self {
            background: fix(self.background, d.background),
            accent: fix(self.accent, d.accent),
            foreground: fix(self.foreground, d.foreground),
            popup: fix(self.popup, d.popup),
            gradient_start: fix(self.gradient_start, d.gradient_start),
            gradient_end: fix(self.gradient_end, d.gradient_end),
        }
    }
}

fn is_hex_color(s: &str) -> bool {
    s.len() == 7 && s.starts_with('#') && s[1..].chars().all(|c| c.is_ascii_hexdigit())
}

/// Columns of the server list besides the name and the Connect button.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(default)]
pub struct Columns {
    pub round_time: bool,
    pub players: bool,
    pub map: bool,
    pub mode: bool,
    pub ping: bool,
}

impl Default for Columns {
    fn default() -> Self {
        Self {
            round_time: true,
            players: true,
            map: false,
            mode: false,
            ping: false,
        }
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub address: String,
    pub name: Option<String>,
    /// RFC 3339.
    pub last_played: String,
    /// Time spent in the game on this server, in seconds.
    #[serde(default)]
    pub play_seconds: u64,
    /// The account used here last time (user id; empty = guest), preselected next time.
    #[serde(default)]
    pub account: Option<String>,
}

/// How many recent servers are remembered.
pub const HISTORY_LIMIT: usize = 20;

impl Default for Settings {
    fn default() -> Self {
        Self {
            guest_name: FALLBACK_USERNAME.to_string(),
            compat_mode: false,
            discord_rpc: true,
            minimize_on_play: true,
            theme: "dark".to_string(),
            theme_gradient: false,
            theme_decor: false,
            custom_theme: CustomTheme::default(),
            font: String::new(),
            columns: Columns::default(),
            enabled_packs: Vec::new(),
            ignore_redial: false,
            verbose_log: false,
            proxy: String::new(),
            language: "auto".to_string(),
            update_repo: bebra_core::self_update::DEFAULT_REPO.to_string(),
            update_check: true,
            update_auto: true,
            last_account: None,
            favorites: Vec::new(),
            history: Vec::new(),
            hubs: default_hubs(),
            extra_args: String::new(),
            enabled_patches: Vec::new(),
            opt_profile: "balanced".to_string(),
            opt_fast_lighting: false,
            opt_unlock_fps: false,
            opt_high_priority: true,
            opt_server_gc: true,
            bg_type: "space".to_string(),
            bg_custom_name: None,
            bg_dim: 40,
            bg_blur: 0,
            client_window_mode: "default".to_string(),
            client_mute_unfocused: true,
            client_ui_scale: "auto".to_string(),
            client_max_fps: 0,
            close_on_play: false,
            auto_refresh_interval: 0,
            confirm_connect: false,
            server_notes: std::collections::HashMap::new(),
            favorite_names: std::collections::HashMap::new(),
            launcher_ui_scale: 100,
            dismissed_intel_warning: false,
        }
    }
}

impl Settings {
    pub fn path(root: &Path) -> PathBuf {
        root.join("settings.json")
    }

    /// A missing or unreadable file means defaults: settings must never stop the app from starting.
    pub fn load(root: &Path) -> Self {
        std::fs::read(Self::path(root))
            .ok()
            .and_then(|b| serde_json::from_slice(&b).ok())
            .unwrap_or_default()
    }

    pub fn save(&self, root: &Path) -> std::io::Result<()> {
        std::fs::create_dir_all(root)?;
        let path = Self::path(root);
        let tmp = path.with_extension("json.tmp");
        std::fs::write(&tmp, serde_json::to_vec_pretty(self)?)?;
        std::fs::rename(tmp, path)
    }

    /// Moves the server to the top of the history.
    pub fn remember_played(&mut self, address: &str, name: Option<String>, when: String) {
        let (play_seconds, account) = self
            .history
            .iter()
            .find(|h| h.address == address)
            .map_or((0, None), |h| (h.play_seconds, h.account.clone()));
        self.history.retain(|h| h.address != address);
        self.history.insert(
            0,
            HistoryEntry {
                address: address.to_string(),
                name,
                last_played: when,
                play_seconds,
                account,
            },
        );
        self.history.truncate(HISTORY_LIMIT);
    }

    /// Notes which account was used on a server that is in the history.
    pub fn remember_account(&mut self, address: &str, account: &str) {
        if let Some(h) = self.history.iter_mut().find(|h| h.address == address) {
            h.account = Some(account.to_string());
        }
    }

    /// Adds time played to a server that is in the history (a server that dropped off the
    /// list in the meantime is simply not counted).
    pub fn add_play_time(&mut self, address: &str, seconds: u64) {
        if let Some(h) = self.history.iter_mut().find(|h| h.address == address) {
            h.play_seconds = h.play_seconds.saturating_add(seconds);
        }
    }

    /// Adds or removes a favourite. Returns whether it is a favourite now.
    pub fn toggle_favorite(&mut self, address: &str) -> bool {
        if let Some(i) = self.favorites.iter().position(|a| a == address) {
            self.favorites.remove(i);
            self.favorite_names.remove(address);
            false
        } else {
            self.favorites.push(address.to_string());
            true
        }
    }

    /// Moves a favourite server up (delta < 0) or down (delta > 0) in the list.
    pub fn move_favorite(&mut self, address: &str, delta: i32) {
        if let Some(i) = self.favorites.iter().position(|a| a == address) {
            let len = self.favorites.len();
            if len <= 1 {
                return;
            }
            let target = (i as i32 + delta).clamp(0, (len - 1) as i32) as usize;
            if target != i {
                let item = self.favorites.remove(i);
                self.favorites.insert(target, item);
            }
        }
    }

    /// Renames or changes the address of an existing favourite.
    pub fn edit_favorite(&mut self, old_address: &str, new_address: &str, new_name: Option<String>) {
        if let Some(i) = self.favorites.iter().position(|a| a == old_address) {
            let saved_name = new_name.or_else(|| self.favorite_names.remove(old_address));
            self.favorites[i] = new_address.to_string();
            if let Some(n) = saved_name {
                let trimmed = n.trim();
                if !trimmed.is_empty() {
                    self.favorite_names.insert(new_address.to_string(), trimmed.chars().take(100).collect());
                } else {
                    self.favorite_names.remove(new_address);
                }
            }
        }
    }

    /// Sets or clears a custom alias for a favourite server.
    pub fn set_favorite_name(&mut self, address: &str, name: Option<String>) {
        if let Some(n) = name {
            let trimmed = n.trim();
            if !trimmed.is_empty() {
                self.favorite_names.insert(address.to_string(), trimmed.chars().take(100).collect());
                return;
            }
        }
        self.favorite_names.remove(address);
    }

    /// Sets or removes a personal note for a server address.
    pub fn set_server_note(&mut self, address: &str, note: &str) {
        let trimmed = note.trim();
        if trimmed.is_empty() {
            self.server_notes.remove(address);
        } else {
            let clean: String = trimmed
                .chars()
                .filter(|c| !c.is_control() || *c == '\n' || *c == '\r' || *c == '\t')
                .take(500)
                .collect();
            let final_text = clean.trim().to_string();
            if final_text.is_empty() {
                self.server_notes.remove(address);
            } else {
                self.server_notes.insert(address.to_string(), final_text);
            }
        }
    }

    /// Cleans up values typed by the user.
    pub fn sanitized(mut self) -> Self {
        let name: String = self
            .guest_name
            .chars()
            .filter(|c| !c.is_control())
            .collect::<String>()
            .trim()
            .chars()
            .take(32)
            .collect();
        self.guest_name = if name.is_empty() {
            FALLBACK_USERNAME.to_string()
        } else {
            name
        };

        self.custom_theme = self.custom_theme.sanitized();

        // A font name ends up in a CSS declaration: allow only what a family name needs.
        self.font = self
            .font
            .chars()
            .filter(|c| c.is_alphanumeric() || matches!(c, ' ' | '-' | '_' | '.'))
            .collect::<String>()
            .trim()
            .chars()
            .take(64)
            .collect();

        // A proxy that cannot be used is dropped rather than kept to fail on every start.
        self.proxy = self.proxy.trim().to_string();
        if !bebra_core::http::is_valid_proxy(&self.proxy) {
            self.proxy.clear();
        }

        // The repository ends up in a URL: it must look like `owner/name`.
        self.update_repo = self.update_repo.trim().to_string();
        if !bebra_core::self_update::valid_repo(&self.update_repo) {
            self.update_repo = bebra_core::self_update::DEFAULT_REPO.to_string();
        }

        // Pack ids are plain file names, each at most once.
        let mut seen = std::collections::HashSet::new();
        self.enabled_packs.retain(|id| {
            !id.is_empty()
                && id.len() <= 200
                && !id.contains(['/', '\\'])
                && seen.insert(id.clone())
        });

        // Patch ids are plain file names, each at most once.
        let mut seen_patches = std::collections::HashSet::new();
        self.enabled_patches.retain(|id| {
            !id.is_empty()
                && id.len() <= 200
                && !id.contains(['/', '\\'])
                && seen_patches.insert(id.clone())
        });

        // Hubs: normalize URLs, filter out invalid/duplicate URLs.
        let mut seen_hubs = std::collections::HashSet::new();
        let mut clean_hubs = Vec::new();
        for h in self.hubs {
            if let Some(norm) = bebra_core::hub::normalize_hub_url(&h.url) {
                if seen_hubs.insert(norm.clone()) {
                    clean_hubs.push(HubSetting {
                        url: norm,
                        enabled: h.enabled,
                    });
                }
            }
        }
        if clean_hubs.is_empty() {
            clean_hubs = default_hubs();
        }
        self.hubs = clean_hubs;

        // Extra args: clean control characters, limit length.
        self.extra_args = self
            .extra_args
            .chars()
            .filter(|c| !c.is_control() || *c == ' ' || *c == '\t')
            .take(1000)
            .collect::<String>()
            .trim()
            .to_string();

        match self.opt_profile.as_str() {
            "off" | "none" | "balanced" | "max_fps" | "potato" | "iskander" | "custom" => {}
            _ => self.opt_profile = "balanced".to_string(),
        }

        match self.bg_type.as_str() {
            "space" | "custom" | "none" => {}
            _ => self.bg_type = "space".to_string(),
        }

        self.bg_custom_name = self.bg_custom_name.and_then(|name| {
            let clean: String = name
                .chars()
                .filter(|c| !c.is_control() && *c != '/' && *c != '\\')
                .take(200)
                .collect();
            let trimmed = clean.trim();
            if trimmed.is_empty() {
                None
            } else {
                Some(trimmed.to_string())
            }
        });

        self.bg_dim = self.bg_dim.min(95);
        self.bg_blur = self.bg_blur.min(30);

        match self.client_window_mode.as_str() {
            "default" | "windowed" | "fullscreen" | "borderless" => {}
            _ => self.client_window_mode = "default".to_string(),
        }

        match self.client_ui_scale.as_str() {
            "auto" | "0.75" | "1.0" | "1.25" | "1.5" | "1.75" | "2.0" => {}
            _ => self.client_ui_scale = "auto".to_string(),
        }

        self.client_max_fps = self.client_max_fps.min(360);

        match self.auto_refresh_interval {
            0 | 15 | 30 | 60 | 120 | 300 => {}
            _ => self.auto_refresh_interval = 0,
        }

        self.launcher_ui_scale = self.launcher_ui_scale.clamp(75, 200);

        let mut clean_notes = std::collections::HashMap::new();
        for (addr, note) in self.server_notes {
            let addr = addr.trim().to_string();
            if addr.is_empty() || addr.len() > 256 {
                continue;
            }
            let clean_note: String = note
                .chars()
                .filter(|c| !c.is_control() || *c == '\n' || *c == '\r' || *c == '\t')
                .take(500)
                .collect();
            let trimmed = clean_note.trim().to_string();
            if !trimmed.is_empty() {
                clean_notes.insert(addr, trimmed);
            }
        }
        self.server_notes = clean_notes;

        let mut clean_fav_names = std::collections::HashMap::new();
        for (addr, name) in self.favorite_names {
            let addr = addr.trim().to_string();
            if addr.is_empty() || addr.len() > 256 {
                continue;
            }
            let clean_name: String = name
                .chars()
                .filter(|c| !c.is_control() || *c == ' ')
                .take(100)
                .collect();
            let trimmed = clean_name.trim().to_string();
            if !trimmed.is_empty() {
                clean_fav_names.insert(addr, trimmed);
            }
        }
        self.favorite_names = clean_fav_names;

        self
    }

    /// Resolves the optimization profile into a concrete runtime configuration.
    pub fn optimization_config(&self) -> bebra_core::launch::OptimizationConfig {
        match self.opt_profile.as_str() {
            "off" | "none" => bebra_core::launch::OptimizationConfig {
                profile: "off".into(),
                server_gc: false,
                fast_lighting: false,
                unlock_fps: false,
                high_priority: false,
                light_resolution: None,
            },
            "potato" => bebra_core::launch::OptimizationConfig {
                profile: "potato".into(),
                server_gc: true,
                fast_lighting: true,
                unlock_fps: true,
                high_priority: true,
                light_resolution: Some(0.35),
            },
            "iskander" => bebra_core::launch::OptimizationConfig {
                profile: "iskander".into(),
                server_gc: true,
                fast_lighting: true,
                unlock_fps: true,
                high_priority: true,
                light_resolution: Some(0.2),
            },
            "max_fps" => bebra_core::launch::OptimizationConfig {
                profile: "max_fps".into(),
                server_gc: true,
                fast_lighting: false,
                unlock_fps: true,
                high_priority: true,
                light_resolution: None,
            },
            "custom" => bebra_core::launch::OptimizationConfig {
                profile: "custom".into(),
                server_gc: self.opt_server_gc,
                fast_lighting: self.opt_fast_lighting,
                unlock_fps: self.opt_unlock_fps,
                high_priority: self.opt_high_priority,
                light_resolution: if self.opt_fast_lighting {
                    Some(0.35)
                } else {
                    None
                },
            },
            _ => bebra_core::launch::OptimizationConfig {
                profile: "balanced".into(),
                server_gc: true,
                fast_lighting: false,
                unlock_fps: false,
                high_priority: true,
                light_resolution: None,
            },
        }
    }

    /// Moves a favourite to the top of the list.
    pub fn raise_favorite(&mut self, address: &str) {
        if let Some(i) = self.favorites.iter().position(|a| a == address) {
            let item = self.favorites.remove(i);
            self.favorites.insert(0, item);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roundtrip_and_defaults() {
        let dir = tempdir();
        assert_eq!(Settings::load(&dir).guest_name, FALLBACK_USERNAME);
        assert!(Settings::load(&dir).minimize_on_play);

        let s = Settings {
            guest_name: "Bebra".into(),
            compat_mode: true,
            last_account: Some("x".into()),
            ..Settings::default()
        };
        s.save(&dir).unwrap();
        let back = Settings::load(&dir);
        assert_eq!(back.guest_name, "Bebra");
        assert!(back.compat_mode);
        assert_eq!(back.last_account.as_deref(), Some("x"));
    }

    #[test]
    fn broken_file_gives_defaults_and_partial_file_fills_in() {
        let dir = tempdir();
        std::fs::write(Settings::path(&dir), b"{ not json").unwrap();
        assert_eq!(Settings::load(&dir).guest_name, FALLBACK_USERNAME);

        std::fs::write(Settings::path(&dir), br#"{"compat_mode": true}"#).unwrap();
        let s = Settings::load(&dir);
        assert!(s.compat_mode);
        assert_eq!(s.guest_name, FALLBACK_USERNAME);
    }

    #[test]
    fn guest_name_is_cleaned() {
        let s = Settings {
            guest_name: "  \u{7}  ".into(),
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.guest_name, FALLBACK_USERNAME);

        let s = Settings {
            guest_name: format!(" {} ", "a".repeat(100)),
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.guest_name.len(), 32);
    }

    #[test]
    fn history_is_newest_first_without_duplicates_and_limited() {
        let mut s = Settings::default();
        s.remember_played("a", None, "1".into());
        s.remember_played("b", Some("B".into()), "2".into());
        s.remember_played("a", Some("A".into()), "3".into());
        let order: Vec<&str> = s.history.iter().map(|h| h.address.as_str()).collect();
        assert_eq!(order, ["a", "b"]);
        assert_eq!(s.history[0].name.as_deref(), Some("A"));

        for i in 0..50 {
            s.remember_played(&format!("s{i}"), None, i.to_string());
        }
        assert_eq!(s.history.len(), HISTORY_LIMIT);
        assert_eq!(s.history[0].address, "s49");
    }

    #[test]
    fn play_time_accumulates_and_survives_replaying() {
        let mut s = Settings::default();
        s.remember_played("a", None, "1".into());
        s.add_play_time("a", 60);
        s.add_play_time("a", 30);
        s.add_play_time("unknown", 999);
        s.remember_played("b", None, "2".into());
        s.remember_played("a", None, "3".into());
        assert_eq!(s.history[0].address, "a");
        assert_eq!(s.history[0].play_seconds, 90);
        assert_eq!(s.history[1].play_seconds, 0);
    }

    #[test]
    fn the_account_is_remembered_per_server() {
        let mut s = Settings::default();
        s.remember_played("a", None, "1".into());
        s.remember_account("a", "user-1");
        s.remember_account("unknown", "x");
        s.remember_played("b", None, "2".into());
        s.remember_account("b", "");
        s.remember_played("a", None, "3".into());
        let a = s.history.iter().find(|h| h.address == "a").unwrap();
        let b = s.history.iter().find(|h| h.address == "b").unwrap();
        assert_eq!(
            a.account.as_deref(),
            Some("user-1"),
            "survives playing again"
        );
        assert_eq!(b.account.as_deref(), Some(""), "guest is a choice too");
        assert_eq!(s.history.len(), 2);
    }

    #[test]
    fn colours_and_fonts_are_cleaned() {
        let s = Settings {
            font: "Comic Sans MS; } body { display:none".into(),
            custom_theme: CustomTheme {
                background: "#abcdef".into(),
                accent: "red".into(),
                foreground: "#12345".into(),
                popup: "url(javascript:1)".into(),
                gradient_start: "#00FF00".into(),
                gradient_end: "".into(),
            },
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.font, "Comic Sans MS  body  displaynone");
        assert_eq!(s.custom_theme.background, "#ABCDEF");
        assert_eq!(s.custom_theme.accent, CustomTheme::default().accent);
        assert_eq!(s.custom_theme.foreground, CustomTheme::default().foreground);
        assert_eq!(s.custom_theme.popup, CustomTheme::default().popup);
        assert_eq!(s.custom_theme.gradient_start, "#00FF00");
        assert_eq!(
            s.custom_theme.gradient_end,
            CustomTheme::default().gradient_end
        );
    }

    #[test]
    fn unusable_proxies_are_dropped() {
        let ok = Settings {
            proxy: " socks5://127.0.0.1:1080 ".into(),
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(ok.proxy, "socks5://127.0.0.1:1080");
        let bad = Settings {
            proxy: "not a proxy".into(),
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(bad.proxy, "");
    }

    #[test]
    fn pack_ids_are_plain_unique_names() {
        let s = Settings {
            enabled_packs: vec![
                "a.zip".into(),
                "../evil.zip".into(),
                "dir/x.zip".into(),
                "a.zip".into(),
                "".into(),
                "b.zip".into(),
            ],
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(s.enabled_packs, ["a.zip", "b.zip"]);
    }

    #[test]
    fn raising_a_favorite_moves_it_to_the_top() {
        let mut s = Settings::default();
        for a in ["a", "b", "c"] {
            s.toggle_favorite(a);
        }
        s.raise_favorite("c");
        s.raise_favorite("missing");
        assert_eq!(s.favorites, ["c", "a", "b"]);
    }

    #[test]
    fn old_settings_files_still_load() {
        // A file from before these options existed.
        let json = r#"{"guest_name":"X","theme":"ocean","favorites":["a"]}"#;
        let s: Settings = serde_json::from_str(json).unwrap();
        assert_eq!(s.guest_name, "X");
        assert!(!s.theme_gradient && s.columns.players && !s.columns.ping);
        assert_eq!(s.favorites, ["a"]);
        assert_eq!(s.hubs.len(), 1);
        assert_eq!(s.extra_args, "");
        assert!(s.enabled_patches.is_empty());
        assert!(s.discord_rpc);
    }

    #[test]
    fn hubs_and_extra_args_are_sanitized() {
        let s = Settings {
            hubs: vec![
                HubSetting {
                    url: "https://hub.corvax.fun".into(),
                    enabled: true,
                },
                HubSetting {
                    url: "https://hub.corvax.fun/".into(), // duplicate
                    enabled: false,
                },
                HubSetting {
                    url: "invalid-url".into(),
                    enabled: true,
                },
            ],
            extra_args: "  --cvar display.vsync=false \0 \r\n ".into(),
            ..Settings::default()
        }
        .sanitized();

        assert_eq!(s.hubs.len(), 1);
        assert_eq!(s.hubs[0].url, "https://hub.corvax.fun/");
        assert!(s.hubs[0].enabled);
        assert_eq!(s.extra_args, "--cvar display.vsync=false");

        // Empty hubs list gets restored to default
        let empty_hubs = Settings {
            hubs: vec![],
            ..Settings::default()
        }
        .sanitized();
        assert_eq!(empty_hubs.hubs.len(), 1);
        assert_eq!(empty_hubs.hubs[0].url, bebra_core::hub::OFFICIAL_HUB_URL);
    }

    #[test]
    fn favorites_toggle() {
        let mut s = Settings::default();
        assert!(s.toggle_favorite("a"));
        assert!(s.toggle_favorite("b"));
        assert!(!s.toggle_favorite("a"));
        assert_eq!(s.favorites, ["b"]);
    }

    #[test]
    fn optimization_profiles_and_config() {
        let mut s = Settings::default();
        assert_eq!(s.opt_profile, "balanced");
        let cfg = s.optimization_config();
        assert_eq!(cfg.profile, "balanced");
        assert!(cfg.server_gc);
        assert!(!cfg.fast_lighting);
        assert!(!cfg.unlock_fps);
        assert!(cfg.high_priority);

        s.opt_profile = "potato".into();
        let cfg_potato = s.optimization_config();
        assert_eq!(cfg_potato.profile, "potato");
        assert!(cfg_potato.fast_lighting);
        assert!(cfg_potato.unlock_fps);
        assert_eq!(cfg_potato.light_resolution, Some(0.35));

        s.opt_profile = "iskander".into();
        let cfg_iskander = s.optimization_config();
        assert_eq!(cfg_iskander.profile, "iskander");
        assert!(cfg_iskander.fast_lighting);
        assert!(cfg_iskander.unlock_fps);
        assert_eq!(cfg_iskander.light_resolution, Some(0.2));

        s.opt_profile = "max_fps".into();
        let cfg_fps = s.optimization_config();
        assert_eq!(cfg_fps.profile, "max_fps");
        assert!(!cfg_fps.fast_lighting);
        assert!(cfg_fps.unlock_fps);

        s.opt_profile = "custom".into();
        s.opt_server_gc = false;
        s.opt_fast_lighting = true;
        s.opt_unlock_fps = true;
        s.opt_high_priority = false;
        let cfg_custom = s.optimization_config();
        assert_eq!(cfg_custom.profile, "custom");
        assert!(!cfg_custom.server_gc);
        assert!(cfg_custom.fast_lighting);
        assert!(cfg_custom.unlock_fps);
        assert!(!cfg_custom.high_priority);

        s.opt_profile = "invalid_name".into();
        let san = s.sanitized();
        assert_eq!(san.opt_profile, "balanced");
    }

    #[test]
    fn background_and_client_settings_sanitized() {
        let s = Settings {
            bg_type: "unknown_mode".into(),
            bg_custom_name: Some("  ../bad/path\\name.png \0 ".into()),
            bg_dim: 150,
            bg_blur: 50,
            client_window_mode: "invalid".into(),
            client_ui_scale: "3.5".into(),
            client_max_fps: 1000,
            auto_refresh_interval: 999,
            ..Settings::default()
        }
        .sanitized();

        assert_eq!(s.bg_type, "space");
        assert_eq!(s.bg_custom_name.as_deref(), Some("..badpathname.png"));
        assert_eq!(s.bg_dim, 95);
        assert_eq!(s.bg_blur, 30);
        assert_eq!(s.client_window_mode, "default");
        assert_eq!(s.client_ui_scale, "auto");
        assert_eq!(s.client_max_fps, 360);
        assert_eq!(s.auto_refresh_interval, 0);
    }

    #[test]
    fn server_notes_sanitization_and_management() {
        let mut s = Settings::default();
        s.set_server_note("ss14s://corvax.example", "  Главный инженер на смене  ");
        assert_eq!(
            s.server_notes.get("ss14s://corvax.example").map(String::as_str),
            Some("Главный инженер на смене")
        );

        // Blank note removes the entry
        s.set_server_note("ss14s://corvax.example", "   ");
        assert!(!s.server_notes.contains_key("ss14s://corvax.example"));

        // Sanitization trims, drops empty/invalid addresses and limits length
        s.server_notes.insert("  ss14s://test.org  ".into(), "  Заметка \0 с нулём  ".into());
        s.server_notes.insert("   ".into(), "валидный текст".into());
        let san = s.sanitized();
        assert_eq!(
            san.server_notes.get("ss14s://test.org").map(String::as_str),
            Some("Заметка  с нулём")
        );
        assert!(!san.server_notes.contains_key("   "));
    }

    #[test]
    fn moving_and_editing_favorites() {
        let mut s = Settings::default();
        s.toggle_favorite("a");
        s.toggle_favorite("b");
        s.toggle_favorite("c");
        assert_eq!(s.favorites, ["a", "b", "c"]);

        s.set_favorite_name("a", Some("Alpha".into()));
        assert_eq!(s.favorite_names.get("a").map(String::as_str), Some("Alpha"));

        // Move b down
        s.move_favorite("b", 1);
        assert_eq!(s.favorites, ["a", "c", "b"]);

        // Move b up twice
        s.move_favorite("b", -2);
        assert_eq!(s.favorites, ["b", "a", "c"]);

        // Edit favorite a -> a2 with new name
        s.edit_favorite("a", "a2", Some("Alpha 2".into()));
        assert_eq!(s.favorites, ["b", "a2", "c"]);
        assert_eq!(s.favorite_names.get("a2").map(String::as_str), Some("Alpha 2"));

        // Sanitization of favorite names and launcher UI scale
        s.favorite_names.insert("  ss14s://fav.org  ".into(), "  Имя \0 с нулём  ".into());
        s.favorite_names.insert("   ".into(), "имя".into());
        s.launcher_ui_scale = 300;
        let san = s.sanitized();
        assert_eq!(
            san.favorite_names.get("ss14s://fav.org").map(String::as_str),
            Some("Имя  с нулём")
        );
        assert!(!san.favorite_names.contains_key("   "));
        assert_eq!(san.launcher_ui_scale, 200);
    }

    fn tempdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!("bebra-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct RecommendedHub {
    pub url: String,
    pub name: String,
}

pub fn recommended_hubs() -> Vec<RecommendedHub> {
    vec![
        RecommendedHub {
            url: "https://hub.spacestation14.com/".to_string(),
            name: "Wizards' Den".to_string(),
        },
        RecommendedHub {
            url: "https://hub.singularity14.co.uk/".to_string(),
            name: "Singularity".to_string(),
        },
        RecommendedHub {
            url: "https://hub.simplestation.org/".to_string(),
            name: "SimpleStation".to_string(),
        },
        RecommendedHub {
            url: "https://cdn.spacestationmultiverse.com/hub/".to_string(),
            name: "Multiverse".to_string(),
        },
    ]
}
