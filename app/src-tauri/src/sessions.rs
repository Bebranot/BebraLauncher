//! Persistent session journal to ensure game play time is never lost, even if
//! `close_on_play` is enabled, the launcher is closed during play, or crashes.

use std::path::{Path, PathBuf};
use std::time::{SystemTime, UNIX_EPOCH};

use serde::{Deserialize, Serialize};

use crate::settings::Settings;

const SESSIONS_FILE: &str = "active_sessions.json";
/// Maximum play time (in seconds) credited if timestamps are unreliable (8 hours).
const MAX_FALLBACK_PLAY_SECS: u64 = 8 * 3600;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct GameSession {
    pub id: u64,
    pub pid: u32,
    pub image_path: String,
    pub address: String,
    pub server_name: Option<String>,
    pub account: Option<String>,
    pub plays_as: String,
    pub log_path: PathBuf,
    pub started_at_secs: u64,
}

pub fn sessions_path(root: &Path) -> PathBuf {
    root.join(SESSIONS_FILE)
}

pub fn load_sessions(root: &Path) -> Vec<GameSession> {
    let path = sessions_path(root);
    std::fs::read(&path)
        .ok()
        .and_then(|bytes| serde_json::from_slice(&bytes).ok())
        .unwrap_or_default()
}

pub fn save_sessions(root: &Path, sessions: &[GameSession]) -> std::io::Result<()> {
    std::fs::create_dir_all(root)?;
    let path = sessions_path(root);
    let tmp = path.with_extension("json.tmp");
    std::fs::write(&tmp, serde_json::to_vec_pretty(sessions)?)?;
    std::fs::rename(tmp, path)
}

pub fn record_start(root: &Path, session: GameSession) {
    let mut sessions = load_sessions(root);
    sessions.retain(|s| s.pid != session.pid);
    sessions.push(session);
    let _ = save_sessions(root, &sessions);
}

pub fn record_end(root: &Path, pid: u32) -> Option<GameSession> {
    let mut sessions = load_sessions(root);
    let idx = sessions.iter().position(|s| s.pid == pid)?;
    let removed = sessions.remove(idx);
    let _ = save_sessions(root, &sessions);
    Some(removed)
}

/// Recovers unclosed sessions on launcher startup:
/// 1. If the process is still running, keeps it in the active list.
/// 2. If the process has exited, calculates elapsed play time using log file modification
///    or system time, credits it to settings, and removes the session.
///
/// Returns the list of currently running sessions.
pub fn recover_sessions(root: &Path, settings: &mut Settings) -> Vec<GameSession> {
    let sessions = load_sessions(root);
    if sessions.is_empty() {
        return Vec::new();
    }

    let mut still_running = Vec::new();
    let mut play_time_added = false;

    for s in sessions {
        if bebra_core::process::is_process_running_from(s.pid, &s.image_path) {
            still_running.push(s);
            continue;
        }

        // Process exited while launcher was not actively waiting for it.
        let elapsed = calculate_session_elapsed(&s);
        if elapsed >= 5 {
            settings.add_play_time(&s.address, elapsed);
            crate::applog::line(&format!(
                "recovered session for {}: credited {}s play time",
                s.address, elapsed
            ));
            play_time_added = true;
        }
    }

    let _ = save_sessions(root, &still_running);
    if play_time_added {
        let _ = settings.save(root);
    }

    still_running
}

fn calculate_session_elapsed(session: &GameSession) -> u64 {
    // If client log exists and was modified after start, its last write time
    // is the most accurate reflection of when the game stopped running.
    if let Ok(meta) = std::fs::metadata(&session.log_path) {
        if let Ok(mod_time) = meta.modified() {
            if let Ok(mod_secs) = mod_time.duration_since(UNIX_EPOCH) {
                let mod_secs = mod_secs.as_secs();
                if mod_secs > session.started_at_secs {
                    return (mod_secs - session.started_at_secs).min(MAX_FALLBACK_PLAY_SECS);
                }
            }
        }
    }

    // Fallback: estimate from current time, capped at MAX_FALLBACK_PLAY_SECS
    let now_secs = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();
    if now_secs > session.started_at_secs {
        (now_secs - session.started_at_secs).min(MAX_FALLBACK_PLAY_SECS)
    } else {
        0
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tempdir() -> PathBuf {
        let dir =
            std::env::temp_dir().join(format!("bebra-sessions-test-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn record_start_and_end_roundtrip() {
        let dir = tempdir();
        let session = GameSession {
            id: 1,
            pid: 12345,
            image_path: "C:\\game.exe".into(),
            address: "ss14://example.com".into(),
            server_name: Some("Test Server".into()),
            account: None,
            plays_as: "Bebra".into(),
            log_path: dir.join("client.log"),
            started_at_secs: 1000,
        };

        record_start(&dir, session.clone());
        let loaded = load_sessions(&dir);
        assert_eq!(loaded.len(), 1);
        assert_eq!(loaded[0], session);

        let ended = record_end(&dir, 12345);
        assert_eq!(ended, Some(session));
        assert!(load_sessions(&dir).is_empty());

        let _ = std::fs::remove_dir_all(&dir);
    }
}
