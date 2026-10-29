//! Text packs: translations and other wording changes that do not touch how the game plays.
//!
//! A pack is a `.zip` in the packs folder. The game's `Locale/<culture>/*.ftl` files hold the
//! texts shown to the player; a pack carries replacements for them. Packs are mounted through
//! the Loader's overlay (`SS14_LOADER_OVERLAY_ZIP`), which masks files of the game by path.
//!
//! Safety rule: **only** Fluent text files under `Locale/` are accepted. A pack with anything
//! else (code, textures, sounds, prototypes, ...) is refused as a whole, and nothing in it is
//! used. So a pack can change what the text says, never what the game does, and no code from a
//! pack is ever run.

use std::collections::BTreeMap;
use std::io::Read;
use std::path::Path;

use sha2::{Digest, Sha256};

use crate::paths::DataDirs;
use crate::{Error, Result};

/// Most files in one pack.
pub const MAX_FILES: usize = 4000;
/// Largest single text file.
pub const MAX_FILE_BYTES: u64 = 2 * 1024 * 1024;
/// Largest pack, unpacked.
pub const MAX_TOTAL_BYTES: u64 = 32 * 1024 * 1024;
/// Largest pack file on disk.
pub const MAX_ZIP_BYTES: u64 = 32 * 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PackInfo {
    /// The file name inside the packs folder; identifies the pack in the settings.
    pub id: String,
    pub name: String,
    pub description: String,
    pub files: usize,
    pub bytes: u64,
    /// Cultures that have texts in the pack, e.g. `ru-RU`.
    pub cultures: Vec<String>,
    /// Why the pack cannot be used. Empty = usable.
    pub problems: Vec<String>,
}

impl PackInfo {
    pub fn is_valid(&self) -> bool {
        self.problems.is_empty()
    }
}

/// The accepted files of a pack: overlay path (`Locale/...`) to contents.
type Files = BTreeMap<String, Vec<u8>>;

#[derive(Default)]
struct Meta {
    name: Option<String>,
    description: Option<String>,
}

/// Turns a path from a zip into an overlay path, or says why it is not allowed.
fn overlay_path(raw: &str) -> std::result::Result<String, String> {
    if raw.is_empty()
        || raw.starts_with('/')
        || raw.contains('\\')
        || raw.contains(':')
        || raw.chars().any(char::is_control)
    {
        return Err(format!("not a plain relative path: {raw}"));
    }
    let parts: Vec<&str> = raw.split('/').collect();
    if parts
        .iter()
        .any(|p| p.is_empty() || *p == "." || *p == "..")
    {
        return Err(format!("not a plain relative path: {raw}"));
    }
    // Packs made for other launchers keep the files under `Resources/`.
    let parts = if parts[0] == "Resources" {
        &parts[1..]
    } else {
        &parts[..]
    };
    let ok = parts.len() >= 3
        && parts[0] == "Locale"
        && parts
            .last()
            .is_some_and(|f| f.ends_with(".ftl") && f.len() > 4);
    if ok {
        Ok(parts.join("/"))
    } else {
        Err(format!(
            "only Locale/<culture>/*.ftl text files are allowed: {raw}"
        ))
    }
}

