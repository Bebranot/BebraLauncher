//! Client-side patch assemblies (.dll) for plugins and modifications such as translators.
//!
//! A patch is executable code loaded into the game process, so the launcher records the SHA-256
//! of every file it installed (`patches/manifest.json`) and checks it again before a launch.
//! A file that changed or appeared behind the launcher's back is not loaded silently (P1-2).

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

use crate::hash::{sha256, to_hex};
use crate::paths::DataDirs;
use crate::Result;

/// The hash file lives next to the patches, not in the database.
const MANIFEST: &str = "manifest.json";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchInfo {
    /// File name inside the patches folder (identifies the patch in settings).
    pub id: String,
    pub name: String,
    pub file_size: u64,
    /// SHA-256 of the file (hex), as it is now.
    pub sha256: String,
    /// Whether the recorded hash matches the file. A patch without a record (installed by an
    /// older version) is reported as unverified once and recorded on the next launch.
    pub verified: bool,
}

/// What the launcher remembers about one installed patch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchRecord {
    pub sha256: String,
    pub size: u64,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
struct Manifest {
    #[serde(default)]
    patches: BTreeMap<String, PatchRecord>,
}

fn manifest_path(dirs: &DataDirs) -> PathBuf {
    dirs.patches().join(MANIFEST)
}

fn read_manifest(dirs: &DataDirs) -> Manifest {
    std::fs::read(manifest_path(dirs))
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default()
}

fn write_manifest(dirs: &DataDirs, manifest: &Manifest) -> Result<()> {
    let folder = dirs.patches();
    std::fs::create_dir_all(&folder)?;
    let target = manifest_path(dirs);
    let text = serde_json::to_vec_pretty(manifest)
        .map_err(|e| crate::Error::Other(format!("patches manifest: {e}")))?;
    crate::fsutil::write_atomic(&target, &text)?;
    Ok(())
}

/// Records the hash of a file the launcher has just installed. Returns what was recorded.
pub fn record_installed(dirs: &DataDirs, id: &str, data: &[u8]) -> Result<PatchRecord> {
    let mut manifest = read_manifest(dirs);
    let record = PatchRecord {
        sha256: to_hex(&sha256(data)),
        size: data.len() as u64,
    };
    manifest.patches.insert(id.to_string(), record.clone());
    write_manifest(dirs, &manifest)?;
    Ok(record)
}

/// Forgets a patch that was deleted.
pub fn forget(dirs: &DataDirs, id: &str) -> Result<()> {
    let mut manifest = read_manifest(dirs);
    if manifest.patches.remove(id).is_some() {
        write_manifest(dirs, &manifest)?;
    }
    Ok(())
}

fn hash_of(path: &Path) -> Option<(String, u64)> {
    let data = std::fs::read(path).ok()?;
    Some((to_hex(&sha256(&data)), data.len() as u64))
}

/// Scans the patches directory for `.dll` files.
pub fn scan_patches(dirs: &DataDirs) -> Result<Vec<PatchInfo>> {
    let folder = dirs.patches();
    if !folder.is_dir() {
        return Ok(Vec::new());
    }
    let manifest = read_manifest(dirs);
    let mut list = Vec::new();
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                if ext.eq_ignore_ascii_case("dll") {
                    let Some(file_name) = path.file_name() else {
                        continue;
                    };
                    let Some(stem) = path.file_stem() else {
                        continue;
                    };
                    let id = file_name.to_string_lossy().to_string();
                    let metadata = entry.metadata()?;
                    let name = stem.to_string_lossy().to_string();
                    let (sha256, size) = hash_of(&path).unwrap_or_default();
                    let verified = manifest
                        .patches
                        .get(&id)
                        .is_some_and(|r| r.sha256 == sha256 && r.size == size);
                    list.push(PatchInfo {
                        id,
                        name,
                        file_size: metadata.len(),
                        sha256,
                        verified,
                    });
                }
            }
        }
    }
    list.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(list)
}

/// Result of turning the enabled ids into files.
#[derive(Debug, Clone, Default)]
pub struct ResolvedPatches {
    /// Files that are safe to load: their hash matches what was installed.
    pub paths: Vec<PathBuf>,
    /// Files that changed after installation (or were replaced) and are therefore not loaded.
    pub rejected: Vec<String>,
    /// Files with no recorded hash (installed by an older version); loaded, but the UI warns.
    pub unverified: Vec<String>,
}

