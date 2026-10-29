//! The Wizards' Den account API (docs/protocol.md, section 7).
//!
//! We only log in, refresh, check and log out. Registering and managing an account happens on the
//! website. Passwords are sent once, never stored or logged; what we keep is the token.
//! Error values never contain response bodies, because those can hold tokens.

use std::time::Duration;

use reqwest::header::{HeaderValue, AUTHORIZATION};
use reqwest::{Client, Response, StatusCode};
use serde::{Deserialize, Deserializer, Serialize};
use time::OffsetDateTime;
use uuid::Uuid;

use crate::http::UrlFallbackSet;
use crate::secret::Token;
use crate::{Error, Result};

pub const AUTH_URLS: [&str; 2] = [
    "https://auth.spacestation14.com/",
    "https://auth.fallback.spacestation14.com/",
];

const REQUEST_TIMEOUT: Duration = Duration::from_secs(30);

/// The auth server's answers are tiny; anything larger is not a JSON response we can use.
const MAX_AUTH_JSON_BYTES: usize = 64 * 1024;

/// Who is logging in: the account name, or its id (for re-login of a saved account).
#[derive(Debug, Clone, Copy)]
pub enum Identity<'a> {
    Username(&'a str),
    UserId(Uuid),
}

#[derive(Debug, Clone)]
pub struct LoginInfo {
    pub user_id: Uuid,
    pub username: String,
    pub token: Token,
    pub expire_time: OffsetDateTime,
}

/// Why the server said no.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum DenyCode {
    None,
    InvalidCredentials,
    AccountUnconfirmed,
    /// Ask for the two-factor code and try again.
    TfaRequired,
    TfaInvalid,
    AccountLocked,
    /// A code we don't know yet (or none was sent).
    Unknown,
}

impl<'de> Deserialize<'de> for DenyCode {
    fn deserialize<D: Deserializer<'de>>(d: D) -> std::result::Result<Self, D::Error> {
        // The server's enum is read case-insensitively by the official launcher; do the same.
        let s = String::deserialize(d)?;
        Ok(match s.to_ascii_lowercase().as_str() {
            "none" => Self::None,
            "invalidcredentials" => Self::InvalidCredentials,
            "accountunconfirmed" => Self::AccountUnconfirmed,
            "tfarequired" => Self::TfaRequired,
            "tfainvalid" => Self::TfaInvalid,
            "accountlocked" => Self::AccountLocked,
            _ => Self::Unknown,
        })
    }
}

#[derive(Debug, Clone)]
pub enum AuthenticateOutcome {
    Success(LoginInfo),
    Denied { errors: Vec<String>, code: DenyCode },
}

// Request bodies. The official launcher sends camelCase JSON including nulls; so do we.
#[derive(Serialize)]
#[serde(rename_all = "camelCase")]
struct AuthenticateRequest<'a> {
    username: Option<&'a str>,
    user_id: Option<Uuid>,
    password: &'a str,
    tfa_code: Option<&'a str>,
}

#[derive(Serialize)]
struct TokenRequest<'a> {
    token: &'a str,
}

// Responses. Names are matched in camelCase or PascalCase.
#[derive(Deserialize)]
struct AuthenticateResponse {
    #[serde(alias = "Token")]
    token: Token,
    #[serde(alias = "Username")]
    username: String,
    #[serde(rename = "userId", alias = "UserId")]
    user_id: Uuid,
    #[serde(
        rename = "expireTime",
        alias = "ExpireTime",
        with = "time::serde::rfc3339"
    )]
    expire_time: OffsetDateTime,
}

#[derive(Deserialize)]
struct DenyResponse {
    #[serde(default, alias = "Errors")]
    errors: Vec<String>,
    #[serde(default = "unknown_code", alias = "Code")]
    code: DenyCode,
}

fn unknown_code() -> DenyCode {
    DenyCode::Unknown
}

