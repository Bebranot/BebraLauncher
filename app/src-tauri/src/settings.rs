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
    /// The account chosen last (user id); preselected next time.
    pub last_account: Option<String>,
}

impl Default for Settings {
    fn default() -> Self {
        Self {
            guest_name: FALLBACK_USERNAME.to_string(),
            compat_mode: false,
            last_account: None,
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

        let s = Settings {
            guest_name: "Bebra".into(),
            compat_mode: true,
            last_account: Some("x".into()),
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

    fn tempdir() -> PathBuf {
        let p = std::env::temp_dir().join(format!("bebra-settings-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&p).unwrap();
        p
    }
}
