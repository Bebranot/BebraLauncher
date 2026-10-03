//! HTTP client setup and fallback-mirror requests (docs/protocol.md, section 1.1).

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Arc;
use std::time::Duration;

pub use reqwest::Client;
use reqwest::{RequestBuilder, Response};
use tokio::task::JoinSet;

use crate::{Error, Result};

/// How long the first mirror gets before the next one is tried in parallel.
pub const ATTEMPT_DELAY: Duration = Duration::from_secs(3);

pub fn build_client() -> Result<Client> {
    let client = Client::builder()
        .user_agent(concat!("BebraLauncher/", env!("CARGO_PKG_VERSION")))
        .connect_timeout(Duration::from_secs(10))
        .build()?;
    Ok(client)
}

/// A base URL with fallback mirrors. Every URL ends with `/`.
#[derive(Debug, Clone)]
pub struct UrlFallbackSet {
    urls: Vec<String>,
    attempt_delay: Duration,
    /// How often each mirror answered first. Shared by every copy and by `join`ed sets.
    wins: Arc<Vec<AtomicU64>>,
}

impl UrlFallbackSet {
    /// # Panics
    /// If `urls` is empty.
    pub fn new<S: Into<String>>(urls: impl IntoIterator<Item = S>) -> Self {
        let urls: Vec<String> = urls
            .into_iter()
            .map(|u| {
                let mut u: String = u.into();
                if !u.ends_with('/') {
                    u.push('/');
                }
                u
            })
            .collect();
        assert!(!urls.is_empty(), "UrlFallbackSet needs at least one url");
        let wins = Arc::new(urls.iter().map(|_| AtomicU64::new(0)).collect());
        Self {
            urls,
            attempt_delay: ATTEMPT_DELAY,
            wins,
        }
    }

    pub fn with_attempt_delay(mut self, delay: Duration) -> Self {
        self.attempt_delay = delay;
        self
    }

    pub fn urls(&self) -> &[String] {
        &self.urls
    }

    /// Appends `suffix` to every mirror.
    pub fn join(&self, suffix: &str) -> Self {
        Self {
            urls: self.urls.iter().map(|u| format!("{u}{suffix}")).collect(),
            attempt_delay: self.attempt_delay,
            wins: self.wins.clone(),
        }
    }

    /// The mirror that answered first most often (the first one if none has answered yet).
    /// Passed to the game as the auth server so it keeps using what works for us.
    pub fn most_successful(&self) -> &str {
        let mut best = 0;
        for i in 1..self.urls.len() {
            if self.wins[i].load(Ordering::Relaxed) > self.wins[best].load(Ordering::Relaxed) {
                best = i;
            }
        }
        &self.urls[best]
    }

    pub async fn get(&self, client: &Client) -> Result<Response> {
        self.send(|url| client.get(url)).await
    }

    /// Starts with the first mirror. If it has not answered within the attempt delay (or failed),
    /// the next one starts alongside it. The first response wins, whatever its HTTP status;
    /// callers check the status. Losing requests are cancelled.
    pub async fn send<F>(&self, build: F) -> Result<Response>
    where
        F: Fn(&str) -> RequestBuilder,
    {
        let mut tasks: JoinSet<(usize, std::result::Result<Response, reqwest::Error>)> =
            JoinSet::new();
        let mut next = 0;
        let mut last_err: Option<Error> = None;

        loop {
            if tasks.is_empty() {
                if next >= self.urls.len() {
                    break;
                }
                self.spawn_attempt(&mut tasks, &build, next);
                next += 1;
            }

            let more_mirrors = next < self.urls.len();
            let delay = tokio::time::sleep(self.attempt_delay);
            tokio::pin!(delay);

            tokio::select! {
                joined = tasks.join_next() => match joined {
                    Some(Ok((index, Ok(resp)))) => {
                        self.wins[index].fetch_add(1, Ordering::Relaxed);
                        return Ok(resp);
                    }
                    Some(Ok((_, Err(e)))) => last_err = Some(e.into()),
                    Some(Err(e)) => last_err = Some(Error::Other(format!("request task failed: {e}"))),
                    None => {}
                },
                _ = &mut delay, if more_mirrors => {
                    self.spawn_attempt(&mut tasks, &build, next);
                    next += 1;
                }
            }
        }

        Err(last_err.unwrap_or_else(|| Error::Other("no mirrors to try".into())))
    }