/// Reads and checks a pack. Returns what could be read and every problem found.
fn read_pack(path: &Path) -> (Files, Meta, Vec<String>) {
    let mut files = Files::new();
    let mut meta = Meta::default();
    let mut problems = Vec::new();

    let fail = |msg: String, files: Files, meta: Meta, mut problems: Vec<String>| {
        problems.push(msg);
        (files, meta, problems)
    };

    match std::fs::metadata(path) {
        Ok(m) if m.len() > MAX_ZIP_BYTES => {
            return fail(
                format!("the pack file is larger than {} MB", MAX_ZIP_BYTES >> 20),
                files,
                meta,
                problems,
            );
        }
        Err(e) => return fail(format!("cannot read the file: {e}"), files, meta, problems),
        Ok(_) => {}
    }
    let file = match std::fs::File::open(path) {
        Ok(f) => f,
        Err(e) => return fail(format!("cannot open the file: {e}"), files, meta, problems),
    };
    let mut zip = match zip::ZipArchive::new(file) {
        Ok(z) => z,
        Err(e) => return fail(format!("not a zip file: {e}"), files, meta, problems),
    };
    if zip.len() > MAX_FILES {
        return fail(
            format!("more than {MAX_FILES} files"),
            files,
            meta,
            problems,
        );
    }

    let mut total: u64 = 0;
    // Report only the first few problems of one kind: a wrong pack has thousands.
    let mut rejected = 0usize;
    for i in 0..zip.len() {
        let mut entry = match zip.by_index(i) {
            Ok(e) => e,
            Err(e) => {
                problems.push(format!("damaged entry {i}: {e}"));
                continue;
            }
        };
        if entry.is_dir() {
            continue;
        }
        let name = entry.name().to_string();

        if name == "pack.json" {
            let mut text = String::new();
            if entry.take(64 * 1024).read_to_string(&mut text).is_ok() {
                if let Ok(v) = serde_json::from_str::<serde_json::Value>(&text) {
                    meta.name = v.get("name").and_then(|x| x.as_str()).map(str::to_string);
                    meta.description = v
                        .get("description")
                        .and_then(|x| x.as_str())
                        .map(str::to_string);
                }
            }
            continue;
        }

        let overlay = match overlay_path(&name) {
            Ok(p) => p,
            Err(why) => {
                rejected += 1;
                if rejected <= 3 {
                    problems.push(why);
                }
                continue;
            }
        };
        if files.contains_key(&overlay) {
            problems.push(format!("the same file twice: {overlay}"));
            continue;
        }
        if entry.size() > MAX_FILE_BYTES {
            problems.push(format!("file too large: {overlay}"));
            continue;
        }

        // Do not trust the size in the header: stop reading at the limit.
        let mut bytes = Vec::new();
        if let Err(e) = (&mut entry)
            .take(MAX_FILE_BYTES + 1)
            .read_to_end(&mut bytes)
        {
            problems.push(format!("cannot read {overlay}: {e}"));
            continue;
        }
        // Count everything that was read, before the size and UTF-8 checks: otherwise a pack with
        // thousands of rejected 2 MB files inflates to gigabytes without touching the limit.
        total += bytes.len() as u64;
        if total > MAX_TOTAL_BYTES {
            problems.push(format!(
                "more than {} MB of text in total",
                MAX_TOTAL_BYTES >> 20
            ));
            break;
        }
        if bytes.len() as u64 > MAX_FILE_BYTES {
            problems.push(format!("file too large: {overlay}"));
            continue;
        }
        if std::str::from_utf8(&bytes).is_err() {
            problems.push(format!("not UTF-8 text: {overlay}"));
            continue;
        }
        files.insert(overlay, bytes);
    }
    if rejected > 3 {
        problems.push(format!(
            "... and {} more files that are not allowed",
            rejected - 3
        ));
    }
    if files.is_empty() && problems.is_empty() {
        problems.push("no Locale/<culture>/*.ftl files in the pack".into());
    }
    (files, meta, problems)
}

fn cultures_of(files: &Files) -> Vec<String> {
    let mut c: Vec<String> = files
        .keys()
        .filter_map(|p| p.split('/').nth(1))
        .map(str::to_string)
        .collect();
    c.sort();
    c.dedup();
    c
}

/// Looks at one pack file.
pub fn inspect(path: &Path) -> PackInfo {
    let id = path
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    let (files, meta, problems) = read_pack(path);
    let fallback_name = path
        .file_stem()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| id.clone());
    PackInfo {
        id,
        name: meta
            .name
            .filter(|n| !n.trim().is_empty())
            .unwrap_or(fallback_name),
        description: meta.description.unwrap_or_default(),
        files: files.len(),
        bytes: files.values().map(|b| b.len() as u64).sum(),
        cultures: cultures_of(&files),
        problems,
    }
}

/// Every `.zip` in the packs folder, by name.
pub fn scan(dirs: &DataDirs) -> Vec<PackInfo> {
    let Ok(rd) = std::fs::read_dir(dirs.packs()) else {
        return Vec::new();
    };
    let mut paths: Vec<_> = rd
        .flatten()
        .map(|e| e.path())
        .filter(|p| p.is_file() && p.extension().is_some_and(|e| e.eq_ignore_ascii_case("zip")))
        .collect();
    paths.sort();
    paths.iter().map(|p| inspect(p)).collect()
}

/// What `build_overlay` made.
#[derive(Debug, Clone)]
pub struct Overlay {
    pub path: std::path::PathBuf,
    pub files: usize,
    /// Packs that were used, in priority order.
    pub used: Vec<String>,
}

