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

/// Most files one dump may write. A real game has a few thousand; a hostile or broken build
/// should not fill the disk.
pub const MAX_DUMP_FILES: usize = 20_000;
/// Most bytes one dump may write, uncompressed.
pub const MAX_DUMP_BYTES: u64 = 256 * 1024 * 1024;

/// Removes a temporary dump folder unless the dump was moved into place.
struct TempDir<'a> {
    path: &'a Path,
    armed: bool,
}

impl Drop for TempDir<'_> {
    fn drop(&mut self) {
        if self.armed {
            let _ = std::fs::remove_dir_all(self.path);
        }
    }
}

/// Writes the `Locale/**/*.ftl` files of the newest installed version to `out_dir/Locale/…`,
/// replacing an earlier dump. Errors if no version is installed.
///
/// The new dump is built in `Locale.tmp` and only moved over `Locale` when it is complete, so an
/// error in the middle (or a crash) leaves the previous dump untouched.
pub fn dump_locale(db: &ContentDb, out_dir: &Path) -> Result<LocaleDump> {
    dump_locale_with_limits(db, out_dir, MAX_DUMP_FILES, MAX_DUMP_BYTES)
}

/// [`dump_locale`] with explicit limits, so a test need not write 256 MB.
fn dump_locale_with_limits(
    db: &ContentDb,
    out_dir: &Path,
    max_files: usize,
    max_bytes: u64,
) -> Result<LocaleDump> {
    let version = db.list_versions()?.into_iter().next().ok_or_else(|| {
        Error::Other("no game version is installed yet: connect to a server once".into())
    })?;

    let target = out_dir.join("Locale");
    let tmp = out_dir.join("Locale.tmp");
    // A leftover from an earlier crash is ours to remove.
    if tmp.exists() {
        std::fs::remove_dir_all(&tmp)?;
    }
    std::fs::create_dir_all(&tmp)?;
    let mut guard = TempDir {
        path: &tmp,
        armed: true,
    };

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
        // `relative` starts with `Locale/`; the dump folder itself is the one being built.
        let Some(relative) = relative.strip_prefix("Locale").ok() else {
            continue;
        };
        let Some(data) = db.read_file(version.id, path)? else {
            continue;
        };
        if files + 1 > max_files {
            return Err(Error::Other(format!(
                "the game has more than {max_files} Locale files: refusing to dump them"
            )));
        }
        if bytes + data.len() as u64 > max_bytes {
            return Err(Error::Other(format!(
                "the Locale files are larger than {} MB: refusing to dump them",
                max_bytes >> 20
            )));
        }
        let dest = tmp.join(relative);
        if let Some(parent) = dest.parent() {
            std::fs::create_dir_all(parent)?;
        }
        std::fs::write(&dest, &data)?;
        files += 1;
        bytes += data.len() as u64;
    }

    // Swap the finished dump in. The old one is moved aside first, so a failed rename can put it
    // back and never leaves the player without a dump.
    let old = out_dir.join("Locale.old");
    if old.exists() {
        std::fs::remove_dir_all(&old)?;
    }
    if target.exists() {
        std::fs::rename(&target, &old)?;
    }
    if let Err(e) = std::fs::rename(&tmp, &target) {
        if old.exists() {
            let _ = std::fs::rename(&old, &target);
        }
        return Err(e.into());
    }
    guard.armed = false;
    let _ = std::fs::remove_dir_all(&old);

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
    fn a_dump_over_the_limit_keeps_the_previous_one() {
        let tmp = tempfile::tempdir().unwrap();
        let db = ContentDb::open(&tmp.path().join("content.db")).unwrap();
        install(&db, "old", &[("Locale/en-US/a.ftl", b"hello = Hello")]);
        let out = tmp.path().join("dump");
        dump_locale(&db, &out).unwrap();
        let before = std::fs::read_to_string(out.join("Locale/en-US/a.ftl")).unwrap();

        // A newer build with more files than the limit allows.
        install(
            &db,
            "new",
            &[
                ("Locale/en-US/a.ftl", b"hello = Hi"),
                ("Locale/en-US/b.ftl", b"menu = Menu"),
                ("Locale/en-US/c.ftl", b"bye = Bye"),
            ],
        );
        assert!(dump_locale_with_limits(&db, &out, 2, MAX_DUMP_BYTES).is_err());
        assert_eq!(
            std::fs::read_to_string(out.join("Locale/en-US/a.ftl")).unwrap(),
            before,
            "the old dump is untouched"
        );
        assert!(!out.join("Locale.tmp").exists(), "no half-written dump");

        // The same for the byte limit.
        let too_small = MAX_DUMP_BYTES.min(5);
        assert!(dump_locale_with_limits(&db, &out, MAX_DUMP_FILES, too_small).is_err());
        assert_eq!(
            std::fs::read_to_string(out.join("Locale/en-US/a.ftl")).unwrap(),
            before
        );

        // Within the limits it replaces the old dump.
        let ok = dump_locale_with_limits(&db, &out, 3, MAX_DUMP_BYTES).unwrap();
        assert_eq!(ok.files, 3);
        assert!(out.join("Locale/en-US/c.ftl").is_file());
    }

    #[test]
    fn nothing_installed_is_an_error() {
        let tmp = tempfile::tempdir().unwrap();
        let db = ContentDb::open(&tmp.path().join("content.db")).unwrap();
        assert!(dump_locale(&db, &tmp.path().join("dump")).is_err());
    }
}
