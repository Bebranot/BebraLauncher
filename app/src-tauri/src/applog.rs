//! The launcher's own log: `logs/launcher.log`, for finding out what went wrong.
//!
//! Never write tokens, passwords or launch environments here. Addresses, account names,
//! error texts and exit codes are fine.

use std::io::Write;
use std::path::PathBuf;
use std::sync::OnceLock;

/// The file is started over when it grows past this (the old one is kept as `.old`).
const MAX_BYTES: u64 = 1024 * 1024;

static PATH: OnceLock<PathBuf> = OnceLock::new();

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
    if std::fs::metadata(path).is_ok_and(|m| m.len() > MAX_BYTES) {
        let _ = std::fs::rename(path, path.with_extension("log.old"));
    }
    let Ok(mut file) = std::fs::OpenOptions::new()
        .create(true)
        .append(true)
        .open(path)
    else {
        return;
    };
    let now = time::OffsetDateTime::now_utc();
    let stamp = now
        .format(&time::format_description::well_known::Rfc3339)
        .unwrap_or_default();
    // One entry per line: keep an error text with line breaks on its line.
    let text = text.replace(['\r', '\n'], " ");
    let _ = writeln!(file, "{stamp} {text}");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn lines_are_appended_with_a_timestamp_and_stay_single_lines() {
        let dir = std::env::temp_dir().join(format!("bebra-log-{}", uuid::Uuid::new_v4()));
        init(dir.clone());
        line("first");
        line("two\nlines");
        let text = std::fs::read_to_string(dir.join("launcher.log")).unwrap();
        let lines: Vec<&str> = text.lines().collect();
        assert_eq!(lines.len(), 2);
        assert!(lines[0].ends_with(" first") && lines[0].contains('T'));
        assert!(lines[1].ends_with(" two lines"));
    }
}