/// Resolves enabled patch IDs into existing full paths and checks them against the manifest.
///
/// A file whose hash does not match the recorded one is *not* returned in `paths`: something
/// replaced it after the launcher installed it. A file with no record is accepted once and its
/// hash is recorded, so an existing installation keeps working after an upgrade.
pub fn resolve_verified(dirs: &DataDirs, enabled: &[String]) -> ResolvedPatches {
    let folder = dirs.patches();
    let mut manifest = read_manifest(dirs);
    let mut dirty = false;
    let mut out = ResolvedPatches::default();
    for id in enabled {
        let clean = id.trim();
        if !valid_id(clean) {
            continue;
        }
        let path = folder.join(clean);
        if !path.is_file() {
            continue;
        }
        let Some((sha256, size)) = hash_of(&path) else {
            out.rejected.push(clean.to_string());
            continue;
        };
        match manifest.patches.get(clean) {
            Some(record) if record.sha256 == sha256 && record.size == size => {
                out.paths.push(path);
            }
            Some(_) => out.rejected.push(clean.to_string()),
            None => {
                manifest
                    .patches
                    .insert(clean.to_string(), PatchRecord { sha256, size });
                dirty = true;
                out.unverified.push(clean.to_string());
                out.paths.push(path);
            }
        }
    }
    if dirty && write_manifest(dirs, &manifest).is_err() {
        // The check still happened; the record is simply written again next time.
        let known: Vec<String> = manifest
            .patches
            .keys()
            .filter(|k| !out.unverified.contains(k))
            .cloned()
            .collect();
        out.unverified.extend(known);
    }
    out
}

/// Checks that an id is a plain file name with the `.dll` extension.
pub fn valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 200
        && !id.contains(['/', '\\', ':'])
        && !id.contains("..")
        && Path::new(id).file_name() == Some(std::ffi::OsStr::new(id))
        && Path::new(id)
            .extension()
            .and_then(|e| e.to_str())
            .is_some_and(|e| e.eq_ignore_ascii_case("dll"))
}

/// Resolves enabled patch IDs into existing full paths without checking hashes.
pub fn resolve_patches(dirs: &DataDirs, enabled: &[String]) -> Vec<PathBuf> {
    let folder = dirs.patches();
    enabled
        .iter()
        .filter(|id| valid_id(id.trim()))
        .map(|id| folder.join(id.trim()))
        .filter(|p| p.is_file())
        .collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dirs() -> (tempfile::TempDir, DataDirs) {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDirs::at(tmp.path());
        dirs.ensure().unwrap();
        (tmp, dirs)
    }

    #[test]
    fn scan_and_resolve_patches() {
        let (_tmp, dirs) = dirs();

        let patch_file = dirs.patches().join("Translator.dll");
        std::fs::write(&patch_file, b"dummy dll content").unwrap();

        let ignore_file = dirs.patches().join("readme.txt");
        std::fs::write(&ignore_file, b"not a patch").unwrap();

        let list = scan_patches(&dirs).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "Translator.dll");
        assert_eq!(list[0].name, "Translator");
        assert_eq!(list[0].file_size, 17);
        assert!(!list[0].verified, "nothing was recorded yet");

        let resolved = resolve_patches(
            &dirs,
            &[
                "Translator.dll".into(),
                "missing.dll".into(),
                "../bad.dll".into(),
                "C:evil.dll".into(),
                "readme.txt".into(),
            ],
        );
        assert_eq!(resolved.len(), 1);
        assert_eq!(resolved[0], patch_file);
    }

    #[test]
    fn a_changed_patch_is_not_loaded() {
        let (_tmp, dirs) = dirs();
        let file = dirs.patches().join("Translator.dll");
        std::fs::write(&file, b"the original patch").unwrap();
        record_installed(&dirs, "Translator.dll", b"the original patch").unwrap();

        let enabled = ["Translator.dll".to_string()];
        let first = resolve_verified(&dirs, &enabled);
        assert_eq!(first.paths, [file.as_path()]);
        assert!(first.rejected.is_empty());
        assert!(first.unverified.is_empty());

        // Something replaced the file after the launcher installed it.
        std::fs::write(&file, b"a different dll").unwrap();
        let second = resolve_verified(&dirs, &enabled);
        assert!(second.paths.is_empty(), "a changed dll must not load");
        assert_eq!(second.rejected, ["Translator.dll"]);
    }

    #[test]
    fn a_patch_without_a_record_is_accepted_and_recorded() {
        let (_tmp, dirs) = dirs();
        let file = dirs.patches().join("Legacy.dll");
        std::fs::write(&file, b"installed by an older version").unwrap();

        let enabled = ["Legacy.dll".to_string()];
        let first = resolve_verified(&dirs, &enabled);
        assert_eq!(first.paths, [file.as_path()]);
        assert_eq!(first.unverified, ["Legacy.dll"], "the UI warns once");
        assert!(first.rejected.is_empty());

        let second = resolve_verified(&dirs, &enabled);
        assert_eq!(second.paths.len(), 1);
        assert!(second.unverified.is_empty(), "now it has a recorded hash");

        forget(&dirs, "Legacy.dll").unwrap();
        assert!(!std::fs::read_to_string(manifest_path(&dirs))
            .unwrap()
            .contains("Legacy"));
    }
}
