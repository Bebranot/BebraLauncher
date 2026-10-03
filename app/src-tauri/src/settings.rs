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

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HistoryEntry {
    pub address: String,
    pub name: Option<String>,
    /// RFC 3339.
    pub last_played: String,
    /// Time spent in the game on this server, in seconds.
    #[serde(default)]
    pub play_seconds: u64,
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
        let play_seconds = self
            .history
            .iter()
            .find(|h| h.address == address)
            .map_or(0, |h| h.play_seconds);
        self.history.retain(|h| h.address != address);
        self.history.insert(
            0,
            HistoryEntry {
                address: address.to_string(),
                name,
                last_played: when,
                play_seconds,
            },
        );
        self.history.truncate(HISTORY_LIMIT);
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
        self
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
