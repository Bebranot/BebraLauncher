//! Discord Rich Presence kept by the launcher while a game runs (`discord_rpc_mode = launcher`).
//!
//! The game's own presence is switched off then (`discord.enabled=false`). The launcher knows the
//! server it started and asks its `/status`, so it does not see what happens inside the client: a
//! redirect to another server shows only after the launcher is asked to redial.
//!
//! Discord's local protocol is small: frames of `opcode: u32 LE, length: u32 LE, JSON` over the
//! pipe `\\.\pipe\discord-ipc-N`. A handshake, then one `SET_ACTIVITY` per change; closing the pipe
//! clears the presence.

use std::fs::{File, OpenOptions};
use std::io::{self, Read, Write};
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

use bebra_core::http::Client;
use bebra_core::server::{self, RunLevel, ServerStatus};
use bebra_core::ServerAddr;
use serde_json::{json, Value};

/// The application of the game itself: Discord shows "Space Station 14" and its images. Replace
/// it to show another name; the images below then have to exist in that application.
const APP_ID: &str = "560499552273170473";
const LARGE_IMAGE: &str = "logo";
const LARGE_TEXT: &str = "Space Station 14";

/// How often the server is asked and the presence refreshed. Discord allows about one update per
/// 15 seconds.
const REFRESH: Duration = Duration::from_secs(30);
/// Discord limits `details`, `state` and image texts to 2..=128 bytes.
const TEXT_MIN: usize = 2;
const TEXT_MAX: usize = 128;
/// Far above any real answer: a broken peer must not make the launcher allocate gigabytes.
const FRAME_LIMIT: usize = 64 * 1024;
/// Frames read while waiting for one answer before the connection counts as broken.
const FRAMES_PER_ANSWER: usize = 16;

const OP_HANDSHAKE: u32 = 0;
const OP_FRAME: u32 = 1;
const OP_CLOSE: u32 = 2;
const OP_PING: u32 = 3;
const OP_PONG: u32 = 4;

fn write_frame(out: &mut impl Write, op: u32, payload: &Value) -> io::Result<()> {
    let body = serde_json::to_vec(payload)?;
    let length = u32::try_from(body.len())
        .map_err(|_| io::Error::new(io::ErrorKind::InvalidInput, "frame too large"))?;
    let mut frame = Vec::with_capacity(body.len() + 8);
    frame.extend_from_slice(&op.to_le_bytes());
    frame.extend_from_slice(&length.to_le_bytes());
    frame.extend_from_slice(&body);
    // One write: a pipe in message-less mode still keeps it in one piece.
    out.write_all(&frame)?;
    out.flush()
}

fn read_frame(input: &mut impl Read) -> io::Result<(u32, Value)> {
    let mut header = [0u8; 8];
    input.read_exact(&mut header)?;
    let op = u32::from_le_bytes([header[0], header[1], header[2], header[3]]);
    let length = u32::from_le_bytes([header[4], header[5], header[6], header[7]]) as usize;
    if length > FRAME_LIMIT {
        return Err(io::Error::new(
            io::ErrorKind::InvalidData,
            "frame too large",
        ));
    }
    let mut body = vec![0u8; length];
    input.read_exact(&mut body)?;
    Ok((op, serde_json::from_slice(&body)?))
}

fn broken(message: impl Into<String>) -> io::Error {
    io::Error::new(io::ErrorKind::InvalidData, message.into())
}

/// A connection to Discord over anything that reads and writes (a pipe; a buffer in tests).
struct Connection<S> {
    stream: S,
    next_nonce: u64,
}

impl<S: Read + Write> Connection<S> {
    fn handshake(mut stream: S) -> io::Result<Self> {
        write_frame(
            &mut stream,
            OP_HANDSHAKE,
            &json!({ "v": 1, "client_id": APP_ID }),
        )?;
        let (op, answer) = read_frame(&mut stream)?;
        if op != OP_FRAME || answer["evt"] != "READY" {
            return Err(broken(format!("handshake refused: {answer}")));
        }
        Ok(Self {
            stream,
            next_nonce: 1,
        })
    }

    fn set_activity(&mut self, pid: u32, activity: &Value) -> io::Result<()> {
        let nonce = self.next_nonce.to_string();
        self.next_nonce += 1;
        write_frame(
            &mut self.stream,
            OP_FRAME,
            &json!({
                "cmd": "SET_ACTIVITY",
                "args": { "pid": pid, "activity": activity },
                "nonce": nonce,
            }),
        )?;
        // Reading the answer keeps the pipe empty and shows a refused activity.
        for _ in 0..FRAMES_PER_ANSWER {
            let (op, answer) = read_frame(&mut self.stream)?;
            match op {
                OP_PING => write_frame(&mut self.stream, OP_PONG, &answer)?,
                OP_CLOSE => return Err(broken(format!("closed by Discord: {answer}"))),
                OP_FRAME if answer["nonce"] == nonce.as_str() => {
                    return if answer["evt"] == "ERROR" {
                        Err(broken(format!("activity refused: {answer}")))
                    } else {
                        Ok(())
                    };
                }
                _ => {}
            }
        }
        Err(broken("no answer from Discord"))
    }
}

