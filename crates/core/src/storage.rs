//! What the launcher keeps on disk, and cleaning up what no installed version needs.

use std::collections::HashSet;
use std::path::Path;

use crate::content_db::cull::CullReport;
use crate::content_db::ContentDb;
use crate::paths::DataDirs;
use crate::updater;
use crate::Result;

/// Sizes in bytes.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct StorageReport {
    pub content_db: u64,
    pub engines: u64,
    pub modules: u64,
    pub scratch: u64,
    pub logs: u64,
}

impl StorageReport {
    pub fn total(&self) -> u64 {
        self.content_db + self.engines + self.modules + self.scratch + self.logs
    }
}

#[derive(Debug, Clone, Copy, Default)]
pub struct CleanReport {
    pub cull: CullReport,
    pub engines_removed: usize,
    pub modules_removed: usize,
    pub bytes_freed: u64,
}

/// Total size of a file or of everything under a folder (0 if it is not there).
pub fn size_of(path: &Path) -> u64 {
    let Ok(meta) = std::fs::symlink_metadata(path) else {
        return 0;
    };
    if meta.is_file() {
        return meta.len();
    }
    if !meta.is_dir() {
        return 0;
    }
    std::fs::read_dir(path)
        .map(|rd| rd.flatten().map(|e| size_of(&e.path())).sum())
        .unwrap_or(0)
}

pub fn report(dirs: &DataDirs) -> StorageReport {
    let db = dirs.content_db();
    let db_files: u64 = ["", "-wal", "-shm"]
        .iter()
        .map(|suffix| {
            let mut name = db.clone().into_os_string();
            name.push(suffix);
            size_of(Path::new(&name))
        })
        .sum();
    StorageReport {
        content_db: db_files,
        engines: size_of(&dirs.engines()),
        modules: size_of(&dirs.modules()),
        scratch: size_of(&dirs.scratch()),
        logs: size_of(&dirs.logs()),
    }
}

/// Removes a file or folder; returns how many bytes that freed (0 if it failed).
fn remove_quietly(path: &Path) -> u64 {
    let size = size_of(path);
    let ok = if path.is_dir() {
        std::fs::remove_dir_all(path).is_ok()
    } else {
        std::fs::remove_file(path).is_ok()
    };
    if ok {
        size
    } else {
        0
    }
}

