//! Saved accounts and their login tokens (docs/protocol.md, section 7).
//!
//! Tokens are kept in the Windows Credential Manager, one entry per account, under the current
//! Windows user. Passwords are never stored: after a login only the token is kept.
//!
//! This is not stronger than the Windows account itself: any program running as the same user can
//! read these entries. It is still much better than a plain file, and nothing is written to disk
//! by us.

use std::collections::BTreeMap;
use std::sync::Mutex;

use serde::{Deserialize, Serialize};
use time::{Duration, OffsetDateTime};
use uuid::Uuid;

use crate::auth::{AuthClient, AuthenticateOutcome, DenyCode, Identity};
use crate::secret::Token;
use crate::{Error, Result};

/// A token that expires within this long is renewed (like the official launcher).
pub const REFRESH_THRESHOLD: Duration = Duration::days(15);

#[derive(Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct StoredAccount {
    pub user_id: Uuid,
    pub username: String,
    pub token: Token,
    #[serde(with = "time::serde::rfc3339")]
    pub expire_time: OffsetDateTime,
}

impl std::fmt::Debug for StoredAccount {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("StoredAccount")
            .field("user_id", &self.user_id)
            .field("username", &self.username)
            .field("token", &self.token)
            .field("expire_time", &self.expire_time)
            .finish()
    }
}

/// Where accounts are kept.
pub trait TokenStore: Send + Sync {
    /// Adds the account, or replaces the one with the same user id.
    fn save(&self, account: &StoredAccount) -> Result<()>;
    fn list(&self) -> Result<Vec<StoredAccount>>;
    /// Forgets an account. Not an error if it isn't there.
    fn remove(&self, user_id: Uuid) -> Result<()>;
}

/// Accounts in memory only. For tests and tools that must not touch the real credential store.
#[derive(Default)]
pub struct MemoryStore {
    accounts: Mutex<BTreeMap<Uuid, StoredAccount>>,
}

impl TokenStore for MemoryStore {
    fn save(&self, account: &StoredAccount) -> Result<()> {
        self.accounts
            .lock()
            .unwrap()
            .insert(account.user_id, account.clone());
        Ok(())
    }

    fn list(&self) -> Result<Vec<StoredAccount>> {
        Ok(self.accounts.lock().unwrap().values().cloned().collect())
    }

    fn remove(&self, user_id: Uuid) -> Result<()> {
        self.accounts.lock().unwrap().remove(&user_id);
        Ok(())
    }
}

/// Windows Credential Manager. Entries are named `<prefix><user id>`.
#[derive(Debug, Clone)]
pub struct CredentialStore {
    prefix: String,
}

impl Default for CredentialStore {
    fn default() -> Self {
        Self::with_prefix("BebraLauncher/account/")
    }
}

impl CredentialStore {
    pub fn with_prefix(prefix: impl Into<String>) -> Self {
        Self {
            prefix: prefix.into(),
        }
    }

    fn target(&self, user_id: Uuid) -> String {
        format!("{}{user_id}", self.prefix)
    }
}

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

/// Entries are at most 2560 bytes (`CRED_MAX_CREDENTIAL_BLOB_SIZE`).
const MAX_BLOB: usize = 2560;

mod cred {
    //! Thin wrappers over the Win32 credential functions.

    use windows_sys::Win32::Foundation::{GetLastError, ERROR_NOT_FOUND};
    use windows_sys::Win32::Security::Credentials::{
        CredDeleteW, CredEnumerateW, CredFree, CredWriteW, CREDENTIALW, CRED_PERSIST_LOCAL_MACHINE,
        CRED_TYPE_GENERIC,
    };

    use super::{wide, Error, Result, MAX_BLOB};