/// The first pipe Discord listens on: there can be several Discord builds, one pipe for each.
fn open_pipe() -> Option<File> {
    (0..10).find_map(|n| {
        OpenOptions::new()
            .read(true)
            .write(true)
            .open(format!(r"\\.\pipe\discord-ipc-{n}"))
            .ok()
    })
}

/// The presence's words in one interface language.
struct Texts {
    on_server: &'static str,
    playing: &'static str,
    lobby: &'static str,
    round: &'static str,
    round_end: &'static str,
}

fn texts(lang: &str) -> Texts {
    match lang {
        "ru" => Texts {
            on_server: "На сервере: ",
            playing: "В игре",
            lobby: "Лобби",
            round: "Раунд",
            round_end: "Конец раунда",
        },
        "uk" => Texts {
            on_server: "На сервері: ",
            playing: "У грі",
            lobby: "Лобі",
            round: "Раунд",
            round_end: "Кінець раунду",
        },
        "de" => Texts {
            on_server: "Auf Server: ",
            playing: "Im Spiel",
            lobby: "Lobby",
            round: "Runde",
            round_end: "Rundenende",
        },
        _ => Texts {
            on_server: "On server: ",
            playing: "In game",
            lobby: "Lobby",
            round: "Round",
            round_end: "Round end",
        },
    }
}

/// `auto` or a language code from the settings -> the code of an interface language, the way the
/// UI's `detectLang` picks it.
pub fn resolve_language(setting: &str) -> String {
    if setting != "auto" {
        return setting.to_string();
    }
    match system_language().as_deref() {
        Some("be") => "ru".to_string(),
        Some(code) => code.to_string(),
        None => "en".to_string(),
    }
}

/// The language part of the Windows user's locale, in lower case.
fn system_language() -> Option<String> {
    use windows_sys::Win32::Globalization::GetUserDefaultLocaleName;
    let mut name = [0u16; 85];
    // SAFETY: the buffer is valid for the length passed, which the call does not exceed.
    let written = unsafe { GetUserDefaultLocaleName(name.as_mut_ptr(), name.len() as i32) };
    let written = usize::try_from(written).ok()?.checked_sub(1)?;
    let locale = String::from_utf16_lossy(name.get(..written)?);
    let base = locale.split('-').next()?.to_lowercase();
    (!base.is_empty()).then_some(base)
}

/// Cuts on a character boundary to fit Discord's limit in bytes.
fn clip(text: &str) -> String {
    let text = text.trim();
    if text.len() <= TEXT_MAX {
        return text.to_string();
    }
    const MARK: &str = "...";
    let mut end = TEXT_MAX - MARK.len();
    while !text.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}{MARK}", text[..end].trim_end())
}

/// What Discord shows. The server's address is never used: it may be a private one.
fn build_activity(
    texts: &Texts,
    server_name: Option<&str>,
    status: Option<&ServerStatus>,
    started_at_secs: u64,
) -> Value {
    let name = server_name
        .map(str::trim)
        .filter(|n| !n.is_empty())
        .or_else(|| status.and_then(|s| s.name.as_deref()).map(str::trim))
        .filter(|n| !n.is_empty());
    let details = name.map_or_else(
        || texts.playing.to_string(),
        |n| format!("{}{n}", texts.on_server),
    );

    let mut state = Vec::new();
    if let Some(status) = status {
        match status.run_level_kind() {
            Some(RunLevel::PreRoundLobby) => state.push(texts.lobby.to_string()),
            Some(RunLevel::InRound) => {
                state.push(texts.round.to_string());
                let map = status.map.as_deref().map_or("", str::trim);
                if !map.is_empty() {
                    state.push(map.to_string());
                }
            }
            Some(RunLevel::PostRound) => state.push(texts.round_end.to_string()),
            None => {}
        }
        state.push(if status.soft_max_players > 0 {
            format!("{}/{}", status.players, status.soft_max_players)
        } else {
            status.players.to_string()
        });
    }
    let state = clip(&state.join(" · "));

    let mut activity = json!({
        "details": clip(&details),
        "timestamps": { "start": started_at_secs },
        "assets": { "large_image": LARGE_IMAGE, "large_text": LARGE_TEXT },
    });
    // Discord refuses the whole activity over a text shorter than its minimum.
    if state.len() >= TEXT_MIN {
        activity["state"] = Value::from(state);
    }
    activity
}

