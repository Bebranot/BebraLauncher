//! Commands the game (or a second copy of the launcher) sends to the running launcher.
//!
//! The Loader's redial API starts `$SS14_LAUNCHER_PATH --commands :RedialWait R<hex> C<hex>`
//! to ask for a reconnect. The words are those of the official launcher (`LauncherCommands.cs`):
//!
//! - `:Ping`        bring the window forward
//! - `:RedialWait`  wait a moment (the old client is still closing)
//! - `R<hex>`       reason, UTF-8 as hex (text from the server: untrusted)
//! - `r<text>`      reason, plain
//! - `C<hex>`       connect to this address, UTF-8 as hex
//! - `c<address>`   connect to this address, plain

#[derive(Debug, PartialEq, Eq)]
pub enum Command {
    Ping,
    RedialWait,
    Reason(String),
    Connect(String),
    Unknown,
}

/// What the launcher has to do after reading the commands.
#[derive(Debug, PartialEq, Eq)]
pub enum Action {
    /// Bring the window to the front.
    Activate,
    Connect {
        address: String,
        /// Why the game asks for it (shown to the player as plain text).
        reason: Option<String>,
    },
    /// A link was opened (`ss14://…`): show the server, but let the player decide.
    Open { address: String },
}

fn decode_hex_utf8(s: &str) -> Option<String> {
    if !s.len().is_multiple_of(2) || !s.is_ascii() {
        return None;
    }
    let bytes: Option<Vec<u8>> = (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).ok())
        .collect();
    String::from_utf8(bytes?).ok()
}

pub fn parse(arg: &str) -> Command {
    if arg == ":Ping" {
        Command::Ping
    } else if arg == ":RedialWait" {
        Command::RedialWait
    } else if let Some(hex) = arg.strip_prefix('R') {
        decode_hex_utf8(hex).map_or(Command::Unknown, Command::Reason)
    } else if let Some(text) = arg.strip_prefix('r') {
        Command::Reason(text.to_string())
    } else if let Some(hex) = arg.strip_prefix('C') {
        decode_hex_utf8(hex).map_or(Command::Unknown, Command::Connect)
    } else if let Some(address) = arg.strip_prefix('c') {
        Command::Connect(address.to_string())
    } else {
        Command::Unknown
    }
}

/// Turns the command line given to a launcher process into actions.
///
/// - `--commands <command>…`: the commands, in order
/// - a single `ss14://` or `ss14s://` address (a clicked link): open that server, no connecting
/// - anything else (a plain second start): just bring the window forward
pub fn actions(args: &[String]) -> Vec<Action> {
    let commands: Vec<Command> = match args {
        [flag, rest @ ..] if flag == "--commands" => rest.iter().map(|a| parse(a)).collect(),
        [addr] if addr.starts_with("ss14://") || addr.starts_with("ss14s://") => {
            return vec![Action::Open {
                address: addr.clone(),
            }];
        }
        _ => vec![Command::Ping],
    };

    let mut out = Vec::new();
    let mut reason = String::new();
    for command in commands {
        match command {
            Command::Ping => out.push(Action::Activate),
            // Time is handled by the caller retrying while the old client is still closing.
            Command::RedialWait => {}
            Command::Reason(r) => reason = r,
            Command::Connect(address) => out.push(Action::Connect {
                address,
                // The reason belongs to one connect only.
                reason: Some(std::mem::take(&mut reason)).filter(|r| !r.is_empty()),
            }),
            Command::Unknown => {}
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    fn hex(s: &str) -> String {
        s.bytes().map(|b| format!("{b:02X}")).collect()
    }

    fn args(a: &[&str]) -> Vec<String> {
        a.iter().map(|s| s.to_string()).collect()
    }

    #[test]
    fn parses_each_command() {
        assert_eq!(parse(":Ping"), Command::Ping);
        assert_eq!(parse(":RedialWait"), Command::RedialWait);
        assert_eq!(parse("R48656C6C6F"), Command::Reason("Hello".into()));
        assert_eq!(parse("rplain text"), Command::Reason("plain text".into()));
        assert_eq!(
            parse(&format!("C{}", hex("ss14://a:1212"))),
            Command::Connect("ss14://a:1212".into())
        );
        assert_eq!(parse("css14://b"), Command::Connect("ss14://b".into()));
        assert_eq!(parse(":Nope"), Command::Unknown);
        assert_eq!(parse(""), Command::Unknown);
    }

    #[test]
    fn bad_hex_is_ignored_not_guessed() {
        assert_eq!(parse("Rzz"), Command::Unknown);
        assert_eq!(parse("R4"), Command::Unknown);
        // Valid hex, invalid UTF-8.
        assert_eq!(parse("RFFFE"), Command::Unknown);
        // Not ASCII: must not panic on a char boundary.
        assert_eq!(parse("Rжж"), Command::Unknown);
    }

    #[test]
    fn reason_text_may_be_cyrillic_and_multiline() {
        let text = "Сервер полон\nПопробуйте позже";
        assert_eq!(
            parse(&format!("R{}", hex(text))),
            Command::Reason(text.into())
        );
    }

    #[test]
    fn redial_from_the_game() {
        let a = actions(&args(&[
            "--commands",
            ":RedialWait",
            &format!("R{}", hex("The server is full!")),
            &format!("C{}", hex("ss14s://lizard.spacestation14.com")),
        ]));
        assert_eq!(
            a,
            [Action::Connect {
                address: "ss14s://lizard.spacestation14.com".into(),
                reason: Some("The server is full!".into()),
            }]
        );
    }

    #[test]
    fn reason_is_used_once() {
        let a = actions(&args(&["--commands", "rfirst", "css14://a", "css14://b"]));
        assert_eq!(
            a,
            [
                Action::Connect {
                    address: "ss14://a".into(),
                    reason: Some("first".into())
                },
                Action::Connect {
                    address: "ss14://b".into(),
                    reason: None
                },
            ]
        );
    }

    #[test]
    fn address_argument_and_plain_start() {
        assert_eq!(
            actions(&args(&["ss14://host:1212"])),
            [Action::Open {
                address: "ss14://host:1212".into()
            }]
        );
        assert_eq!(actions(&args(&[])), [Action::Activate]);
        assert_eq!(actions(&args(&["something", "else"])), [Action::Activate]);
        assert_eq!(actions(&args(&["--commands", ":Ping"])), [Action::Activate]);
    }
}
