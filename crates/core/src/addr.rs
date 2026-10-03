//! `ss14://` and `ss14s://` server addresses (docs/protocol.md, section 1.3).

use std::fmt;

use url::Url;

use crate::{Error, Result};

/// Port used for `ss14://` addresses that don't specify one.
pub const DEFAULT_SERVER_PORT: u16 = 1212;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ServerAddr {
    /// The address exactly as given (the hub identifies servers by this string).
    original: String,
    url: Url,
}

impl ServerAddr {
    /// Parses `ss14://host[:port][/path]` or `ss14s://...`. A missing scheme means `ss14://`.
    pub fn parse(input: &str) -> Result<Self> {
        let input = input.trim();
        let with_scheme = if input.contains("://") {
            input.to_string()
        } else {
            format!("ss14://{input}")
        };

        let bad = || Error::InvalidAddress(input.to_string());
        let url = Url::parse(&with_scheme).map_err(|_| bad())?;
        if url.scheme() != "ss14" && url.scheme() != "ss14s" {
            return Err(bad());
        }
        if url.host_str().is_none_or(|h| h.trim().is_empty()) {
            return Err(bad());
        }

        Ok(Self {
            original: input.to_string(),
            url,
        })
    }

    pub fn as_str(&self) -> &str {
        &self.original
    }

    /// `ss14s://` means HTTPS.
    pub fn is_tls(&self) -> bool {
        self.url.scheme() == "ss14s"
    }

    pub fn host(&self) -> &str {
        // Checked in `parse`.
        self.url.host_str().unwrap_or_default()
    }

    /// Port as written in the address, if any.
    pub fn port(&self) -> Option<u16> {
        self.url.port()
    }

    /// HTTP(S) address of the game server API, with a trailing slash.
    /// `ss14://` without a port gets 1212; `ss14s://` without a port stays on 443.
    pub fn api_base(&self) -> String {
        let scheme = if self.is_tls() { "https" } else { "http" };
        let port = match self.url.port() {
            Some(p) => Some(p),
            None if !self.is_tls() => Some(DEFAULT_SERVER_PORT),
            None => None,
        };

        let mut out = format!("{scheme}://{}", self.host());
        if let Some(p) = port {
            out.push_str(&format!(":{p}"));
        }

        let path = self.url.path();
        out.push_str(path);
        if !path.ends_with('/') {
            out.push('/');
        }
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
        let port = self.url.port().unwrap_or(if self.is_tls() {
            443
        } else {
            DEFAULT_SERVER_PORT
        });
        format!("udp://{}:{port}/", self.host())
    }
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
    fn rejects_garbage() {
        for bad in [
            "",
            "   ",
            "ss14://",
            "http://example.com",
            "ftp://x",
            "ss14s:///path",
        ] {
            assert!(
                ServerAddr::parse(bad).is_err(),
                "{bad:?} should be rejected"
            );
        }
    }

    #[test]
    fn original_string_is_preserved() {
        let a = ServerAddr::parse("ss14://194.93.2.107:1212/").unwrap();
        assert_eq!(a.as_str(), "ss14://194.93.2.107:1212/");
        assert_eq!(a.to_string(), "ss14://194.93.2.107:1212/");
    }
}
