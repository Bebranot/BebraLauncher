//! `ss14://` and `ss14s://` server addresses (docs/protocol.md, section 1.3).

use std::fmt;

use url::Host;

use crate::{Error, Result};

/// Port used for `ss14://` addresses that don't specify one.
pub const DEFAULT_SERVER_PORT: u16 = 1212;

#[derive(Debug, Clone)]
pub struct ServerAddr {
    /// The address exactly as given (the hub identifies servers by this string).
    original: String,
    /// Normalized host: lowercase IDNA domain, or an IPv6 address in brackets.
    host: String,
    port: Option<u16>,
    tls: bool,
    /// Path as written, starting with `/` when non-empty.
    path: String,
}

/// Two addresses are the same server when their normalized parts match, even if one was typed as
/// `SS14://EXAMPLE.COM` and the other as `ss14://example.com`. [`as_str`](ServerAddr::as_str)
/// still returns exactly what the caller passed (the hub keys servers by that string).
impl PartialEq for ServerAddr {
    fn eq(&self, other: &Self) -> bool {
        self.host == other.host
            && self.port == other.port
            && self.tls == other.tls
            && self.path == other.path
    }
}

impl Eq for ServerAddr {}

impl ServerAddr {
    /// Parses `ss14://host[:port][/path]` or `ss14s://...`. A missing scheme means `ss14://`.
    /// Also accepts `http://` and `https://` by mapping them to `ss14://` and `ss14s://`.
    ///
    /// The host goes through [`url::Host::parse`], so a Unicode name becomes its IDNA form and
    /// the case is normalized: `SS14://EXAMPLE.COM`, `ss14://example.com` and `ss14://EXAMPLE.com`
    /// are the same server. Credentials (`user:pass@host`) are refused: the hub would log them.
    pub fn parse(input: &str) -> Result<Self> {
        let input = input.trim();
        let bad = || Error::InvalidAddress(input.to_string());
        if input.is_empty() || input.chars().any(char::is_control) {
            return Err(bad());
        }

        let (scheme, rest) = match input.find("://") {
            Some(i) => (input[..i].to_ascii_lowercase(), &input[i + 3..]),
            None => ("ss14".to_string(), input),
        };
        let tls = match scheme.as_str() {
            "ss14" | "http" => false,
            "ss14s" | "https" => true,
            _ => return Err(bad()),
        };

        // Userinfo has no meaning here and `ss14://trusted@evil` must never look like `evil`.
        if rest.contains('@') || rest.contains('?') || rest.contains('#') {
            return Err(bad());
        }
        let (authority, path) = match rest.find('/') {
            Some(i) => (&rest[..i], &rest[i..]),
            None => (rest, ""),
        };
        if authority.is_empty() || authority.chars().any(char::is_whitespace) {
            return Err(bad());
        }
        if path.chars().any(char::is_whitespace) {
            return Err(bad());
        }

        let (raw_host, port) = split_host_port(authority).ok_or_else(bad)?;
        let host = normalize_host(raw_host).ok_or_else(bad)?;

        Ok(Self {
            original: input.to_string(),
            host,
            port,
            tls,
            path: path.to_string(),
        })
    }

    pub fn as_str(&self) -> &str {
        &self.original
    }

    /// `ss14s://` means HTTPS.
    pub fn is_tls(&self) -> bool {
        self.tls
    }

    pub fn host(&self) -> &str {
        &self.host
    }

    /// Port as written in the address, if any.
    pub fn port(&self) -> Option<u16> {
        self.port
    }

    /// HTTP(S) address of the game server API, with a trailing slash.
    /// `ss14://` without a port gets 1212; `ss14s://` without a port stays on 443.
    pub fn api_base(&self) -> String {
        let scheme = if self.tls { "https" } else { "http" };
        let port = match self.port {
            Some(p) => Some(p),
            None if !self.tls => Some(DEFAULT_SERVER_PORT),
            None => None,
        };

        let mut out = format!("{scheme}://{}", self.host);
        if let Some(p) = port {
            out.push_str(&format!(":{p}"));
        }

        out.push_str(&self.path);
        if !self.path.ends_with('/') {
            out.push('/');
        }
        out
    }

