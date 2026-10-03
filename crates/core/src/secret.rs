//! A login token that refuses to be printed by accident.

use serde::{Deserialize, Serialize};

/// A Wizards' Den login token. `Debug` hides the value, there is no `Display`; the only way to
/// read it is [`Token::expose`], so every use of the real value is easy to find.
#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct Token(String);

impl Token {
    pub fn new(value: impl Into<String>) -> Self {
        Self(value.into())
    }

    /// The real token. Use only to send it where it belongs (the auth server, the game client).
    pub fn expose(&self) -> &str {
        &self.0
    }
}

impl std::fmt::Debug for Token {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str("Token(<hidden>)")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn debug_never_shows_the_value() {
        let t = Token::new("super-secret");
        assert_eq!(format!("{t:?}"), "Token(<hidden>)");
        assert_eq!(format!("{:?}", Some(&t)), "Some(Token(<hidden>))");
        assert_eq!(format!("{t:#?}"), "Token(<hidden>)");
        assert_eq!(t.expose(), "super-secret");
    }

    #[test]
    fn serializes_as_a_plain_string() {
        let t = Token::new("abc");
        assert_eq!(serde_json::to_string(&t).unwrap(), "\"abc\"");
        let back: Token = serde_json::from_str("\"abc\"").unwrap();
        assert_eq!(back, t);
    }
}
