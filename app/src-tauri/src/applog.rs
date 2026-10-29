//! The launcher's own log: `logs/launcher.log`, for finding out what went wrong.
//!
//! Never write tokens, passwords or launch environments here. Addresses, account names,
//! error texts and exit codes are fine.

use std::io::{Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::sync::{Mutex, OnceLock};

/// The file is started over when it grows past this (the old one is kept as `.old`).
const MAX_BYTES: u64 = 1024 * 1024;

/// A tail is only ever this much of the file (P2-40).
const TAIL_MAX_BYTES: u64 = 256 * 1024;

static PATH: OnceLock<PathBuf> = OnceLock::new();

/// One writer at a time: otherwise a rotation (rename) can hit a file another thread has open,
/// and on Windows that rename fails (P2-40).
static WRITE_LOCK: Mutex<()> = Mutex::new(());

/// Sets the log file and records panics there. Call once at startup.
pub fn init(logs_dir: PathBuf) {
    let _ = std::fs::create_dir_all(&logs_dir);
    let _ = PATH.set(logs_dir.join("launcher.log"));

    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        line(&format!("PANIC: {info}"));
        previous(info);
    }));
}

pub fn line(text: &str) {
    let Some(path) = PATH.get() else {
        return;
    };
    write_line(path, text);
}

fn write_line(path: &Path, text: &str) {
    let now = time::OffsetDateTime::now_utc();
    let stamp = now
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    // One entry per line: keep an error text with line breaks on its line.
    let text = text.replace(['\r', '\n'], " ");
    let entry = format!("{stamp} {text}\n");

    let _guard = WRITE_LOCK.lock().unwrap_or_else(|p| p.into_inner());
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES) {
        // Nobody has the file open right now, so the rename can succeed.
        let _ = std::fs::rename(path, path.with_extension("log.old"));
    }
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let _ = file.write_all(entry.as_bytes());
}

/// The last `lines` lines of a text file, read from the end in blocks (P2-40).
///
/// A game log can be tens of megabytes; only its tail is ever shown. The first line of the
/// result is dropped when reading stopped before the start of the file, because it may be cut
/// in the middle of a character.
pub fn tail(path: &Path, lines: usize) -> Vec<String> {
    const BLOCK: u64 = 8 * 1024;

    let Ok(mut file) = std::fs::File::open(path) else {
        return Vec::new();
    };
    let Ok(len) = file.metadata().map(|m| m.len()) else {
        return Vec::new();
    };
    let mut data: Vec<u8> = Vec::new();
    let mut pos = len;
    while pos > 0 && data.len() < TAIL_MAX_BYTES as usize {
        let take = BLOCK.min(pos);
        pos -= take;
        if file.seek(SeekFrom::Start(pos)).is_err() {
            return Vec::new();
        }
        let mut buf = vec![0u8; take as usize];
        if file.read_exact(&mut buf).is_err() {
            return Vec::new();
        }
        buf.extend_from_slice(&data);
        data = buf;
        // Enough complete lines already?
        if data.iter().rev().filter(|b| **b == b'\n').count() > lines {
            break;
        }
    }

    let complete = pos == 0;
    let text = String::from_utf8_lossy(&data);
    let mut out: Vec<String> = text.lines().map(str::to_string).collect();
    if !complete && !out.is_empty() {
        out.remove(0);
    }
    if out.len() > lines {
        out.drain(..out.len() - lines);
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_appended_with_a_timestamp_and_stay_single_lines() {
        let dir = std::env::temp_dir().join(format!("bebra-log-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("launcher.log");
        // The file itself, not the global one: other tests may log too.
        write_line(&path, "first");
        write_line(&path, "two\nlines");
        let text = std::fs::read_to_string(&path).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with(" first") && lines[0].contains('T'));
        assert!(lines[1].ends_with(" two lines"));

        let _ = std::fs::remove_dir_all(&dir);
    }

    /// A log over the limit is started over; the old one is kept next to it (P2-40).
    #[test]
    fn a_large_log_is_rotated() {
        let dir = std::env::temp_dir().join(format!("bebra-rot-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("launcher.log");
        std::fs::write(&path, vec![b'x'; MAX_BYTES as usize + 1]).unwrap();

        write_line(&path, "after rotation");
        let now = std::fs::read_to_string(&path).unwrap();
        assert!(now.contains("after rotation"));
        assert!(now.len() < 1024);
        assert!(path.with_extension("log.old").exists());

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn the_tail_holds_only_the_last_lines() {
        let dir = std::env::temp_dir().join(format!("bebra-tail-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("client.log");
        let text: String = (0..500).map(|i| format!("line {i}\n")).collect();
        std::fs::write(&path, text).unwrap();

        let tail = tail(&path, 15);
        assert_eq!(tail.len(), 15);
        assert_eq!(tail[0], "line 485");
        assert_eq!(tail[14], "line 499");

        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn a_tail_of_a_missing_file_is_empty() {
        let path = std::env::temp_dir().join(format!("bebra-none-{}", uuid::Uuid::new_v4()));
        assert!(tail(&path, 10).is_empty());
    }

    /// A log larger than the read limit still gives the last lines, and never a half line.
    #[test]
    fn a_huge_log_is_not_read_whole() {
        let dir = std::env::temp_dir().join(format!("bebra-huge-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("client.log");
        let line = "x".repeat(100);
        let mut body = String::new();
        for i in 0..4000 {
            body.push_str(&format!("{i} {line}\n"));
        }
        std::fs::write(&path, body).unwrap();

        let tail = tail(&path, 3);
        assert_eq!(tail.len(), 3);
        assert!(tail[2].starts_with("3999 "));
        assert!(!tail[0].is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