#[derive(Deserialize)]
struct RefreshResponse {
    #[serde(rename = "newToken", alias = "NewToken")]
    new_token: Token,
    #[serde(
        rename = "expireTime",
        alias = "ExpireTime",
        with = "time::serde::rfc3339"
    )]
    expire_time: OffsetDateTime,
}

#[derive(Debug, Clone)]
pub struct AuthClient {
    client: Client,
    urls: UrlFallbackSet,
}

impl AuthClient {
    pub fn official(client: Client) -> Self {
        Self::new(client, UrlFallbackSet::new(AUTH_URLS))
    }

    pub fn new(client: Client, urls: UrlFallbackSet) -> Self {
        Self { client, urls }
    }

    /// The auth mirror that has worked best so far; the game client is told to use it.
    pub fn most_successful_url(&self) -> &str {
        self.urls.most_successful()
    }

    /// Logs in with a password (and the two-factor code, once the server asks for it).
    pub async fn authenticate(
        &self,
        who: Identity<'_>,
        password: &str,
        tfa_code: Option<&str>,
    ) -> Result<AuthenticateOutcome> {
        let body = AuthenticateRequest {
            username: match who {
                Identity::Username(n) => Some(n),
                Identity::UserId(_) => None,
            },
            user_id: match who {
                Identity::UserId(id) => Some(id),
                Identity::Username(_) => None,
            },
            password,
            tfa_code,
        };
        let resp = self
            .urls
            .join("api/auth/authenticate")
            .send_ordered(
                |url| self.client.post(url).timeout(REQUEST_TIMEOUT).json(&body),
                |status| status.is_success() || status == StatusCode::UNAUTHORIZED,
            )
            .await?;

        match resp.status() {
            s if s.is_success() => {
                let r: AuthenticateResponse = read_json(resp).await?;
                Ok(AuthenticateOutcome::Success(LoginInfo {
                    user_id: r.user_id,
                    username: r.username,
                    token: r.token,
                    expire_time: r.expire_time,
                }))
            }
            StatusCode::UNAUTHORIZED => {
                // A proxy's HTML error page (or an empty body) must still mean "no": the player
                // typed a wrong password, not "the launcher is broken".
                match read_json::<DenyResponse>(resp).await {
                    Ok(r) => Ok(AuthenticateOutcome::Denied {
                        errors: r.errors,
                        code: r.code,
                    }),
                    Err(_) => Ok(AuthenticateOutcome::Denied {
                        errors: Vec::new(),
                        code: DenyCode::Unknown,
                    }),
                }
            }
            _ => Err(unexpected_status(&resp)),
        }
    }

    /// Asks for a longer-lived token. `None` means the server no longer accepts this one: the
    /// account has to log in again.
    pub async fn refresh(&self, token: &Token) -> Result<Option<(Token, OffsetDateTime)>> {
        let body = TokenRequest {
            token: token.expose(),
        };
        let resp = self
            .urls
            .join("api/auth/refresh")
            .send_ordered(
                |url| self.client.post(url).timeout(REQUEST_TIMEOUT).json(&body),
                |status| status.is_success() || status == StatusCode::UNAUTHORIZED,
            )
            .await?;

        match resp.status() {
            s if s.is_success() => {
                let r: RefreshResponse = read_json(resp).await?;
                Ok(Some((r.new_token, r.expire_time)))
            }
            StatusCode::UNAUTHORIZED => Ok(None),
            _ => Err(unexpected_status(&resp)),
        }
    }

    /// Is the token still accepted?
    pub async fn check(&self, token: &Token) -> Result<bool> {
        let mut value =
            HeaderValue::from_str(&format!("SS14Auth {}", token.expose())).map_err(|_| {
                Error::Other("the token contains characters that cannot be sent".into())
            })?;
        value.set_sensitive(true);

        let resp = self
            .urls
            .join("api/auth/ping")
            .send(|url| {
                self.client
                    .get(url)
                    .timeout(REQUEST_TIMEOUT)
                    .header(AUTHORIZATION, value.clone())
            })
            .await?;

        match resp.status() {
            s if s.is_success() => Ok(true),
            StatusCode::UNAUTHORIZED => Ok(false),
            _ => Err(unexpected_status(&resp)),
        }
    }