    pub fn write(target: &str, user_name: &str, blob: &[u8]) -> Result<()> {
        if blob.len() > MAX_BLOB {
            return Err(Error::Other(
                "the account data is too large for the credential store".into(),
            ));
        }
        let mut target = wide(target);
        let mut user = wide(user_name);
        let mut data = blob.to_vec();

        // SAFETY: an all-zero CREDENTIALW is valid (null pointers, zero counts); the pointers we set
        // refer to buffers that live until after the call.
        unsafe {
            let mut c: CREDENTIALW = std::mem::zeroed();
            c.Type = CRED_TYPE_GENERIC;
            c.TargetName = target.as_mut_ptr();
            c.UserName = user.as_mut_ptr();
            c.CredentialBlobSize = data.len() as u32;
            c.CredentialBlob = data.as_mut_ptr();
            c.Persist = CRED_PERSIST_LOCAL_MACHINE;
            if CredWriteW(&c, 0) == 0 {
                return Err(Error::Other(format!(
                    "could not save to the Windows credential store (error {})",
                    GetLastError()
                )));
            }
        }
        Ok(())
    }

    /// All entries whose name starts with `prefix`, as `(target, blob)`.
    pub fn list(prefix: &str) -> Result<Vec<(String, Vec<u8>)>> {
        let filter = wide(&format!("{prefix}*"));
        let mut count = 0u32;
        let mut items: *mut *mut CREDENTIALW = std::ptr::null_mut();

        // SAFETY: on success `items` points to `count` valid CREDENTIALW pointers until CredFree.
        unsafe {
            if CredEnumerateW(filter.as_ptr(), 0, &mut count, &mut items) == 0 {
                let err = GetLastError();
                if err == ERROR_NOT_FOUND {
                    return Ok(Vec::new());
                }
                return Err(Error::Other(format!(
                    "could not read the Windows credential store (error {err})"
                )));
            }
            let mut out = Vec::with_capacity(count as usize);
            for i in 0..count as usize {
                let c = &**items.add(i);
                let name = pwstr_to_string(c.TargetName);
                let blob = if c.CredentialBlob.is_null() {
                    Vec::new()
                } else {
                    std::slice::from_raw_parts(c.CredentialBlob, c.CredentialBlobSize as usize)
                        .to_vec()
                };
                out.push((name, blob));
            }
            CredFree(items as *const _);
            Ok(out)
        }
    }

    pub fn delete(target: &str) -> Result<()> {
        let target = wide(target);
        // SAFETY: `target` is a valid null-terminated string for the duration of the call.
        unsafe {
            if CredDeleteW(target.as_ptr(), CRED_TYPE_GENERIC, 0) == 0 {
                let err = GetLastError();
                if err == ERROR_NOT_FOUND {
                    return Ok(());
                }
                return Err(Error::Other(format!(
                    "could not remove from the Windows credential store (error {err})"
                )));
            }
        }
        Ok(())
    }

    /// # Safety
    /// `p` must be null or a valid null-terminated UTF-16 string.
    unsafe fn pwstr_to_string(p: *const u16) -> String {
        if p.is_null() {
            return String::new();
        }
        let mut len = 0;
        while *p.add(len) != 0 {
            len += 1;
        }
        String::from_utf16_lossy(std::slice::from_raw_parts(p, len))
    }
}

impl TokenStore for CredentialStore {
    fn save(&self, account: &StoredAccount) -> Result<()> {
        let blob = serde_json::to_vec(account)
            .map_err(|e| Error::Other(format!("cannot encode account: {e}")))?;
        cred::write(&self.target(account.user_id), &account.username, &blob)
    }

    fn list(&self) -> Result<Vec<StoredAccount>> {
        let mut accounts: Vec<StoredAccount> = cred::list(&self.prefix)?
            .into_iter()
            // An entry we can't read (damaged, or written by something else) is skipped, not fatal.
            .filter_map(|(_, blob)| serde_json::from_slice(&blob).ok())
            .collect();
        accounts.sort_by_key(|a| a.username.to_lowercase());
        Ok(accounts)
    }

    fn remove(&self, user_id: Uuid) -> Result<()> {
        cred::delete(&self.target(user_id))
    }
}

// ---- using accounts ----

/// How trustworthy a saved token looks, judged by its expiry time alone.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Freshness {
    Fine,
    NeedsRefresh,
    Expired,
}