/// Drops old versions the usual way, then the engines and modules that no remaining version
/// depends on, and the leftovers of unfinished downloads. Never call this while a game is
/// being prepared: its engine may not be recorded yet.
pub fn clean(dirs: &DataDirs, db: &ContentDb) -> Result<CleanReport> {
    let db_before = report(dirs).content_db;
    let cull = updater::cleanup(db)?;

    let deps = db.all_engine_dependencies()?;
    let engines_in_use: HashSet<&str> = deps
        .iter()
        .filter(|(m, _)| m == "Robust")
        .map(|(_, v)| v.as_str())
        .collect();
    let modules_in_use: HashSet<(&str, &str)> = deps
        .iter()
        .filter(|(m, _)| m != "Robust")
        .map(|(m, v)| (m.as_str(), v.as_str()))
        .collect();

    let mut out = CleanReport {
        cull,
        ..CleanReport::default()
    };

    if let Ok(rd) = std::fs::read_dir(dirs.engines()) {
        for entry in rd.flatten() {
            let path = entry.path();
            let name = path.file_name().and_then(|n| n.to_str()).unwrap_or("");
            // `<version>.zip` and `<version>.sig`
            let Some(version) = name
                .strip_suffix(".zip")
                .or_else(|| name.strip_suffix(".sig"))
            else {
                continue;
            };
            if engines_in_use.contains(version) {
                continue;
            }
            let freed = remove_quietly(&path);
            out.bytes_freed += freed;
            if freed > 0 && name.ends_with(".zip") {
                out.engines_removed += 1;
            }
        }
    }

    if let Ok(modules) = std::fs::read_dir(dirs.modules()) {
        for module in modules.flatten() {
            let module_name = module.file_name().to_string_lossy().into_owned();
            let Ok(versions) = std::fs::read_dir(module.path()) else {
                continue;
            };
            for version in versions.flatten() {
                let version_name = version.file_name().to_string_lossy().into_owned();
                if modules_in_use.contains(&(module_name.as_str(), version_name.as_str())) {
                    continue;
                }
                let freed = remove_quietly(&version.path());
                out.bytes_freed += freed;
                if freed > 0 {
                    out.modules_removed += 1;
                }
            }
        }
    }

    // Unpacked downloads in progress are never here between launches.
    if let Ok(rd) = std::fs::read_dir(dirs.scratch()) {
        for entry in rd.flatten() {
            out.bytes_freed += remove_quietly(&entry.path());
        }
    }

    // Give the space of removed rows back to the disk. Best effort: a running game holds
    // the database open, and then this is simply skipped.
    let conn = db.connection();
    let _ = conn.execute_batch("PRAGMA wal_checkpoint(TRUNCATE);");
    if cull.versions_removed > 0 || cull.blobs_removed > 0 || cull.interrupted_removed > 0 {
        let _ = conn.execute_batch("VACUUM;");
    }

    out.bytes_freed += db_before.saturating_sub(report(dirs).content_db);
    Ok(out)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn write(path: &Path, bytes: usize) {
        std::fs::create_dir_all(path.parent().unwrap()).unwrap();
        std::fs::write(path, vec![7u8; bytes]).unwrap();
    }

    #[test]
    fn sizes_add_up() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDirs::at(tmp.path());
        write(&dirs.engines().join("1.0.0.zip"), 100);
        write(&dirs.modules().join("M").join("1").join("a.dll"), 50);
        write(&dirs.logs().join("x.log"), 5);
        let r = report(&dirs);
        assert_eq!((r.engines, r.modules, r.logs), (100, 50, 5));
        assert_eq!(r.total(), 155);
        assert_eq!(size_of(&tmp.path().join("missing")), 0);
    }

    #[test]
    fn clean_removes_only_what_no_version_needs() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDirs::at(tmp.path());
        dirs.ensure().unwrap();
        let db = ContentDb::open(&dirs.content_db()).unwrap();

        let v = db.create_version("fork", "v1").unwrap();
        db.add_engine_dependency(v, "Robust", "100.0.0").unwrap();
        db.add_engine_dependency(v, "Robust.Client.WebView", "1.2.3")
            .unwrap();

        for ext in ["zip", "sig"] {
            write(&dirs.engines().join(format!("100.0.0.{ext}")), 10);
            write(&dirs.engines().join(format!("99.0.0.{ext}")), 10);
        }
        write(
            &dirs
                .modules()
                .join("Robust.Client.WebView")
                .join("1.2.3")
                .join("a"),
            20,
        );
        write(
            &dirs
                .modules()
                .join("Robust.Client.WebView")
                .join("0.9.0")
                .join("a"),
            20,
        );
        write(&dirs.scratch().join("half-downloaded.zip"), 30);

        let r = clean(&dirs, &db).unwrap();
        assert_eq!(r.engines_removed, 1);
        assert_eq!(r.modules_removed, 1);
        assert!(r.bytes_freed >= 20 + 20 + 30);

        assert!(dirs.engines().join("100.0.0.zip").is_file());
        assert!(dirs.engines().join("100.0.0.sig").is_file());
        assert!(!dirs.engines().join("99.0.0.zip").exists());
        assert!(!dirs.engines().join("99.0.0.sig").exists());
        assert!(dirs
            .modules()
            .join("Robust.Client.WebView/1.2.3/a")
            .is_file());
        assert!(!dirs.modules().join("Robust.Client.WebView/0.9.0").exists());
        assert!(!dirs.scratch().join("half-downloaded.zip").exists());
    }
}