    /// The address as .NET prints it (what the official launcher passes to the client):
    /// `scheme://host[:port]/path`, with `/` when the path is empty.
    pub fn normalized(&self) -> String {
        let scheme = if self.tls { "ss14s" } else { "ss14" };
        let mut out = format!("{scheme}://{}", self.host);
        if let Some(p) = self.port {
            out.push_str(&format!(":{p}"));
        }
        out.push_str(if self.path.is_empty() {
            "/"
        } else {
            &self.path
        });
        out
    }

    pub fn status_url(&self) -> String {
        format!("{}status", self.api_base())
    }

    pub fn info_url(&self) -> String {
        format!("{}info", self.api_base())
    }

    /// Self-hosted client zip. Not necessarily the real download address.
    pub fn client_zip_url(&self) -> String {
        format!("{}client.zip", self.api_base())
    }

    /// Fallback game address when `/info` has no `connect_address`:
    /// same host and *API* port, UDP scheme (mirrors the official launcher).
    pub fn default_connect_url(&self) -> String {
        let port = self
            .port
            .unwrap_or(if self.tls { 443 } else { DEFAULT_SERVER_PORT });
        format!("udp://{}:{port}/", self.host)
    }
}

/// Splits `host[:port]` without touching the host itself. IPv6 must be in brackets.
fn split_host_port(authority: &str) -> Option<(&str, Option<u16>)> {
    if authority.starts_with('[') {
        let close = authority.find(']')?;
        let host = &authority[..=close];
        let rest = &authority[close + 1..];
        let port = match rest.strip_prefix(':') {
            Some(p) => Some(parse_port(p)?),
            None if rest.is_empty() => None,
            None => return None,
        };
        return Some((host, port));
    }
    match authority.rsplit_once(':') {
        Some((host, port)) => {
            // More than one `:` without brackets is a bare IPv6 address, not `host:port`.
            if host.contains(':') {
                return None;
            }
            Some((host, Some(parse_port(port)?)))
        }
        None => Some((authority, None)),
    }
}

fn parse_port(text: &str) -> Option<u16> {
    if text.is_empty() || !text.bytes().all(|b| b.is_ascii_digit()) {
        return None;
    }
    match text.parse::<u16>() {
        Ok(0) | Err(_) => None,
        Ok(p) => Some(p),
    }
}

/// Lowercase IDNA host, IPv4, or IPv6 in brackets.
fn normalize_host(raw: &str) -> Option<String> {
    if raw.is_empty() {
        return None;
    }
    match Host::parse(raw).ok()? {
        Host::Domain(d) if d.is_empty() => None,
        Host::Domain(d) => Some(d),
        Host::Ipv4(ip) => Some(ip.to_string()),
        Host::Ipv6(ip) => Some(format!("[{ip}]")),
    }
}

/// Validates `connect_address` from `/info` and normalizes it to `udp://host:port/`.
/// Anything else (another scheme, no port, credentials) is refused so the caller can fall back
/// to the address the player actually picked.
pub fn normalize_connect_address(raw: &str) -> Option<String> {
    let raw = raw.trim();
    if raw.is_empty() || raw.chars().any(|c| c.is_control() || c.is_whitespace()) {
        return None;
    }
    let u = url::Url::parse(raw).ok()?;
    if u.scheme() != "udp" || !u.username().is_empty() || u.password().is_some() {
        return None;
    }
    let port = u.port()?;
    if port == 0 {
        return None;
    }
    let host = match u.host()? {
        Host::Domain(d) => normalize_host(d)?,
        Host::Ipv4(ip) => ip.to_string(),
        Host::Ipv6(ip) => format!("[{ip}]"),
    };
    Some(format!("udp://{host}:{port}/"))
}

impl fmt::Display for ServerAddr {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(&self.original)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn base(s: &str) -> String {
        ServerAddr::parse(s).unwrap().api_base()
    }