    fn spawn_attempt<F>(
        &self,
        tasks: &mut JoinSet<(usize, std::result::Result<Response, reqwest::Error>)>,
        build: &F,
        index: usize,
    ) where
        F: Fn(&str) -> RequestBuilder,
    {
        let request = build(&self.urls[index]).send();
        tasks.spawn(async move { (index, request.await) });
    }
}

#[cfg(test)]
mod tests {
    use std::time::Instant;

    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    use tokio::net::TcpListener;

    use super::*;

    enum Behavior {
        Reply(&'static str),
        Hang,
    }

    /// Minimal HTTP server on a random local port.
    async fn spawn_server(behavior: Behavior) -> String {
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = listener.local_addr().unwrap();
        tokio::spawn(async move {
            let mut held = Vec::new();
            loop {
                let Ok((mut sock, _)) = listener.accept().await else {
                    return;
                };
                match behavior {
                    Behavior::Reply(body) => {
                        tokio::spawn(async move {
                            let mut buf = [0u8; 4096];
                            let _ = sock.read(&mut buf).await;
                            let resp = format!(
                                "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n{body}",
                                body.len()
                            );
                            let _ = sock.write_all(resp.as_bytes()).await;
                        });
                    }
                    Behavior::Hang => held.push(sock),
                }
            }
        });
        format!("http://{addr}/")
    }

    /// A local address nothing listens on: connecting fails immediately.
    async fn dead_url() -> String {
        let l = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let addr = l.local_addr().unwrap();
        drop(l);
        format!("http://{addr}/")
    }

    #[test]
    fn urls_get_trailing_slash_and_join() {
        let set = UrlFallbackSet::new(["https://a.example", "https://b.example/"]);
        assert_eq!(set.urls(), ["https://a.example/", "https://b.example/"]);
        let j = set.join("api/servers");
        assert_eq!(
            j.urls(),
            [
                "https://a.example/api/servers",
                "https://b.example/api/servers"
            ]
        );
    }

    #[tokio::test]
    async fn failed_first_mirror_falls_through_immediately() {
        let good = spawn_server(Behavior::Reply("second")).await;
        let set = UrlFallbackSet::new([dead_url().await, good])
            .with_attempt_delay(Duration::from_secs(5));

        let started = Instant::now();
        let body = set
            .get(&build_client().unwrap())
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_eq!(body, "second");
        assert!(
            started.elapsed() < Duration::from_secs(2),
            "should not wait for the attempt delay"
        );
    }

    #[tokio::test]
    async fn hanging_first_mirror_is_raced_after_delay() {
        let hang = spawn_server(Behavior::Hang).await;
        let good = spawn_server(Behavior::Reply("second")).await;
        let set = UrlFallbackSet::new([hang, good]).with_attempt_delay(Duration::from_millis(150));

        let body = set
            .get(&build_client().unwrap())
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_eq!(body, "second");
    }

    #[tokio::test]
    async fn first_mirror_wins_when_healthy() {
        let first = spawn_server(Behavior::Reply("first")).await;
        let second = spawn_server(Behavior::Reply("second")).await;
        let set = UrlFallbackSet::new([first, second]).with_attempt_delay(Duration::from_secs(5));

        let body = set
            .get(&build_client().unwrap())
            .await
            .unwrap()
            .text()
            .await
            .unwrap();
        assert_eq!(body, "first");
    }

    #[tokio::test]
    async fn most_successful_follows_who_answers() {
        let dead = dead_url().await;
        let good = spawn_server(Behavior::Reply("ok")).await;
        let set = UrlFallbackSet::new([dead.clone(), good.clone()]);
        assert_eq!(
            set.most_successful(),
            dead,
            "first one until someone has answered"
        );

        let client = build_client().unwrap();
        set.get(&client).await.unwrap();
        assert_eq!(set.most_successful(), good);

        // `join`ed sets count towards the same statistics.
        let joined = set.join("x");
        joined.get(&client).await.unwrap();
        assert_eq!(set.most_successful(), good);
    }

    #[tokio::test]
    async fn all_mirrors_dead_is_an_error() {
        let set = UrlFallbackSet::new([dead_url().await, dead_url().await]);
        assert!(set.get(&build_client().unwrap()).await.is_err());
    }
}