    /// Invalidates the token on the server. A failure only means the token stays valid until it
    /// expires; callers should still forget it locally.
    pub async fn logout(&self, token: &Token) -> Result<()> {
        let body = TokenRequest {
            token: token.expose(),
        };
        let resp = self
            .urls
            .join("api/auth/logout")
            .send_ordered(
                |url| self.client.post(url).timeout(REQUEST_TIMEOUT).json(&body),
                |status| status.is_success(),
            )
            .await?;
        if resp.status().is_success() {
            Ok(())
        } else {
            Err(unexpected_status(&resp))
        }
    }
}

fn unexpected_status(resp: &Response) -> Error {
    Error::HttpStatus {
        status: resp.status().as_u16(),
        url: resp.url().to_string(),
    }
}

/// Parses a JSON response, with a hard size limit and an idle timeout. On failure the message says
/// nothing about the body (it can hold tokens).
async fn read_json<T: serde::de::DeserializeOwned>(resp: Response) -> Result<T> {
    crate::http::read_json_limited(resp, MAX_AUTH_JSON_BYTES, REQUEST_TIMEOUT)
        .await
        .map_err(|e| Error::Other(format!("the auth server sent an unreadable response ({e})")))
}

#[cfg(test)]
mod tests {
    use serde_json::{json, Value};

    use super::*;
    use crate::http::build_client;
    use crate::test_util::{serve, Req};

    const TOKEN: &str = "tok-SECRET-123";
    const ID: &str = "11111111-2222-3333-4444-555555555555";

    fn client_for(base: &str) -> AuthClient {
        AuthClient::new(
            build_client().unwrap(),
            UrlFallbackSet::new([base.to_string()]),
        )
    }

    fn body_of(req: &Req) -> Value {
        serde_json::from_slice(&req.body).unwrap()
    }

    fn ok_login() -> Vec<u8> {
        json!({
            "token": TOKEN, "username": "Player", "userId": ID,
            "expireTime": "2026-11-03T08:38:01.2306510+00:00"
        })
        .to_string()
        .into_bytes()
    }

    #[tokio::test]
    async fn login_request_and_success_response() {
        let (base, seen) = serve(|_| (200, vec![], ok_login())).await;
        let out = client_for(&base)
            .authenticate(Identity::Username("Player"), "hunter2", None)
            .await
            .unwrap();

        let req = seen.lock().unwrap()[0].clone();
        assert_eq!(req.method, "POST");
        assert_eq!(req.path, "/api/auth/authenticate");
        assert_eq!(
            body_of(&req),
            json!({"username": "Player", "userId": null, "password": "hunter2", "tfaCode": null})
        );

        let AuthenticateOutcome::Success(info) = out else {
            panic!("expected success")
        };
        assert_eq!(info.username, "Player");
        assert_eq!(info.user_id.to_string(), ID);
        assert_eq!(info.token.expose(), TOKEN);
        assert_eq!(info.expire_time.year(), 2026);
        assert_eq!(info.expire_time.month() as u8, 11);
        assert!(
            !format!("{info:?}").contains("SECRET"),
            "the token stays out of Debug"
        );
    }

    #[tokio::test]
    async fn login_by_user_id_with_two_factor_code() {
        let (base, seen) = serve(|_| (200, vec![], ok_login())).await;
        let id: Uuid = ID.parse().unwrap();
        client_for(&base)
            .authenticate(Identity::UserId(id), "pw", Some("123456"))
            .await
            .unwrap();
        assert_eq!(
            body_of(&seen.lock().unwrap()[0]),
            json!({"username": null, "userId": ID, "password": "pw", "tfaCode": "123456"})
        );
    }

    #[tokio::test]
    async fn pascal_case_responses_are_understood_too() {
        let body = json!({
            "Token": TOKEN, "Username": "Player", "UserId": ID, "ExpireTime": "2026-11-03T08:38:01Z"
        })
        .to_string()
        .into_bytes();
        let (base, _) = serve(move |_| (200, vec![], body.clone())).await;
        let out = client_for(&base)
            .authenticate(Identity::Username("Player"), "pw", None)
            .await
            .unwrap();
        assert!(matches!(out, AuthenticateOutcome::Success(_)));
    }