    #[test]
    fn plain_scheme_uses_default_port() {
        assert_eq!(base("ss14://adven.space"), "http://adven.space:1212/");
        assert_eq!(base("adven.space"), "http://adven.space:1212/");
    }

    #[test]
    fn explicit_port_is_kept() {
        assert_eq!(
            base("ss14://104.243.34.95:1212"),
            "http://104.243.34.95:1212/"
        );
        assert_eq!(
            base("ss14://starlight.network:31201"),
            "http://starlight.network:31201/"
        );
    }

    #[test]
    fn tls_scheme_has_no_forced_port() {
        assert_eq!(
            base("ss14s://eridani.imperialconnect.space/"),
            "https://eridani.imperialconnect.space/"
        );
    }

    #[test]
    fn path_gets_trailing_slash() {
        assert_eq!(
            base("ss14s://spacestories.club/core"),
            "https://spacestories.club/core/"
        );
        assert_eq!(
            base("ss14s://server.ss14.org/perseus/"),
            "https://server.ss14.org/perseus/"
        );
    }

    #[test]
    fn ipv6_host_keeps_brackets() {
        assert_eq!(base("ss14://[::1]:1212"), "http://[::1]:1212/");
        assert_eq!(
            ServerAddr::parse("ss14://[::1]:1212").unwrap().normalized(),
            "ss14://[::1]:1212/"
        );
    }

    #[test]
    fn host_is_lowercased_and_idna_encoded() {
        let a = ServerAddr::parse("SS14://EXAMPLE.COM").unwrap();
        assert_eq!(a.host(), "example.com");
        assert_eq!(a.normalized(), "ss14://example.com/");
        assert_eq!(
            a,
            ServerAddr::parse("ss14://example.com").unwrap(),
            "the same server must compare equal regardless of case"
        );

        let cyrillic = ServerAddr::parse("ss14://тест.рф").unwrap();
        assert_eq!(cyrillic.host(), "xn--e1aybc.xn--p1ai");
        assert_eq!(cyrillic.api_base(), "http://xn--e1aybc.xn--p1ai:1212/");
        assert!(cyrillic.api_base().is_ascii());
    }

    #[test]
    fn endpoints() {
        let a = ServerAddr::parse("ss14s://spacestories.club/core").unwrap();
        assert_eq!(a.status_url(), "https://spacestories.club/core/status");
        assert_eq!(a.info_url(), "https://spacestories.club/core/info");
        assert_eq!(
            a.client_zip_url(),
            "https://spacestories.club/core/client.zip"
        );
    }

    #[test]
    fn default_connect_address() {
        let a = ServerAddr::parse("ss14://example.com").unwrap();
        assert_eq!(a.default_connect_url(), "udp://example.com:1212/");
        let b = ServerAddr::parse("ss14://example.com:1214").unwrap();
        assert_eq!(b.default_connect_url(), "udp://example.com:1214/");
    }

    #[test]
    fn normalized_form() {
        let n = |s: &str| ServerAddr::parse(s).unwrap().normalized();
        assert_eq!(
            n("ss14s://lizard.spacestation14.com"),
            "ss14s://lizard.spacestation14.com/"
        );
        assert_eq!(n("ss14://104.243.34.95:1212"), "ss14://104.243.34.95:1212/");
        assert_eq!(
            n("ss14s://server.ss14.org/perseus/"),
            "ss14s://server.ss14.org/perseus/"
        );
        assert_eq!(n("example.com"), "ss14://example.com/");
        assert_eq!(n("http://localhost:1212"), "ss14://localhost:1212/");
        assert_eq!(
            n("https://hub.corvax.fun:443"),
            "ss14s://hub.corvax.fun:443/"
        );
    }

