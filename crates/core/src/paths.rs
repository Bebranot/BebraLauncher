//! Where the launcher keeps its data.
//!
//! We use our own folder (not the official launcher's `Space Station 14\launcher`), so the two
//! launchers never touch each other's databases.

use std::path::{Path, PathBuf};

use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct DataDirs {
    root: PathBuf,
}

impl DataDirs {
    /// `%LOCALAPPDATA%\BebraLauncher`.
    pub fn default_windows() -> Result<Self> {
        let base = std::env::var_os("LOCALAPPDATA")
            .filter(|v| !v.is_empty())
            .ok_or_else(|| Error::Other("LOCALAPPDATA is not set".into()))?;
        Ok(Self::at(Path::new(&base).join("BebraLauncher")))
    }

    pub fn at(root: impl Into<PathBuf>) -> Self {
        Self { root: root.into() }
    }

    pub fn root(&self) -> &Path {
        &self.root
    }

    pub fn content_db(&self) -> PathBuf {
        self.root.join("content.db")
    }

    /// Engine builds: `<version>.zip` and `<version>.sig`.
    pub fn engines(&self) -> PathBuf {
        self.root.join("engines")
    }

    /// Engine modules: `<name>/<version>/`.
    pub fn modules(&self) -> PathBuf {
        self.root.join("modules")
    }

    /// Temporary files (zip downloads while they are unpacked).
    pub fn scratch(&self) -> PathBuf {
        self.root.join("tmp")
    }

    /// Text packs (`.zip` files the player puts here).
    pub fn packs(&self) -> PathBuf {
        self.root.join("packs")
    }

    /// Client patches (`.dll` files the player puts here).
    pub fn patches(&self) -> PathBuf {
        self.root.join("patches")
    }

    /// Custom backgrounds / wallpapers uploaded by the player.
    pub fn backgrounds(&self) -> PathBuf {
        self.root.join("backgrounds")
    }

    /// The merged overlay of the enabled packs, as handed to the Loader.
    pub fn overlays(&self) -> PathBuf {
        self.root.join("overlays")
    }

    pub fn logs(&self) -> PathBuf {
        self.root.join("logs")
    }

    pub fn ensure(&self) -> Result<()> {
        for dir in [
            self.engines(),
            self.modules(),
            self.logs(),
            self.packs(),
            self.patches(),
            self.backgrounds(),
        ] {
            std::fs::create_dir_all(dir)?;
        }
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn layout() {
        let d = DataDirs::at("C:/data");
        assert_eq!(d.content_db(), Path::new("C:/data/content.db"));
        assert_eq!(d.engines(), Path::new("C:/data/engines"));
    }

    #[test]
    fn ensure_creates_folders() {
        let tmp = tempfile::tempdir().unwrap();
        let d = DataDirs::at(tmp.path().join("x"));
        d.ensure().unwrap();
        assert!(d.engines().is_dir() && d.modules().is_dir() && d.logs().is_dir());
    }
}