/// What `overlay-<key>.key` remembers about a finished overlay: its file name, how many files it
/// holds and which packs went into it. Written after the zip, so a key without its zip is stale.
const OVERLAY_KEY_SUFFIX: &str = ".key";

/// Size and modification time of a pack: enough to tell that its contents may have changed.
fn pack_fingerprint(path: &Path) -> Option<(u64, u128)> {
    let meta = std::fs::metadata(path).ok()?;
    if !meta.is_file() {
        return None;
    }
    let mtime = meta
        .modified()
        .ok()?
        .duration_since(std::time::UNIX_EPOCH)
        .ok()?
        .as_nanos();
    Some((meta.len(), mtime))
}

/// A name of an overlay file we wrote: `overlay-<hex>.zip`. Keeps a corrupted or hostile key file
/// from pointing the loader at something else in the folder.
fn is_overlay_zip_name(name: &str) -> bool {
    name.strip_prefix("overlay-")
        .and_then(|rest| rest.strip_suffix(".zip"))
        .is_some_and(|hash| hash.len() == 16 && hash.bytes().all(|b| b.is_ascii_hexdigit()))
}

/// Stable name of the key file for a list of enabled packs, derived from their ids, sizes and
/// modification times. Any edit of a pack (or the list) gives another key, so the cache is never
/// used for other input.
fn overlay_input_key(dirs: &DataDirs, enabled: &[String]) -> String {
    let mut hasher = Sha256::new();
    for id in enabled {
        hasher.update((id.len() as u64).to_le_bytes());
        hasher.update(id.as_bytes());
        match pack_fingerprint(&dirs.packs().join(id)) {
            Some((size, mtime)) => {
                hasher.update([1u8]);
                hasher.update(size.to_le_bytes());
                hasher.update(mtime.to_le_bytes());
            }
            None => hasher.update([0u8]),
        }
    }
    hex::encode(hasher.finalize())
}

/// `(zip file name, file count, used packs)` from a key file, in the format written below.
fn read_overlay_key(path: &Path) -> Option<(String, usize, Vec<String>)> {
    let text = std::fs::read_to_string(path).ok()?;
    let mut lines = text.lines();
    let target = lines.next()?.to_string();
    if !is_overlay_zip_name(&target) {
        return None;
    }
    let files: usize = lines.next()?.parse().ok()?;
    let used: Vec<String> = lines.map(str::to_string).collect();
    Some((target, files, used))
}

fn write_overlay_key(path: &Path, target: &str, files: usize, used: &[String]) -> Result<()> {
    let mut text = format!("{target}\n{files}\n");
    for id in used {
        text.push_str(id);
        text.push('\n');
    }
    let tmp = path.with_extension("key.tmp");
    std::fs::write(&tmp, text)?;
    let _ = std::fs::remove_file(path);
    std::fs::rename(&tmp, path)?;
    Ok(())
}

/// Removes everything in the overlays folder except the current zip and key. Another overlay may
/// still be open in a running game: then removing it fails, which is fine.
fn drop_stale_overlays(dir: &Path, target: &Path, key: &Path) {
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    for entry in rd.flatten() {
        let path = entry.path();
        if path != target && path != key {
            let _ = std::fs::remove_file(path);
        }
    }
}