    #[test]
    fn rejects_garbage() {
        for bad in [
            "",
            "   ",
            "ss14://",
            "ftp://example.com",
            "ftp://x",
            "ss14s:///path",
            "ss14://host:0",
            "ss14://host:99999",
            "ss14://host:",
            "ss14://::1:1212",
            "ss14://user@evil.example",
            "ss14://user:pass@evil.example",
            "ss14://example.com/?x=1",
            "ss14://exa mple.com",
        ] {
            assert!(
                ServerAddr::parse(bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn connect_address_is_validated() {
        assert_eq!(
            normalize_connect_address("udp://example.com:1214/").as_deref(),
            Some("udp://example.com:1214/")
        );
        assert_eq!(
            normalize_connect_address("UDP://EXAMPLE.COM:1214").as_deref(),
            Some("udp://example.com:1214/")
        );
        assert_eq!(
            normalize_connect_address("udp://[::1]:1214/").as_deref(),
            Some("udp://[::1]:1214/")
        );
        for bad in [
            "",
            "example.com:1214",
            "tcp://example.com:1214/",
            "udp://example.com/",
            "udp://user:pass@example.com:1214/",
            "udp://example.com:0/",
            "javascript:alert(1)",
        ] {
            assert_eq!(normalize_connect_address(bad), None, "{bad:?}");
        }
    }

    #[test]
    fn original_string_is_preserved() {
        let a = ServerAddr::parse("ss14://194.93.2.107:1212/").unwrap();
        assert_eq!(a.as_str(), "ss14://194.93.2.107:1212/");
        assert_eq!(a.to_string(), "ss14://194.93.2.107:1212/");
    }

    #[test]
    fn uppercase_scheme_is_accepted_and_normalized() {
        let a = ServerAddr::parse("SS14S://EXAMPLE.COM/Core").unwrap();
        assert!(a.is_tls());
        assert_eq!(a.host(), "example.com");
        assert_eq!(a.normalized(), "ss14s://example.com/Core");
        assert_eq!(a.api_base(), "https://example.com/Core/");

        let plain = ServerAddr::parse("Ss14://Example.COM:1212").unwrap();
        assert!(!plain.is_tls());
        assert_eq!(plain.normalized(), "ss14://example.com:1212/");
    }

    #[test]
    fn userinfo_is_refused_in_every_position() {
        for bad in [
            "ss14://user@example.com",
            "SS14://user:pass@example.com",
            "ss14s://user@example.com:443/",
            "user@example.com",
            "ss14://user@example.com:1212/path",
            "ss14://example.com/path@evil",
        ] {
            assert!(ServerAddr::parse(bad).is_err(), "{bad:?} must be refused");
        }
    }

    #[test]
    fn idn_hosts_are_lowercased_and_punycoded() {
        let upper = ServerAddr::parse("SS14://ТЕСТ.РФ").unwrap();
        let lower = ServerAddr::parse("ss14://тест.рф").unwrap();
        assert_eq!(upper.host(), "xn--e1aybc.xn--p1ai");
        assert_eq!(upper, lower);
        assert_eq!(upper.api_base(), "http://xn--e1aybc.xn--p1ai:1212/");
        // Mixed case and a port keep working.
        assert_eq!(
            ServerAddr::parse("SS14://ТЕСТ.РФ:1212")
                .unwrap()
                .normalized(),
            "ss14://xn--e1aybc.xn--p1ai:1212/"
        );
    }

    #[test]
    fn port_zero_is_never_accepted() {
        for bad in [
            "ss14://host:0",
            "SS14://HOST:0",
            "ss14s://host:0/",
            "udp://host:0/",
            "ss14://[::1]:0",
        ] {
            assert!(ServerAddr::parse(bad).is_err(), "{bad:?}");
            assert_eq!(normalize_connect_address(bad), None, "{bad:?}");
        }
        // A leading zero is still zero.
        assert!(ServerAddr::parse("ss14://host:00000").is_err());
    }

    #[test]
    fn connect_address_rejects_credentials_and_uppercase_port_zero() {
        for bad in [
            "UDP://user@example.com:1214/",
            "UDP://user:pass@example.com:1214/",
            "UDP://EXAMPLE.COM:0/",
            "UDP://[::1]:0/",
        ] {
            assert_eq!(normalize_connect_address(bad), None, "{bad:?}");
        }
        assert_eq!(
            normalize_connect_address("UDP://ТЕСТ.РФ:1214").as_deref(),
            Some("udp://xn--e1aybc.xn--p1ai:1214/")
        );
    }
}
