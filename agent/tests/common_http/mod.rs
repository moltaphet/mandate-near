//! Loopback public-protocol fixture; never accesses account signing configuration.

use near_sdk::serde_json::{Value, json};
use std::sync::{Arc, Mutex};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;

pub struct Mock {
    pub url: String,
    pub requests: Arc<Mutex<Vec<Value>>>,
    pub task: tokio::task::JoinHandle<()>,
}

pub async fn server<F>(handler: F) -> Mock
where
    F: Fn(Value) -> Value + Send + Sync + 'static,
{
    let listener = TcpListener::bind("127.0.0.1:0").await.expect("loopback HTTP fixture can bind");
    let url = format!("http://{}/rpc", listener.local_addr().unwrap());
    let requests = Arc::new(Mutex::new(Vec::new()));
    let captured = requests.clone();
    let task = tokio::spawn(async move {
        loop {
            let (mut socket, _) = listener.accept().await.unwrap();
            let mut bytes = Vec::new();
            let mut end = None;
            while end.is_none() {
                let mut part = [0u8; 1024];
                let n = socket.read(&mut part).await.unwrap();
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&part[..n]);
                end = bytes.windows(4).position(|w| w == b"\r\n\r\n").map(|i| i + 4);
                assert!(bytes.len() <= 128 * 1024, "fixture request is bounded");
            }
            let Some(end) = end else { continue };
            let headers = std::str::from_utf8(&bytes[..end]).unwrap().to_ascii_lowercase();
            let length: usize = headers
                .lines()
                .find_map(|line| line.strip_prefix("content-length:"))
                .unwrap()
                .trim()
                .parse()
                .unwrap();
            while bytes.len() < end + length {
                let mut part = [0u8; 4096];
                let n = socket.read(&mut part).await.unwrap();
                if n == 0 {
                    break;
                }
                bytes.extend_from_slice(&part[..n]);
            }
            let body: Value = near_sdk::serde_json::from_slice(&bytes[end..end + length]).unwrap();
            let mut evidence = body.clone();
            evidence["_fixture_has_auth"] =
                json!(headers.lines().any(|line| line.starts_with("x-api-key:")));
            captured.lock().unwrap().push(evidence);
            let response = handler(body);
            let response = near_sdk::serde_json::to_vec(&response).unwrap();
            let header = format!(
                "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                response.len()
            );
            socket.write_all(header.as_bytes()).await.unwrap();
            socket.write_all(&response).await.unwrap();
        }
    });
    Mock { url, requests, task }
}

pub fn result(value: Value) -> Value {
    json!({"jsonrpc":"2.0","id":1,"result":value})
}