/// Merges the enabled packs into one overlay zip. Earlier packs win when several have the
/// same file. Packs that are missing or not valid are skipped (and reported by [`scan`]).
/// Returns `None` when there is nothing to mount.
///
/// Building reads and unpacks every enabled pack, so the result is cached: `overlay-<key>.key`
/// holds the input fingerprint (id, size and modification time of each pack) and is used to skip
/// the work next time. The zip name still comes from the merged contents, so a running game's
/// overlay is never rewritten.
pub fn build_overlay(dirs: &DataDirs, enabled: &[String]) -> Result<Option<Overlay>> {
    let dir = dirs.overlays();
    std::fs::create_dir_all(&dir)?;
    let key_digest = overlay_input_key(dirs, enabled);
    let key_path = dir.join(format!("overlay-{}{OVERLAY_KEY_SUFFIX}", &key_digest[..16]));

    if let Some((target_name, files, used)) = read_overlay_key(&key_path) {
        let target = dir.join(&target_name);
        if target.is_file() {
            drop_stale_overlays(&dir, &target, &key_path);
            return Ok(Some(Overlay {
                path: target,
                files,
                used,
            }));
        }
    }

    let mut merged = Files::new();
    let mut used = Vec::new();
    for id in enabled {
        // Ids are plain file names; anything with a separator is not one of ours.
        if id.contains(['/', '\\']) || id.contains("..") {
            continue;
        }
        let (files, _, problems) = read_pack(&dirs.packs().join(id));
        if !problems.is_empty() || files.is_empty() {
            continue;
        }
        for (path, bytes) in files {
            merged.entry(path).or_insert(bytes);
        }
        used.push(id.clone());
    }
    if merged.is_empty() {
        return Ok(None);
    }

    // The name depends on the contents: an overlay in use by a running game is never rewritten.
    let mut hasher = Sha256::new();
    for (path, bytes) in &merged {
        hasher.update((path.len() as u64).to_le_bytes());
        hasher.update(path.as_bytes());
        hasher.update((bytes.len() as u64).to_le_bytes());
        hasher.update(bytes);
    }
    let digest = hex::encode(hasher.finalize());
    let target = dir.join(format!("overlay-{}.zip", &digest[..16]));

    if !target.is_file() {
        let tmp = dir.join(format!("overlay-{}.tmp", &digest[..16]));
        {
            let file = std::fs::File::create(&tmp)?;
            let mut writer = zip::ZipWriter::new(file);
            let options = zip::write::SimpleFileOptions::default()
                .compression_method(zip::CompressionMethod::Deflated);
            for (path, bytes) in &merged {
                writer
                    .start_file(path.as_str(), options)
                    .map_err(|e| Error::Other(format!("cannot write the overlay: {e}")))?;
                std::io::Write::write_all(&mut writer, bytes)?;
            }
            writer
                .finish()
                .map_err(|e| Error::Other(format!("cannot write the overlay: {e}")))?;
        }
        std::fs::rename(&tmp, &target)?;
    }

    // The key is written last: a key without a zip is simply ignored next time.
    let target_name = target
        .file_name()
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_default();
    write_overlay_key(&key_path, &target_name, merged.len(), &used)?;

    drop_stale_overlays(&dir, &target, &key_path);

    Ok(Some(Overlay {
        path: target,
        files: merged.len(),
        used,
    }))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Write;

    fn make_zip(path: &Path, files: &[(&str, &[u8])]) {
        let file = std::fs::File::create(path).unwrap();
        let mut w = zip::ZipWriter::new(file);
        let options = zip::write::SimpleFileOptions::default();
        for (name, bytes) in files {
            w.start_file(*name, options).unwrap();
            w.write_all(bytes).unwrap();
        }
        w.finish().unwrap();
    }

    fn dirs() -> (tempfile::TempDir, DataDirs) {
        let tmp = tempfile::tempdir().unwrap();
        let dirs = DataDirs::at(tmp.path());
        std::fs::create_dir_all(dirs.packs()).unwrap();
        (tmp, dirs)
    }

    #[test]
    fn only_locale_text_files_pass() {
        assert_eq!(
            overlay_path("Locale/ru-RU/ui/menu.ftl").unwrap(),
            "Locale/ru-RU/ui/menu.ftl"
        );
        assert_eq!(
            overlay_path("Resources/Locale/en-US/x.ftl").unwrap(),
            "Locale/en-US/x.ftl"
        );
        for bad in [
            "Content.Client.dll",
            "Assemblies/evil.dll",
            "Textures/a.png",
            "Prototypes/x.yml",
            "Locale/readme.txt",
            "Locale/ru-RU/code.dll",
            "Locale/ru-RU/.ftl",
            "Locale/x.ftl",
            "/Locale/ru-RU/x.ftl",
            "Locale/../Prototypes/x.ftl",
            "Locale/ru-RU/../../x.ftl",
            "Locale\\ru-RU\\x.ftl",
            "C:/Locale/ru-RU/x.ftl",
            "Locale//ru-RU/x.ftl",
            "",
        ] {
            assert!(overlay_path(bad).is_err(), "should refuse {bad:?}");
        }
    }

    #[test]
    fn a_text_pack_is_valid_and_described() {
        let (_t, dirs) = dirs();
        let p = dirs.packs().join("russian.zip");
        make_zip(
            &p,
            &[
                (
                    "pack.json",
                    r#"{"name":"Русский","description":"Перевод интерфейса"}"#.as_bytes(),
                ),
                ("Locale/ru-RU/a.ftl", "hello = Привет".as_bytes()),
                ("Locale/ru-RU/b/c.ftl", b"x = y"),
                ("Locale/en-US/a.ftl", b"hello = Hi"),
            ],
        );
        let info = inspect(&p);
        assert!(info.is_valid(), "{:?}", info.problems);
        assert_eq!(info.name, "Русский");
        assert_eq!(info.description, "Перевод интерфейса");
        assert_eq!(info.files, 3);
        assert_eq!(info.cultures, ["en-US", "ru-RU"]);
        assert_eq!(info.id, "russian.zip");
    }

    #[test]
    fn a_pack_with_anything_else_is_refused_as_a_whole() {
        let (_t, dirs) = dirs();
        let p = dirs.packs().join("sneaky.zip");
        make_zip(
            &p,
            &[
                ("Locale/ru-RU/a.ftl", b"x = y"),
                ("Assemblies/Patch.dll", b"MZ"),
                ("Textures/wall.png", b"png"),
            ],
        );
        let info = inspect(&p);
        assert!(!info.is_valid());
        assert!(info.problems.iter().any(|m| m.contains("Patch.dll")));

        // And it is not mounted, not even its text files.
        let overlay = build_overlay(&dirs, &["sneaky.zip".to_string()]).unwrap();
        assert!(overlay.is_none());
    }

    #[test]
    fn broken_and_oversized_packs_are_refused() {
        let (_t, dirs) = dirs();
        std::fs::write(dirs.packs().join("junk.zip"), b"not a zip").unwrap();
        assert!(!inspect(&dirs.packs().join("junk.zip")).is_valid());

        make_zip(
            &dirs.packs().join("binary.zip"),
            &[("Locale/ru-RU/a.ftl", &[0xff, 0xfe, 0x00, 0x01])],
        );
        let info = inspect(&dirs.packs().join("binary.zip"));
        assert!(info.problems.iter().any(|m| m.contains("UTF-8")));

        let big = vec![b'a'; (MAX_FILE_BYTES + 1) as usize];
        make_zip(
            &dirs.packs().join("big.zip"),
            &[("Locale/ru-RU/a.ftl", &big)],
        );
        assert!(!inspect(&dirs.packs().join("big.zip")).is_valid());

        make_zip(&dirs.packs().join("empty.zip"), &[("readme.txt", b"hi")]);
        assert!(!inspect(&dirs.packs().join("empty.zip")).is_valid());
    }

    #[test]
    fn overlay_merges_with_earlier_packs_winning() {
        let (_t, dirs) = dirs();
        make_zip(
            &dirs.packs().join("a.zip"),
            &[
                ("Locale/ru-RU/x.ftl", b"k = from a"),
                ("Locale/ru-RU/only-a.ftl", b"a = 1"),
            ],
        );
        make_zip(
            &dirs.packs().join("b.zip"),
            &[
                ("Locale/ru-RU/x.ftl", b"k = from b"),
                ("Locale/ru-RU/only-b.ftl", b"b = 2"),
            ],
        );

        let overlay = build_overlay(
            &dirs,
            &["a.zip".into(), "b.zip".into(), "missing.zip".into()],
        )
        .unwrap()
        .unwrap();
        assert_eq!(overlay.used, ["a.zip", "b.zip"]);
        assert_eq!(overlay.files, 3);

        let mut zip = zip::ZipArchive::new(std::fs::File::open(&overlay.path).unwrap()).unwrap();
        let mut text = String::new();
        zip.by_name("Locale/ru-RU/x.ftl")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, "k = from a");
        assert!(zip.by_name("Locale/ru-RU/only-b.ftl").is_ok());

        // Same packs give the same file, so a running game's overlay is never rewritten.
        let again = build_overlay(&dirs, &["a.zip".into(), "b.zip".into()])
            .unwrap()
            .unwrap();
        assert_eq!(again.path, overlay.path);
        // A different combination gets another file and the old one goes away.
        let other = build_overlay(&dirs, &["b.zip".into()]).unwrap().unwrap();
        assert_ne!(other.path, overlay.path);
        assert!(!overlay.path.exists());
    }

    #[test]
    fn nothing_enabled_means_no_overlay_and_ids_cannot_escape_the_folder() {
        let (_t, dirs) = dirs();
        assert!(build_overlay(&dirs, &[]).unwrap().is_none());
        make_zip(
            &dirs.root().join("outside.zip"),
            &[("Locale/ru-RU/x.ftl", b"k = v")],
        );
        assert!(build_overlay(&dirs, &["../outside.zip".to_string()])
            .unwrap()
            .is_none());
    }

    #[test]
    fn rejected_files_still_count_towards_the_total_limit() {
        let (_t, dirs) = dirs();
        // Each entry is read (2 MB) but refused as not being UTF-8; the total must still stop it.
        let big = vec![0xffu8; MAX_FILE_BYTES as usize];
        let files: Vec<(String, &[u8])> = (0..17)
            .map(|i| (format!("Locale/ru-RU/f{i}.ftl"), big.as_slice()))
            .collect();
        let refs: Vec<(&str, &[u8])> = files.iter().map(|(n, d)| (n.as_str(), *d)).collect();
        let p = dirs.packs().join("bomb.zip");
        make_zip(&p, &refs);
        assert!(
            std::fs::metadata(&p).unwrap().len() <= MAX_ZIP_BYTES,
            "the zip itself stays small; the bomb is in the unpacked size"
        );

        let (kept, _, problems) = read_pack(&p);
        assert!(kept.is_empty());
        assert!(
            problems.iter().any(|m| m.contains("MB of text in total")),
            "{problems:?}"
        );
    }

    #[test]
    fn a_cached_overlay_is_used_without_reading_the_packs() {
        let (_t, dirs) = dirs();
        make_zip(
            &dirs.packs().join("a.zip"),
            &[("Locale/ru-RU/x.ftl", b"k = v")],
        );
        let enabled = ["a.zip".to_string()];
        let first = build_overlay(&dirs, &enabled).unwrap().unwrap();
        let key = dirs.overlays().join(format!(
            "overlay-{}.key",
            &overlay_input_key(&dirs, &enabled)[..16]
        ));
        assert!(key.is_file(), "the key is written next to the zip");
        let entries = std::fs::read_dir(dirs.overlays()).unwrap().count();
        assert_eq!(entries, 2, "one zip and one key, nothing else");

        // A key that points at another (existing) overlay must be trusted: if the packs were read,
        // the real merge would give the first overlay instead.
        let fake = dirs.overlays().join("overlay-0123456789abcdef.zip");
        std::fs::write(&fake, b"not really a zip").unwrap();
        write_overlay_key(&key, "overlay-0123456789abcdef.zip", 42, &enabled).unwrap();
        let cached = build_overlay(&dirs, &enabled).unwrap().unwrap();
        assert_eq!(cached.path, fake);
        assert_eq!(cached.files, 42);
        assert_eq!(cached.used, enabled);
        assert!(!first.path.exists(), "old overlays are cleaned up");
    }

    #[test]
    fn editing_a_pack_invalidates_the_overlay_cache() {
        let (_t, dirs) = dirs();
        let pack = dirs.packs().join("a.zip");
        make_zip(&pack, &[("Locale/ru-RU/x.ftl", b"k = old")]);
        let enabled = ["a.zip".to_string()];
        let first = build_overlay(&dirs, &enabled).unwrap().unwrap();

        std::thread::sleep(std::time::Duration::from_millis(20));
        make_zip(&pack, &[("Locale/ru-RU/x.ftl", b"k = new")]);
        let second = build_overlay(&dirs, &enabled).unwrap().unwrap();
        assert_ne!(second.path, first.path, "different contents, another zip");

        let mut zip = zip::ZipArchive::new(std::fs::File::open(&second.path).unwrap()).unwrap();
        let mut text = String::new();
        zip.by_name("Locale/ru-RU/x.ftl")
            .unwrap()
            .read_to_string(&mut text)
            .unwrap();
        assert_eq!(text, "k = new");
    }

    #[test]
    fn scan_lists_only_zip_files_in_name_order() {
        let (_t, dirs) = dirs();
        make_zip(
            &dirs.packs().join("b.zip"),
            &[("Locale/ru-RU/x.ftl", b"k = v")],
        );
        make_zip(
            &dirs.packs().join("a.ZIP"),
            &[("Locale/ru-RU/x.ftl", b"k = v")],
        );
        std::fs::write(dirs.packs().join("notes.txt"), b"x").unwrap();
        let ids: Vec<String> = scan(&dirs).into_iter().map(|p| p.id).collect();
        assert_eq!(ids, ["a.ZIP", "b.zip"]);
    }
}
