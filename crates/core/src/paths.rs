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
    /// `%LOCALAPPDATA%\BebraLauncher`, or `<exe_dir>\data` if `portable.txt` / `.portable` exists
    /// next to the executable.
    pub fn default_windows() -> Result<Self> {
        let exe = std::env::current_exe().ok();
        let appdata = std::env::var_os("LOCALAPPDATA");
        Self::resolve_root(exe.as_deref(), appdata.as_deref().map(Path::new))
    }

    /// Resolves the data directory, giving precedence to a portable trigger file beside the executable.
    pub fn resolve_root(exe_path: Option<&Path>, local_app_data: Option<&Path>) -> Result<Self> {
        if let Some(exe) = exe_path {
            if let Some(dir) = exe.parent() {
                if dir.join("portable.txt").is_file() || dir.join(".portable").is_file() {
                    return Ok(Self::at(dir.join("data")));
                }
            }
        }
        let base = local_app_data
            .filter(|v| !v.as_os_str().is_empty())
            .ok_or_else(|| Error::Other("LOCALAPPDATA is not set".into()))?;
        Ok(Self::at(base.join("BebraLauncher")))
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
            // Both are used right away by downloads and the pack overlay; creating them here
            // keeps every later step from having to think about it.
            self.scratch(),
            self.overlays(),
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
        assert!(
            d.scratch().is_dir() && d.overlays().is_dir(),
            "downloads and the pack overlay need tmp/ and overlays/"
        );
    }

    #[test]
    fn portable_mode_detection() {
        let tmp = tempfile::tempdir().unwrap();
        let exe = tmp.path().join("BebraLauncher.exe");
        std::fs::write(&exe, b"").unwrap();

        // Without portable.txt, uses LOCALAPPDATA
        let d = DataDirs::resolve_root(Some(&exe), Some(Path::new("C:\\AppData\\Local"))).unwrap();
        assert_eq!(d.root(), Path::new("C:\\AppData\\Local\\BebraLauncher"));

        // With portable.txt next to exe, uses data/ folder beside exe
        std::fs::write(tmp.path().join("portable.txt"), b"").unwrap();
        let d_portable =
            DataDirs::resolve_root(Some(&exe), Some(Path::new("C:\\AppData\\Local"))).unwrap();
        assert_eq!(d_portable.root(), tmp.path().join("data"));
    }
}
