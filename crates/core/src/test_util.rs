//! A throwaway local HTTP server for tests.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

#[derive(Clone, Debug)]
pub(crate) struct Req {
    pub method: String,
    pub path: String,
    pub headers: HashMap<String, String>,
    pub body: Vec<u8>,
}

pub(crate) type Reply = (u16, Vec<(&'static str, String)>, Vec<u8>);

/// Runs on a random port and answers every request with what `handler` returns
/// (status, extra headers, body). Returns the base URL and every request seen.
pub(crate) async fn serve(
    handler: impl Fn(&Req) -> Reply + Send + Sync + 'static,
) -> (String, Arc<Mutex<Vec<Req>>>) {
    serve_with_base(|_| handler).await
}

/// Like [`serve`], but the handler is built from the server's own base URL, for replies that
/// must point back at the server (indexes with download links).
pub(crate) async fn serve_with_base<H>(
    make: impl FnOnce(String) -> H,
) -> (String, Arc<Mutex<Vec<Req>>>)
where
    H: Fn(&Req) -> Reply + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let base = format!("http://{}/", listener.local_addr().unwrap());
    let handler = make(base.clone());
    let seen = Arc::new(Mutex::new(Vec::new()));
    let handler = Arc::new(handler);
    let log = seen.clone();
    tokio::spawn(async move {
        loop {
            let Ok((mut sock, _)) = listener.accept().await else {
                return;
            };
            let handler = handler.clone();
            let log = log.clone();
            tokio::spawn(async move {
                let mut buf = Vec::new();
                let mut chunk = [0u8; 8192];
                let header_end = loop {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        return;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                    if let Some(p) = buf.windows(4).position(|w| w == b"\r\n\r\n") {
                        break p + 4;
                    }
                };
                let head = String::from_utf8_lossy(&buf[..header_end]).to_string();
                let mut lines = head.lines();
                let request_line = lines.next().unwrap().to_string();
                let mut parts = request_line.split(' ');
                let method = parts.next().unwrap().to_string();
                let path = parts.next().unwrap_or("/").to_string();
                let headers: HashMap<String, String> = lines
                    .filter_map(|l| l.split_once(':'))
                    .map(|(k, v)| (k.trim().to_lowercase(), v.trim().to_string()))
                    .collect();
                let want: usize = headers
                    .get("content-length")
                    .and_then(|v| v.parse().ok())
                    .unwrap_or(0);
                while buf.len() < header_end + want {
                    let n = sock.read(&mut chunk).await.unwrap_or(0);
                    if n == 0 {
                        break;
                    }
                    buf.extend_from_slice(&chunk[..n]);
                }
                let req = Req {
                    method,
                    path,
                    headers,
                    body: buf[header_end..].to_vec(),
                };
                let (status, extra, body) = handler(&req);
                log.lock().unwrap().push(req);

                let mut resp = format!(
                    "HTTP/1.1 {status} X\r\nContent-Length: {}\r\nConnection: close\r\n",
                    body.len()
                );
                for (k, v) in extra {
                    resp.push_str(&format!("{k}: {v}\r\n"));
                }
                resp.push_str("\r\n");
                let _ = sock.write_all(resp.as_bytes()).await;
                let _ = sock.write_all(&body).await;
            });
        }
    });
    (base, seen)
}
