//! Hiding logins and passwords that may sit inside a URL (P1-13).
//!
//! The launcher lets the player use a proxy like `socks5h://user:password@host:1080`, and
//! error texts often repeat the address they are about. Such a text must never reach
//! `logs/launcher.log` (bug reports get forwarded) or a toast: the userinfo part is replaced
//! with `***` before anything is printed.
//!
//! `bebra_core` is expected to grow the same helper (`http.rs`/`hub.rs`); until then the app
//! keeps this local copy, and call sites can switch over without changing their shape.

/// Replaces the userinfo of every `scheme://user:pass@host` found in `text` with `***`.
///
/// Everything else is left alone, so the message stays readable:
/// `socks5h://user:pw@10.0.0.1:1080` becomes `socks5h://***@10.0.0.1:1080`.
pub fn text(input: &str) -> String {
    let mut out = String::with_capacity(input.len());
    let mut rest = input;
    while let Some(i) = rest.find("://") {
        out.push_str(&rest[..i + 3]);
        rest = &rest[i + 3..];

        // The authority ends where the path, the query or the fragment starts, or at a
        // character that cannot be part of a URL in a message.
        let end = rest
            .find(|c: char| {
                c.is_whitespace() || matches!(c, '/' | '?' | '#' | '"' | '\'' | '`' | '<' | '>')
            })
            .unwrap_or(rest.len());
        let authority = &rest[..end];
        match authority.rfind('@') {
            // Everything before the last `@` is userinfo (`user:password`).
            Some(at) => {
                out.push_str("***@");
                out.push_str(&authority[at + 1..]);
            }
            None => out.push_str(authority),
        }
        rest = &rest[end..];
    }
    out.push_str(rest);
    out
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn userinfo_is_hidden() {
        let masked = text("not a usable proxy address: socks5h://user:secret@10.0.0.1:1080");
        assert_eq!(
            masked,
            "not a usable proxy address: socks5h://***@10.0.0.1:1080"
        );
        assert!(!masked.contains("user"));
        assert!(!masked.contains("secret"));
    }

    #[test]
    fn several_urls_and_percent_encoded_passwords() {
        let masked = text(
            "proxy socks5h://bob:p%40ss@127.0.0.1:1080 failed; hub https://alice:qwerty@hub.example/api?x=1",
        );
        assert_eq!(
            masked,
            "proxy socks5h://***@127.0.0.1:1080 failed; hub https://***@hub.example/api?x=1"
        );
        assert!(!masked.contains("qwerty"));
    }

    #[test]
    fn text_without_urls_is_unchanged() {
        assert_eq!(text("failed to connect"), "failed to connect");
        assert_eq!(text(""), "");
        // No scheme: nothing that looks like userinfo is touched.
        assert_eq!(text("user:pass@host"), "user:pass@host");
    }

    #[test]
    fn address_without_userinfo_is_kept() {
        assert_eq!(
            text("hub https://hub.example/api/status"),
            "hub https://hub.example/api/status"
        );
    }
}
