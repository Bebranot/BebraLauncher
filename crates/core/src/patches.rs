//! Client-side patch assemblies (.dll) for plugins and modifications such as translators.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::paths::DataDirs;
use crate::Result;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PatchInfo {
    /// File name inside the patches folder (identifies the patch in settings).
    pub id: String,
    pub name: String,
    pub file_size: u64,
}

/// Scans the patches directory for `.dll` files.
pub fn scan_patches(dirs: &DataDirs) -> Result<Vec<PatchInfo>> {
    let folder = dirs.patches();
    if !folder.is_dir() {
        return Ok(Vec::new());
    }
    let mut list = Vec::new();
    for entry in std::fs::read_dir(folder)? {
        let entry = entry?;
        let path = entry.path();
        if path.is_file() {
            if let Some(ext) = path.extension().and_then(|s| s.to_str()) {
                if ext.eq_ignore_ascii_case("dll") {
                    let id = path.file_name().unwrap().to_string_lossy().to_string();
                    let metadata = entry.metadata()?;
                    let name = path.file_stem().unwrap().to_string_lossy().to_string();
                    list.push(PatchInfo {
                        id,
                        name,
                        file_size: metadata.len(),
                    });
                }
            }
        }
    }
    list.sort_by(|a, b| a.id.cmp(&b.id));
    Ok(list)
}

/// Resolves enabled patch IDs into existing full paths.
pub fn resolve_patches(dirs: &DataDirs, enabled: &[String]) -> Vec<PathBuf> {
    let folder = dirs.patches();
    let mut out = Vec::new();
    for id in enabled {
        let clean = id.trim();
        if clean.is_empty()
            || clean.len() > 200
            || clean.contains(['/', '\\', ':'])
            || clean.contains("..")
        {
            continue;
        }
        let p = std::path::Path::new(clean);
        if p.file_name() != Some(std::ffi::OsStr::new(clean)) {
            continue;
        }
        if !p
            .extension()
            .and_then(|e| e.to_str())
            .map(|e| e.eq_ignore_ascii_case("dll"))
            .unwrap_or(false)
        {
            continue;
        }
        let path = folder.join(clean);
        if path.is_file() {
            out.push(path);
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn scan_and_resolve_patches() {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDirs::at(tmp.path());
        dirs.ensure().unwrap();

        let patch_file = dirs.patches().join("Translator.dll");
        std::fs::write(&patch_file, b"dummy dll content").unwrap();

        let ignore_file = dirs.patches().join("readme.txt");
        std::fs::write(&ignore_file, b"not a patch").unwrap();

        let list = scan_patches(&dirs).unwrap();
        assert_eq!(list.len(), 1);
        assert_eq!(list[0].id, "Translator.dll");
        assert_eq!(list[0].name, "Translator");
        assert_eq!(list[0].file_size, 17);

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
}
