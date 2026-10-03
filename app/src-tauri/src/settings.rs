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
    /// `auto` (follow Windows), `ru` or `en`.
    pub language: String,
    /// The account chosen last (user id); preselected next time.
    /// An empty string means the player chose to play as a guest; `None`: never chosen.
    pub last_account: Option<String>,
    /// Addresses of favourite servers.
    pub favorites: Vec<String>,
    /// Servers played recently, newest first.
    pub history: Vec<HistoryEntry>,
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
            minimize_on_play: true,
            theme: "dark".to_string(),
            theme_gradient: true,
            theme_decor: true,
            custom_theme: CustomTheme::default(),
            font: String::new(),
            columns: Columns::default(),
            enabled_packs: Vec::new(),
            ignore_redial: false,
            language: "auto".to_string(),
            last_account: None,
            favorites: Vec::new(),
            history: Vec::new(),
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
            false
        } else {
            self.favorites.push(address.to_string());
            true
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

        // Pack ids are plain file names, each at most once.
        let mut seen = std::collections::HashSet::new();
        self.enabled_packs.retain(|id| {
            !id.is_empty()
                && id.len() <= 200
                && !id.contains(['/', '\\'])
                && seen.insert(id.clone())
        });
        self
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
        assert!(s.theme_gradient && s.columns.players && !s.columns.ping);
        assert_eq!(s.favorites, ["a"]);
    }

    #[test]
    fn favorites_toggle() {
        let mut s = Settings::default();
        assert!(s.toggle_favorite("a"));
        assert!(s.toggle_favorite("b"));
        assert!(!s.toggle_favorite("a"));
        assert_eq!(s.favorites, ["b"]);
    }

    fn tempdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!("bebra-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