pub fn freshness(expire_time: OffsetDateTime, now: OffsetDateTime) -> Freshness {
    if expire_time <= now {
        Freshness::Expired
    } else if expire_time <= now + REFRESH_THRESHOLD {
        Freshness::NeedsRefresh
    } else {
        Freshness::Fine
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum AccountStatus {
    /// The server accepts the token.
    Available,
    /// The account has to log in again.
    Expired,
}

#[derive(Debug, Clone)]
pub enum LoginResult {
    /// Logged in and saved.
    Success(StoredAccount),
    Denied {
        errors: Vec<String>,
        code: DenyCode,
    },
}

/// Logs in and, on success, saves the account (replacing an older entry of the same account).
/// The password is only passed on to the server.
pub async fn login(
    auth: &AuthClient,
    store: &dyn TokenStore,
    who: Identity<'_>,
    password: &str,
    tfa_code: Option<&str>,
) -> Result<LoginResult> {
    match auth.authenticate(who, password, tfa_code).await? {
        AuthenticateOutcome::Success(info) => {
            let account = StoredAccount {
                user_id: info.user_id,
                username: info.username,
                token: info.token,
                expire_time: info.expire_time,
            };
            store.save(&account)?;
            Ok(LoginResult::Success(account))
        }
        AuthenticateOutcome::Denied { errors, code } => Ok(LoginResult::Denied { errors, code }),
    }
}

/// Makes sure the account's token is usable, renewing it when it is about to expire.
///
/// - expired by time: no network, the account must log in again
/// - expires within [`REFRESH_THRESHOLD`]: renewed; the new token is saved
/// - otherwise: `verify` decides whether to ask the server if the token is still accepted
///
/// A network error is returned as an error and leaves the saved account untouched.
pub async fn ensure_fresh(
    auth: &AuthClient,
    store: &dyn TokenStore,
    account: StoredAccount,
    now: OffsetDateTime,
    verify: bool,
) -> Result<(StoredAccount, AccountStatus)> {
    match freshness(account.expire_time, now) {
        Freshness::Expired => Ok((account, AccountStatus::Expired)),
        Freshness::NeedsRefresh => match auth.refresh(&account.token).await? {
            Some((token, expire_time)) => {
                let renewed = StoredAccount {
                    token,
                    expire_time,
                    ..account
                };
                store.save(&renewed)?;
                Ok((renewed, AccountStatus::Available))
            }
            None => Ok((account, AccountStatus::Expired)),
        },
        Freshness::Fine => {
            if verify && !auth.check(&account.token).await? {
                return Ok((account, AccountStatus::Expired));
            }
            Ok((account, AccountStatus::Available))
        }
    }
}

/// Logs out on the server (best effort) and forgets the account locally.
/// Returns whether the server was told; the local entry is removed either way.
pub async fn logout(
    auth: &AuthClient,
    store: &dyn TokenStore,
    account: &StoredAccount,
) -> Result<bool> {
    let server_told = auth.logout(&account.token).await.is_ok();
    store.remove(account.user_id)?;
    Ok(server_told)
}

/// Finds an account by name (ignoring case) or by user id.
pub fn find_account<'a>(
    accounts: &'a [StoredAccount],
    selector: &str,
) -> Result<&'a StoredAccount> {
    let selector = selector.trim();
    if let Ok(id) = selector.parse::<Uuid>() {
        if let Some(a) = accounts.iter().find(|a| a.user_id == id) {
            return Ok(a);
        }
    }
    let matches: Vec<&StoredAccount> = accounts
        .iter()
        .filter(|a| a.username.eq_ignore_ascii_case(selector))
        .collect();
    match matches.as_slice() {
        [one] => Ok(one),
        [] => Err(Error::Other(format!(
            "no saved account matches {selector:?}"
        ))),
        _ => Err(Error::Other(format!(
            "several saved accounts are named {selector:?}; use the user id"
        ))),
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;
    use time::macros::datetime;

    use super::*;
    use crate::http::{build_client, UrlFallbackSet};
    use crate::test_util::{serve, Req};

    const NOW: OffsetDateTime = datetime!(2026-10-03 12:00 UTC);

    fn account(name: &str, id: u128, expires: OffsetDateTime) -> StoredAccount {
        StoredAccount {
            user_id: Uuid::from_u128(id),
            username: name.to_string(),
            token: Token::new(format!("token-of-{name}")),
            expire_time: expires,
        }
    }

    fn auth_for(base: &str) -> AuthClient {
        AuthClient::new(
            build_client().unwrap(),
            UrlFallbackSet::new([base.to_string()]),
        )
    }

    // ---- freshness ----

    #[test]
    fn freshness_thresholds() {
        assert_eq!(
            freshness(NOW - Duration::seconds(1), NOW),
            Freshness::Expired
        );
        assert_eq!(freshness(NOW, NOW), Freshness::Expired);
        assert_eq!(
            freshness(NOW + Duration::days(1), NOW),
            Freshness::NeedsRefresh
        );
        assert_eq!(
            freshness(NOW + Duration::days(15), NOW),
            Freshness::NeedsRefresh
        );
        assert_eq!(
            freshness(NOW + Duration::days(15) + Duration::seconds(1), NOW),
            Freshness::Fine
        );
        assert_eq!(freshness(NOW + Duration::days(60), NOW), Freshness::Fine);
    }

    // ---- stores ----

    fn store_contract(store: &dyn TokenStore) {
        assert!(store.list().unwrap().is_empty());

        let a = account("alice", 1, NOW + Duration::days(30));
        let b = account("Bob", 2, NOW + Duration::days(30));
        store.save(&b).unwrap();
        store.save(&a).unwrap();
        let listed = store.list().unwrap();
        assert_eq!(
            listed,
            [a.clone(), b.clone()],
            "sorted by name, ignoring case"
        );

        // Saving again replaces.
        let a2 = StoredAccount {
            token: Token::new("new-token"),
            ..a.clone()
        };
        store.save(&a2).unwrap();
        let listed = store.list().unwrap();
        assert_eq!(listed.len(), 2);
        assert_eq!(listed[0].token.expose(), "new-token");

        store.remove(a.user_id).unwrap();
        store.remove(a.user_id).unwrap(); // already gone: fine
        assert_eq!(store.list().unwrap(), std::slice::from_ref(&b));
        store.remove(b.user_id).unwrap();
        assert!(store.list().unwrap().is_empty());
    }

    #[test]
    fn memory_store_follows_the_contract() {
        store_contract(&MemoryStore::default());
    }

    /// A store entry name that no real account uses, removed again at the end.
    struct TestCredentials(CredentialStore);

    impl TestCredentials {
        fn new(tag: &str) -> Self {
            let prefix = format!("BebraLauncher-test/{tag}-{}/", std::process::id());
            Self(CredentialStore::with_prefix(prefix))
        }
    }

    impl Drop for TestCredentials {
        fn drop(&mut self) {
            for a in self.0.list().unwrap_or_default() {
                let _ = self.0.remove(a.user_id);
            }
        }
    }

    #[test]
    fn credential_manager_follows_the_contract() {
        let t = TestCredentials::new("contract");
        store_contract(&t.0);
    }

    #[test]
    fn credential_manager_survives_junk_entries_and_is_scoped_by_prefix() {
        let t = TestCredentials::new("junk");
        let other = TestCredentials::new("other");

        t.0.save(&account("alice", 1, NOW + Duration::days(30)))
            .unwrap();
        other
            .0
            .save(&account("mallory", 9, NOW + Duration::days(30)))
            .unwrap();
        // Something under our prefix that isn't one of our accounts.
        cred::write(&format!("{}garbage", t.0.prefix), "x", b"this is not json").unwrap();

        let listed = t.0.list().unwrap();
        assert_eq!(listed.len(), 1, "junk is skipped");
        assert_eq!(listed[0].username, "alice");
        assert_eq!(other.0.list().unwrap().len(), 1);
        let _ = cred::delete(&format!("{}garbage", t.0.prefix));
    }

    #[test]
    fn credential_manager_keeps_the_token_intact() {
        let t = TestCredentials::new("roundtrip");
        let mut a = account(
            "ünïcode name",
            7,
            datetime!(2027-01-02 03:04:05.1234567 UTC),
        );
        a.token = Token::new("A".repeat(300));
        t.0.save(&a).unwrap();
        assert_eq!(t.0.list().unwrap(), [a]);
    }

    #[test]
    fn oversized_account_data_is_refused() {
        let t = TestCredentials::new("big");
        let mut a = account("big", 3, NOW);
        a.token = Token::new("x".repeat(5000));
        assert!(t.0.save(&a).is_err());
    }

    // ---- login / refresh / logout ----

    fn login_reply(token: &str) -> Vec<u8> {
        json!({"token": token, "username": "Player", "userId": Uuid::from_u128(5).to_string(),
               "expireTime": "2026-11-03T08:38:01Z"})
        .to_string()
        .into_bytes()
    }

    #[tokio::test]
    async fn login_saves_the_account_and_replaces_older_ones() {
        let (base, _) = serve(|_| (200, vec![], login_reply("t-new"))).await;
        let store = MemoryStore::default();
        store
            .save(&account("Player", 5, NOW + Duration::days(1)))
            .unwrap();

        let r = login(
            &auth_for(&base),
            &store,
            Identity::Username("Player"),
            "pw",
            None,
        )
        .await
        .unwrap();
        let LoginResult::Success(acc) = r else {
            panic!("expected success")
        };
        assert_eq!(acc.token.expose(), "t-new");

        let saved = store.list().unwrap();
        assert_eq!(saved.len(), 1, "same account, one entry");
        assert_eq!(saved[0].token.expose(), "t-new");
    }

    #[tokio::test]
    async fn a_denied_login_saves_nothing() {
        let (base, _) = serve(|_| {
            (
                401,
                vec![],
                json!({"errors": ["bad"], "code": "InvalidCredentials"})
                    .to_string()
                    .into_bytes(),
            )
        })
        .await;
        let store = MemoryStore::default();
        let r = login(
            &auth_for(&base),
            &store,
            Identity::Username("x"),
            "pw",
            None,
        )
        .await
        .unwrap();
        assert!(matches!(
            r,
            LoginResult::Denied {
                code: DenyCode::InvalidCredentials,
                ..
            }
        ));
        assert!(store.list().unwrap().is_empty());
    }

    #[tokio::test]
    async fn expired_tokens_need_no_network() {
        // Nothing listens here: any request would fail.
        let dead = {
            let l = std::net::TcpListener::bind("127.0.0.1:0").unwrap();
            format!("http://{}/", l.local_addr().unwrap())
        };
        let store = MemoryStore::default();
        let (acc, status) = ensure_fresh(
            &auth_for(&dead),
            &store,
            account("a", 1, NOW - Duration::days(1)),
            NOW,
            true,
        )
        .await
        .unwrap();
        assert_eq!(status, AccountStatus::Expired);
        assert_eq!(acc.username, "a");
    }

    #[tokio::test]
    async fn tokens_close_to_expiry_are_renewed_and_saved() {
        let (base, seen) = serve(|_| {
            (
                200,
                vec![],
                json!({"newToken": "renewed", "expireTime": "2027-06-01T00:00:00Z"})
                    .to_string()
                    .into_bytes(),
            )
        })
        .await;
        let store = MemoryStore::default();
        let old = account("a", 1, NOW + Duration::days(3));
        store.save(&old).unwrap();

        let (acc, status) = ensure_fresh(&auth_for(&base), &store, old, NOW, false)
            .await
            .unwrap();
        assert_eq!(status, AccountStatus::Available);
        assert_eq!(acc.token.expose(), "renewed");
        assert_eq!(store.list().unwrap()[0].token.expose(), "renewed", "saved");
        assert_eq!(seen.lock().unwrap()[0].path, "/api/auth/refresh");
    }

    #[tokio::test]
    async fn a_refused_renewal_means_login_again() {
        let (base, _) = serve(|_| (401, vec![], vec![])).await;
        let store = MemoryStore::default();
        let old = account("a", 1, NOW + Duration::days(3));
        store.save(&old).unwrap();
        let (acc, status) = ensure_fresh(&auth_for(&base), &store, old.clone(), NOW, false)
            .await
            .unwrap();
        assert_eq!(status, AccountStatus::Expired);
        assert_eq!(acc, old);
        assert_eq!(
            store.list().unwrap()[0],
            old,
            "the saved entry is left as it was"
        );
    }

    #[tokio::test]
    async fn healthy_tokens_are_only_checked_when_asked() {
        let (base, seen) =
            serve(
                |req: &Req| match req.headers.get("authorization").map(String::as_str) {
                    Some("SS14Auth token-of-ok") => (200, vec![], vec![]),
                    _ => (401, vec![], vec![]),
                },
            )
            .await;
        let store = MemoryStore::default();
        let ok = account("ok", 1, NOW + Duration::days(60));
        let revoked = account("revoked", 2, NOW + Duration::days(60));

        let (_, s) = ensure_fresh(&auth_for(&base), &store, ok.clone(), NOW, false)
            .await
            .unwrap();
        assert_eq!(s, AccountStatus::Available);
        assert!(seen.lock().unwrap().is_empty(), "no request without verify");

        let (_, s) = ensure_fresh(&auth_for(&base), &store, ok, NOW, true)
            .await
            .unwrap();
        assert_eq!(s, AccountStatus::Available);
        let (_, s) = ensure_fresh(&auth_for(&base), &store, revoked, NOW, true)
            .await
            .unwrap();
        assert_eq!(s, AccountStatus::Expired);
    }

    #[tokio::test]
    async fn network_errors_do_not_touch_the_saved_account() {
        let (base, _) = serve(|_| (500, vec![], vec![])).await;
        let store = MemoryStore::default();
        let old = account("a", 1, NOW + Duration::days(3));
        store.save(&old).unwrap();
        assert!(
            ensure_fresh(&auth_for(&base), &store, old.clone(), NOW, false)
                .await
                .is_err()
        );
        assert_eq!(store.list().unwrap(), [old]);
    }

    #[tokio::test]
    async fn logout_forgets_the_account_even_if_the_server_cannot_be_told() {
        let store = MemoryStore::default();
        let acc = account("a", 1, NOW + Duration::days(30));
        store.save(&acc).unwrap();

        let (down, _) = serve(|_| (503, vec![], vec![])).await;
        assert!(!logout(&auth_for(&down), &store, &acc).await.unwrap());
        assert!(store.list().unwrap().is_empty());

        store.save(&acc).unwrap();
        let (up, seen) = serve(|_| (200, vec![], vec![])).await;
        assert!(logout(&auth_for(&up), &store, &acc).await.unwrap());
        assert!(store.list().unwrap().is_empty());
        assert_eq!(
            serde_json::from_slice::<serde_json::Value>(&seen.lock().unwrap()[0].body).unwrap(),
            json!({"token": "token-of-a"})
        );
    }

    // ---- picking an account ----

    #[test]
    fn accounts_are_found_by_name_or_id() {
        let list = [
            account("Alice", 1, NOW),
            account("Bob", 2, NOW),
            account("bob", 3, NOW),
        ];
        assert_eq!(
            find_account(&list, "alice").unwrap().user_id,
            Uuid::from_u128(1)
        );
        assert_eq!(
            find_account(&list, " ALICE ").unwrap().user_id,
            Uuid::from_u128(1)
        );
        assert_eq!(
            find_account(&list, &Uuid::from_u128(3).to_string())
                .unwrap()
                .username,
            "bob"
        );
        assert!(
            find_account(&list, "bob").is_err(),
            "two accounts named bob"
        );
        assert!(find_account(&list, "carol").is_err());
        assert!(find_account(&[], "x").is_err());
    }

    #[test]
    fn secrets_stay_out_of_debug_output() {
        let a = account("a", 1, NOW);
        assert!(!format!("{a:?}").contains("token-of-a"));
    }
}