/// What one game's presence needs to know.
pub struct PresenceRequest {
    pub client: Client,
    pub addr: ServerAddr,
    pub server_name: Option<String>,
    /// The interface language's code, see [`resolve_language`].
    pub language: String,
    pub started_at_secs: u64,
}

/// The presence of one running game: it ends when this is dropped.
pub struct Presence {
    // Dropping the sender is what wakes the worker up and ends it.
    _stop: mpsc::Sender<()>,
}

/// Starts keeping the presence. It is only a convenience: `None` if the worker cannot start, and
/// Discord may be missing or closed and reopened at any moment.
pub fn start(request: PresenceRequest) -> Option<Presence> {
    let (stop, stopped) = mpsc::channel();
    let worker = std::thread::Builder::new()
        .name("presence".into())
        .spawn(move || run(&request, &stopped));
    match worker {
        Ok(_) => Some(Presence { _stop: stop }),
        Err(e) => {
            crate::applog::line(&format!("discord presence: no thread: {e}"));
            None
        }
    }
}

fn run(request: &PresenceRequest, stopped: &mpsc::Receiver<()>) {
    let Ok(rt) = tokio::runtime::Builder::new_current_thread()
        .enable_all()
        .build()
    else {
        crate::applog::line("discord presence: no runtime");
        return;
    };
    let words = texts(&request.language);
    let pid = std::process::id();
    let mut connection: Option<Connection<File>> = None;
    let mut status: Option<ServerStatus> = None;
    let mut shown: Option<Value> = None;

    loop {
        if connection.is_none() {
            shown = None;
            connection = match open_pipe().map(Connection::handshake) {
                Some(Ok(connected)) => {
                    crate::applog::line("discord presence: connected");
                    Some(connected)
                }
                Some(Err(e)) => {
                    crate::applog::line(&format!("discord presence: {e}"));
                    None
                }
                // Discord is not running; looked for again on the next round.
                None => None,
            };
        }
        // No Discord, no reason to bother the server.
        if let Some(active) = connection.as_mut() {
            // A failed request keeps the last known numbers.
            if let Ok(fresh) = rt.block_on(server::fetch_status(&request.client, &request.addr)) {
                status = Some(fresh);
            }
            let activity = build_activity(
                &words,
                request.server_name.as_deref(),
                status.as_ref(),
                request.started_at_secs,
            );
            if shown.as_ref() != Some(&activity) {
                match active.set_activity(pid, &activity) {
                    Ok(()) => shown = Some(activity),
                    Err(e) => {
                        crate::applog::line(&format!("discord presence: {e}"));
                        connection = None;
                    }
                }
            }
        }
        match stopped.recv_timeout(REFRESH) {
            Err(RecvTimeoutError::Timeout) => {}
            // The game ended; closing the pipe clears the presence.
            Ok(()) | Err(RecvTimeoutError::Disconnected) => return,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::io::Cursor;

    /// Answers from `input`, what was written ends up in `output`.
    struct Duplex {
        input: Cursor<Vec<u8>>,
        output: Vec<u8>,
    }

    impl Read for Duplex {
        fn read(&mut self, buf: &mut [u8]) -> io::Result<usize> {
            self.input.read(buf)
        }
    }

    impl Write for Duplex {
        fn write(&mut self, buf: &[u8]) -> io::Result<usize> {
            self.output.write(buf)
        }
        fn flush(&mut self) -> io::Result<()> {
            Ok(())
        }
    }

    fn peer(frames: &[(u32, Value)]) -> Duplex {
        let mut input = Vec::new();
        for (op, payload) in frames {
            write_frame(&mut input, *op, payload).unwrap();
        }
        Duplex {
            input: Cursor::new(input),
            output: Vec::new(),
        }
    }

    fn sent(output: &[u8]) -> Vec<(u32, Value)> {
        let mut cursor = Cursor::new(output);
        let mut frames = Vec::new();
        while (cursor.position() as usize) < output.len() {
            frames.push(read_frame(&mut cursor).unwrap());
        }
        frames
    }

    fn ready() -> (u32, Value) {
        (OP_FRAME, json!({ "evt": "READY" }))
    }

    #[test]
    fn a_frame_is_opcode_length_json_in_little_endian() {
        let mut out = Vec::new();
        write_frame(&mut out, OP_FRAME, &json!({})).unwrap();
        assert_eq!(out, [1, 0, 0, 0, 2, 0, 0, 0, b'{', b'}']);
        assert_eq!(read_frame(&mut Cursor::new(out)).unwrap(), (1, json!({})));
    }

    #[test]
    fn an_oversized_frame_is_refused_before_it_is_allocated() {
        let mut frame = OP_FRAME.to_le_bytes().to_vec();
        frame.extend_from_slice(&u32::MAX.to_le_bytes());
        assert!(read_frame(&mut Cursor::new(frame)).is_err());
    }

    #[test]
    fn the_handshake_names_the_application() {
        let connection = Connection::handshake(peer(&[ready()])).unwrap();
        let frames = sent(&connection.stream.output);
        assert_eq!(frames[0].0, OP_HANDSHAKE);
        assert_eq!(frames[0].1["client_id"], APP_ID);
        assert_eq!(frames[0].1["v"], 1);
    }

    #[test]
    fn a_refused_handshake_is_an_error() {
        let close = (
            OP_CLOSE,
            json!({ "code": 4000, "message": "Invalid Client ID" }),
        );
        assert!(Connection::handshake(peer(&[close])).is_err());
    }

    #[test]
    fn set_activity_waits_for_its_own_answer_and_answers_pings() {
        let frames = [
            ready(),
            (OP_PING, json!({ "n": 7 })),
            (OP_FRAME, json!({ "evt": "OTHER", "nonce": "0" })),
            (OP_FRAME, json!({ "cmd": "SET_ACTIVITY", "nonce": "1" })),
        ];
        let mut connection = Connection::handshake(peer(&frames)).unwrap();
        connection
            .set_activity(42, &json!({ "details": "x" }))
            .unwrap();
        let written = sent(&connection.stream.output);
        assert_eq!(written[1].1["cmd"], "SET_ACTIVITY");
        assert_eq!(written[1].1["args"]["pid"], 42);
        assert_eq!(written[1].1["args"]["activity"]["details"], "x");
        assert_eq!(written[2], (OP_PONG, json!({ "n": 7 })));
    }

    #[test]
    fn a_refused_activity_and_a_closed_pipe_are_errors() {
        let refused = (OP_FRAME, json!({ "evt": "ERROR", "nonce": "1" }));
        let mut connection = Connection::handshake(peer(&[ready(), refused])).unwrap();
        assert!(connection.set_activity(1, &json!({})).is_err());

        let mut connection = Connection::handshake(peer(&[ready()])).unwrap();
        assert!(connection.set_activity(1, &json!({})).is_err(), "no answer");
    }

    fn status(json: &str) -> ServerStatus {
        serde_json::from_str(json).unwrap()
    }

    #[test]
    fn the_activity_shows_server_phase_map_and_players() {
        let s = status(r#"{"run_level":1,"map":"Cluster","players":12,"soft_max_players":100}"#);
        let a = build_activity(&texts("ru"), Some("Local Duty"), Some(&s), 1000);
        assert_eq!(a["details"], "На сервере: Local Duty");
        assert_eq!(a["state"], "Раунд · Cluster · 12/100");
        assert_eq!(a["timestamps"]["start"], 1000);
        assert_eq!(a["assets"]["large_image"], LARGE_IMAGE);

        let lobby = status(r#"{"run_level":0,"map":"Cluster","players":3}"#);
        let a = build_activity(&texts("en"), Some("S"), Some(&lobby), 0);
        assert_eq!(
            a["state"], "Lobby · 3",
            "no map before the round, no limit if unknown"
        );
    }

    #[test]
    fn the_activity_works_without_a_status_or_a_name() {
        let a = build_activity(&texts("ru"), None, None, 0);
        assert_eq!(a["details"], "В игре");
        assert!(
            a.get("state").is_none(),
            "Discord refuses a too short state"
        );

        let s = status(r#"{"name":"From status"}"#);
        let a = build_activity(&texts("en"), Some("  "), Some(&s), 0);
        assert_eq!(a["details"], "On server: From status");
    }

    #[test]
    fn long_texts_are_cut_to_the_limit_on_a_character_boundary() {
        let long = "я".repeat(200);
        let clipped = clip(&long);
        assert!(clipped.len() <= TEXT_MAX);
        assert!(clipped.ends_with("..."));
        assert_eq!(clip(" short "), "short");

        let a = build_activity(&texts("ru"), Some(&long), None, 0);
        assert!(a["details"].as_str().unwrap().len() <= TEXT_MAX);
    }

    #[test]
    fn an_explicit_language_is_kept() {
        assert_eq!(resolve_language("de"), "de");
        assert!(!resolve_language("auto").is_empty());
    }
}