    #[tokio::test]
    async fn denials_carry_the_reason() {
        for (code_text, expected) in [
            ("InvalidCredentials", DenyCode::InvalidCredentials),
            ("TfaRequired", DenyCode::TfaRequired),
            ("tfaInvalid", DenyCode::TfaInvalid),
            ("AccountUnconfirmed", DenyCode::AccountUnconfirmed),
            ("AccountLocked", DenyCode::AccountLocked),
            ("SomethingNew", DenyCode::Unknown),
        ] {
            let body = json!({"errors": ["nope"], "code": code_text})
                .to_string()
                .into_bytes();
            let (base, _) = serve(move |_| (401, vec![], body.clone())).await;
            let out = client_for(&base)
                .authenticate(Identity::Username("a"), "b", None)
                .await
                .unwrap();
            let AuthenticateOutcome::Denied { errors, code } = out else {
                panic!("expected a denial for {code_text}")
            };
            assert_eq!(
                (errors, code),
                (vec!["nope".to_string()], expected),
                "{code_text}"
            );
        }
    }

    #[tokio::test]
    async fn an_unreadable_401_is_a_denial_not_a_parse_error() {
        // A proxy or a captive portal answers with HTML: the player is still refused.
        let (base, _) = serve(|_| (401, vec![], b"<html>blocked</html>".to_vec())).await;
        let out = client_for(&base)
            .authenticate(Identity::Username("a"), "b", None)
            .await
            .unwrap();
        let AuthenticateOutcome::Denied { errors, code } = out else {
            panic!("expected a denial")
        };
        assert!(errors.is_empty());
        assert_eq!(code, DenyCode::Unknown);

        let (base, _) = serve(|_| (401, vec![], Vec::new())).await;
        let out = client_for(&base)
            .authenticate(Identity::Username("a"), "b", None)
            .await
            .unwrap();
        assert!(matches!(
            out,
            AuthenticateOutcome::Denied {
                code: DenyCode::Unknown,
                ..
            }
        ));
    }

    #[tokio::test]
    async fn oversized_responses_are_not_read_to_the_end() {
        let big = format!("{{\"token\":\"{}\"}}", "a".repeat(MAX_AUTH_JSON_BYTES));
        let (base, _) = serve(move |_| (200, vec![], big.clone().into_bytes())).await;
        let err = client_for(&base)
            .authenticate(Identity::Username("a"), "b", None)
            .await
            .unwrap_err();
        let text = err.to_string();
        assert!(
            text.contains("larger") || text.contains("too large"),
            "{text}"
        );
    }

    #[tokio::test]
    async fn other_statuses_and_garbage_are_errors_without_leaking_the_body() {
        let (base, _) = serve(|_| (500, vec![], b"boom".to_vec())).await;
        let err = client_for(&base)
            .authenticate(Identity::Username("a"), "b", None)
            .await
            .unwrap_err();
        assert!(matches!(err, Error::HttpStatus { status: 500, .. }));

        let garbage = format!("{{\"token\": \"{TOKEN}\", this is not json").into_bytes();
        let (base, _) = serve(move |_| (200, vec![], garbage.clone())).await;
        let err = client_for(&base)
            .authenticate(Identity::Username("a"), "b", None)
            .await
            .unwrap_err();
        assert!(!err.to_string().contains("SECRET"), "{err}");
        assert!(!format!("{err:?}").contains("SECRET"), "{err:?}");
    }

