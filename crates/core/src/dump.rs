//! Reading installed game files out for the player: the texts of the game, as a starting point
//! for a translation. Nothing is changed in the installed game.

use std::path::{Path, PathBuf};

use crate::content_db::ContentDb;
use crate::{Error, Result};

#[derive(Debug, Clone)]
pub struct LocaleDump {
    /// The version the texts were taken from.
    pub version_id: i64,
    pub files: usize,
    pub bytes: u64,
    pub dir: PathBuf,
}

/// A path from the manifest, checked so that it stays inside the output folder.
fn safe_relative(path: &str) -> Option<PathBuf> {
    if path.is_empty() || path.starts_with('/') || path.contains('\\') || path.contains(':') {
        return None;
    }
    let mut out = PathBuf::new();
    for part in path.split('/') {
        if part.is_empty() || part == "." || part == ".." {
            return None;
        }
        out.push(part);
    }
    Some(out)
}

/// Writes the `Locale/**/*.ftl` files of the newest installed version to `out_dir/Locale/…`,
/// replacing an earlier dump. Errors if no version is installed.
pub fn dump_locale(db: &ContentDb, out_dir: &Path) -> Result<LocaleDump> {
    let version = db.list_versions()?.into_iter().next().ok_or_else(|| {
        Error::Other("no game version is installed yet: connect to a server once".into())
    })?;

    let target = out_dir.join("Locale");
    if target.exists() {
        std::fs::remove_dir_all(&target)?;
    }
    std::fs::create_dir_all(&target)?;

    let mut files = 0usize;
    let mut bytes = 0u64;
    for entry in db.manifest_entries(version.id)? {
        let path = entry.path.as_str();
        if !path.starts_with("Locale/") || !path.ends_with(".ftl") {
            continue;
        }
        let Some(relative) = safe_relative(path) else {
            continue;
        };
        let Some(data) = db.read_file(version.id, path)? else {
            continue;
        };
        let dest = out_dir.join(relative);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, &data)?;
        files += 1;
        bytes += data.len() as u64;
    }

    Ok(LocaleDump {
        version_id: version.id,
        files,
        bytes,
        dir: target,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::hash::{blake2b256, to_hex};
    use crate::manifest::Manifest;

    fn install(db: &ContentDb, fork_version: &str, files: &[(&str, &[u8])]) -> i64 {
        let version = db.create_version("fork", fork_version).unwrap();
        let mut text = String::from("Robust Content Manifest 1\n");
        for (path, data) in files {
            let hash = blake2b256(data);
            db.store_blob(&hash, data).unwrap();
            text.push_str(&format!("{} {path}\n", to_hex(&hash)));
        }
        let manifest = Manifest::parse(text.as_bytes()).unwrap();
        db.fill_manifest(version, &manifest).unwrap();
        version
    }

    #[test]
    fn safe_paths_only() {
        assert!(safe_relative("Locale/ru-RU/a.ftl").is_some());
        for bad in ["", "/x", "../x", "a/../b", "a//b", "C:/x", "a\\b", "./a"] {
            assert!(safe_relative(bad).is_none(), "{bad:?}");
        }
    }

    #[test]
    fn dumps_only_the_locale_texts_of_the_newest_version() {
        let tmp = tempfile::tempdir().unwrap();
        let db = ContentDb::open(&tmp.path().join("content.db")).unwrap();
        install(&db, "old", &[("Locale/en-US/old.ftl", b"old = 1")]);
        let newest = install(
            &db,
            "new",
            &[
                ("Locale/en-US/a.ftl", b"hello = Hello"),
                ("Locale/en-US/ui/b.ftl", b"menu = Menu"),
                ("Textures/x.png", b"png"),
                ("Locale/en-US/readme.txt", b"not a text pack file"),
            ],
        );

        let out = tmp.path().join("dump");
        let report = dump_locale(&db, &out).unwrap();
        assert_eq!(report.version_id, newest);
        assert_eq!(report.files, 2);
        assert_eq!(
            std::fs::read_to_string(out.join("Locale/en-US/ui/b.ftl")).unwrap(),
            "menu = Menu"
        );
        assert!(!out.join("Textures").exists());
        assert!(!out.join("Locale/en-US/readme.txt").exists());
        assert!(!out.join("Locale/en-US/old.ftl").exists());

        // Dumping again replaces the folder: nothing stale stays.
        std::fs::write(out.join("Locale/en-US/stale.ftl"), b"x").unwrap();
        dump_locale(&db, &out).unwrap();
        assert!(!out.join("Locale/en-US/stale.ftl").exists());
    }

    #[test]
    fn nothing_installed_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let db = ContentDb::open(&tmp.path().join("content.db")).unwrap();
        assert!(dump_locale(&db, &tmp.path().join("dump")).is_err());
    }
}