    #[tokio::test]
    async fn refresh_gives_a_new_token_or_none() {
        let (base, seen) = serve(|req| match body_of(req)["token"].as_str() {
            Some("old") => (
                200,
                vec![],
                json!({"newToken": "new-token", "expireTime": "2027-01-01T00:00:00Z"})
                    .to_string()
                    .into_bytes(),
            ),
            Some("dead") => (401, vec![], vec![]),
            _ => (500, vec![], vec![]),
        })
        .await;
        let c = client_for(&base);

        let (token, expire) = c.refresh(&Token::new("old")).await.unwrap().unwrap();
        assert_eq!(token.expose(), "new-token");
        assert_eq!(expire.year(), 2027);
        assert_eq!(seen.lock().unwrap()[0].path, "/api/auth/refresh");

        assert!(c.refresh(&Token::new("dead")).await.unwrap().is_none());
        assert!(c.refresh(&Token::new("other")).await.is_err());
    }

    #[tokio::test]
    async fn check_uses_the_authorization_header() {
        let (base, seen) =
            serve(
                |req| match req.headers.get("authorization").map(String::as_str) {
                    Some("SS14Auth good") => (200, vec![], vec![]),
                    Some("SS14Auth bad") => (401, vec![], vec![]),
                    _ => (500, vec![], vec![]),
                },
            )
            .await;
        let c = client_for(&base);
        assert!(c.check(&Token::new("good")).await.unwrap());
        assert!(!c.check(&Token::new("bad")).await.unwrap());
        assert!(c.check(&Token::new("weird")).await.is_err());
        let req = seen.lock().unwrap()[0].clone();
        assert_eq!(
            (req.method.as_str(), req.path.as_str()),
            ("GET", "/api/auth/ping")
        );

        // A token that can't be a header value is refused locally, not sent.
        assert!(c.check(&Token::new("line\nbreak")).await.is_err());
    }

    #[tokio::test]
    async fn logout_posts_the_token() {
        let (base, seen) = serve(|_| (200, vec![], vec![])).await;
        client_for(&base).logout(&Token::new("t1")).await.unwrap();
        let req = seen.lock().unwrap()[0].clone();
        assert_eq!(req.path, "/api/auth/logout");
        assert_eq!(body_of(&req), json!({"token": "t1"}));

        let (base, _) = serve(|_| (503, vec![], vec![])).await;
        assert!(client_for(&base).logout(&Token::new("t1")).await.is_err());
    }

    #[tokio::test]
    async fn remembers_which_mirror_works() {
        let (good, _) = serve(|_| (200, vec![], vec![])).await;
        let dead = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            format!("http://{}/", l.local_addr().unwrap())
        };
        let c = AuthClient::new(
            build_client().unwrap(),
            UrlFallbackSet::new([dead.clone(), good.clone()]),
        );
        assert_eq!(c.most_successful_url(), dead);
        c.logout(&Token::new("x")).await.unwrap();
        assert_eq!(c.most_successful_url(), good);
    }

    #[tokio::test]
    async fn login_and_refresh_skip_a_5xx_mirror() {
        let (bad, bad_seen) = serve(|_| (500, vec![], b"boom".to_vec())).await;
        let (good, good_seen) = serve(|req| {
            let body = if req.path == "/api/auth/refresh" {
                json!({"newToken": "new-token", "expireTime": "2027-01-01T00:00:00Z"})
                    .to_string()
                    .into_bytes()
            } else {
                ok_login()
            };
            (200, vec![], body)
        })
        .await;
        let c = AuthClient::new(
            build_client().unwrap(),
            UrlFallbackSet::new([bad, good.clone()]),
        );

        let out = c
            .authenticate(Identity::Username("Player"), "pw", None)
            .await
            .unwrap();
        assert!(matches!(out, AuthenticateOutcome::Success(_)));
        assert_eq!(bad_seen.lock().unwrap().len(), 1, "one POST per mirror");
        assert_eq!(good_seen.lock().unwrap().len(), 1);

        let (token, _) = c.refresh(&Token::new("old")).await.unwrap().unwrap();
        assert_eq!(token.expose(), "new-token");
        assert_eq!(bad_seen.lock().unwrap().len(), 2);
        assert_eq!(
            c.most_successful_url(),
            good,
            "a mirror answering 5xx must not become the auth server"
        );
    }
}
