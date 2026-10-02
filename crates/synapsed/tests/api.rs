// SPDX-License-Identifier: MIT OR Apache-2.0
//! `synapsed` over real loopback HTTP (M5a), including the PM's security conditions:
//! claims prove the role key, Host-header check, no CORS, Bearer on every authenticated route,
//! constant-time token handling, and no token or key material in responses.

use std::io::{Read, Write};
use std::net::SocketAddr;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chrono::{DateTime, Utc};
use serde_json::{Value, json};
use synapse::certificate::chain_to_pem;
use synapse::keystore::Keystore;
use synapse::roles::sign_claim;
use synapsed::{Daemon, DaemonConfig, DaemonError};

struct Running {
    addr: SocketAddr,
    home: tempfile::TempDir,
    started_at: DateTime<Utc>,
    stop: Option<tokio::sync::oneshot::Sender<()>>,
    task: Option<tokio::task::JoinHandle<std::io::Result<()>>>,
}

impl Running {
    fn url(&self, path: &str) -> String {
        format!("http://{}{path}", self.addr)
    }

    async fn stop(mut self) {
        if let Some(tx) = self.stop.take() {
            let _ = tx.send(());
        }
        if let Some(task) = self.task.take() {
            let _ = task.await;
        }
    }
}

fn config(home: &std::path::Path) -> DaemonConfig {
    DaemonConfig {
        home: home.to_path_buf(),
        addr: "127.0.0.1:0".parse().unwrap(),
        sweep_every: Duration::from_secs(3600),
    }
}

async fn start() -> Running {
    let home = tempfile::tempdir().unwrap();
    Keystore::init_account(home.path(), "acct").expect("init account");
    let daemon = Daemon::open(&config(home.path())).expect("open daemon");
    let started_at = daemon.started_at();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(daemon.serve(listener, async move {
        let _ = rx.await;
    }));
    Running {
        addr,
        home,
        started_at,
        stop: Some(tx),
        task: Some(task),
    }
}

fn client() -> reqwest::Client {
    reqwest::Client::new()
}

/// A wire-format claim for `role`, signed after the daemon started (M3 refuses claims signed at or
/// before the table's start).
fn claim_body(d: &Running, role: &str, nonce: u8) -> Value {
    let store = Keystore::open(d.home.path()).unwrap();
    // The certificate is issued at the real time; only the claim is signed after the daemon's
    // start (a certificate issued in the daemon's future would be "not yet valid").
    let identity = store.role(role, Utc::now()).unwrap();
    let signed_at = Utc::now().max(d.started_at + chrono::Duration::seconds(1));
    let req = sign_claim(&identity, [nonce; 16], signed_at).unwrap();
    json!({
        "chain_pem": chain_to_pem(&req.chain),
        "nonce_hex": req.nonce.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        "signed_at": req.signed_at.to_rfc3339(),
        "signature_b64": B64.encode(&req.signature),
    })
}

async fn claim(d: &Running, role: &str, nonce: u8) -> (String, u64) {
    let (status, reply, text) = post(d, "/v1/claim", None, claim_body(d, role, nonce)).await;
    assert_eq!(status, 200, "claim refused: {text}");
    (
        reply["session"].as_str().expect("a session token").to_string(),
        reply["epoch"].as_u64().expect("an epoch"),
    )
}

async fn post(d: &Running, path: &str, token: Option<&str>, body: Value) -> (u16, Value, String) {
    let mut req = client().post(d.url(path)).json(&body);
    if let Some(t) = token {
        req = req.bearer_auth(t);
    }
    let resp = req.send().await.unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap();
    let value = serde_json::from_str(&text).unwrap_or(Value::Null);
    (status, value, text)
}

#[tokio::test]
async fn health_answers_without_a_token() {
    let d = start().await;
    let reply: Value = client().get(d.url("/v1/health")).send().await.unwrap().json().await.unwrap();
    assert_eq!(reply["ok"], true);
    assert!(reply["started_at"].is_string());
    d.stop().await;
}

#[tokio::test]
async fn a_round_trip_between_two_roles() {
    let d = start().await;
    let (alpha, _) = claim(&d, "alpha", 1).await;
    let (beta, _) = claim(&d, "beta", 2).await;
    let body = B64.encode(b"hello beta");
    let (s, sent, _) = post(&d, "/v1/send", Some(&alpha), json!({"to": "beta@acct", "body_b64": body})).await;
    assert_eq!(s, 200, "{sent}");
    assert_eq!(sent["outcome"], "queued");
    let id = sent["message_id"].as_str().unwrap().to_string();

    let (s, got, _) = post(&d, "/v1/fetch", Some(&beta), json!({})).await;
    assert_eq!(s, 200, "{got}");
    let msg = &got["messages"][0];
    assert_eq!(msg["message_id"], id.as_str());
    assert_eq!(msg["from"], "alpha@acct", "from is the sender's verified role");
    assert_eq!(B64.decode(msg["body_b64"].as_str().unwrap()).unwrap(), b"hello beta");

    let (s, acked, _) = post(&d, "/v1/ack", Some(&beta), json!({"message_id": id})).await;
    assert_eq!(s, 200, "{acked}");
    assert_eq!(acked["outcome"], "removed");
    d.stop().await;
}

#[tokio::test]
async fn a_forged_from_is_refused() {
    let d = start().await;
    let (alpha, _) = claim(&d, "alpha", 1).await;
    let (s, _, _) = post(
        &d,
        "/v1/send",
        Some(&alpha),
        json!({"to": "beta@acct", "from": "beta@acct", "body_b64": B64.encode(b"x")}),
    )
    .await;
    assert_eq!(s, 400, "a client-supplied from must be refused");
    d.stop().await;
}

#[tokio::test]
async fn a_bad_or_missing_token_is_401() {
    let d = start().await;
    let bad = "00".repeat(32);
    for path in ["/v1/send", "/v1/fetch", "/v1/ack", "/v1/heartbeat"] {
        for token in [None, Some(bad.as_str()), Some("not-hex")] {
            let (s, _, _) = post(&d, path, token, json!({})).await;
            assert_eq!(s, 401, "{path} with token {token:?}");
        }
    }
    let s = client().get(d.url("/v1/list")).send().await.unwrap().status().as_u16();
    assert_eq!(s, 401, "/v1/list without a token");
    d.stop().await;
}

#[tokio::test]
async fn an_old_epoch_token_is_409_after_takeover() {
    let d = start().await;
    let (old, first) = claim(&d, "alpha", 1).await;
    let (_new, second) = claim(&d, "alpha", 2).await;
    assert_eq!(second, first + 1);
    let (s, reply, _) = post(&d, "/v1/send", Some(&old), json!({"to": "beta@acct", "body_b64": ""})).await;
    assert_eq!(s, 409, "{reply}");
    assert_eq!(reply["error"], "superseded");
    assert_eq!(reply["current"], second);
    d.stop().await;
}

#[tokio::test]
async fn a_claim_without_key_proof_issues_no_token() {
    let d = start().await;
    let mut body = claim_body(&d, "alpha", 1);
    let mut sig = B64.decode(body["signature_b64"].as_str().unwrap()).unwrap();
    sig[0] ^= 1;
    body["signature_b64"] = json!(B64.encode(&sig));
    let (s, reply, _) = post(&d, "/v1/claim", None, body).await;
    assert_eq!(s, 403, "{reply}");
    assert_eq!(reply["error"], "claim_refused");
    assert!(reply.get("session").is_none(), "no token for an unproven claim");
    d.stop().await;
}

/// Raw HTTP, so the Host header is exactly what the test says.
fn raw(addr: SocketAddr, request: &str) -> String {
    let mut s = std::net::TcpStream::connect(addr).unwrap();
    s.set_read_timeout(Some(Duration::from_secs(5))).unwrap();
    s.write_all(request.as_bytes()).unwrap();
    let mut out = String::new();
    let _ = s.read_to_string(&mut out);
    out
}

#[tokio::test]
async fn a_wrong_host_header_is_421() {
    let d = start().await;
    let addr = d.addr;
    let port = addr.port();
    let cases = [
        format!("evil.example:{port}"),
        format!("127.0.0.1:{}", port.wrapping_add(1)),
        "127.0.0.1".to_string(),
    ];
    for host in cases {
        let req = format!("GET /v1/health HTTP/1.1\r\nHost: {host}\r\nConnection: close\r\n\r\n");
        let reply = tokio::task::spawn_blocking(move || raw(addr, &req)).await.unwrap();
        assert!(reply.starts_with("HTTP/1.1 421"), "Host {host}: {reply}");
    }
    let good = format!("GET /v1/health HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n");
    let reply = tokio::task::spawn_blocking(move || raw(addr, &good)).await.unwrap();
    assert!(reply.starts_with("HTTP/1.1 200"), "control, localhost:port: {reply}");
    d.stop().await;
}

#[tokio::test]
async fn no_response_carries_cors_headers() {
    let d = start().await;
    let (alpha, _) = claim(&d, "alpha", 1).await;
    let responses = vec![
        client().get(d.url("/v1/health")).header("Origin", "https://evil.example").send().await.unwrap(),
        client()
            .request(reqwest::Method::OPTIONS, d.url("/v1/send"))
            .header("Origin", "https://evil.example")
            .header("Access-Control-Request-Method", "POST")
            .send()
            .await
            .unwrap(),
        client()
            .post(d.url("/v1/fetch"))
            .bearer_auth(&alpha)
            .header("Origin", "https://evil.example")
            .json(&json!({}))
            .send()
            .await
            .unwrap(),
    ];
    for r in responses {
        let cors: Vec<_> = r
            .headers()
            .keys()
            .filter(|k| k.as_str().starts_with("access-control-"))
            .collect();
        assert!(cors.is_empty(), "CORS headers on {}: {cors:?}", r.url());
    }
    d.stop().await;
}

#[tokio::test]
async fn a_post_without_json_content_type_is_415() {
    let d = start().await;
    let (alpha, _) = claim(&d, "alpha", 1).await;
    let s = client()
        .post(d.url("/v1/fetch"))
        .bearer_auth(&alpha)
        .header("Content-Type", "text/plain")
        .body("{}")
        .send()
        .await
        .unwrap()
        .status()
        .as_u16();
    assert_eq!(s, 415);
    d.stop().await;
}

#[test]
fn a_non_loopback_bind_is_refused() {
    let home = tempfile::tempdir().unwrap();
    Keystore::init_account(home.path(), "acct").unwrap();
    let mut cfg = config(home.path());
    cfg.addr = "0.0.0.0:7920".parse().unwrap();
    assert!(matches!(Daemon::open(&cfg), Err(DaemonError::NotLoopback)));
}

#[tokio::test]
async fn a_second_daemon_on_the_same_home_fails() {
    let d = start().await;
    assert!(Daemon::open(&config(d.home.path())).is_err(), "the store lock keeps one daemon");
    d.stop().await;
}

#[tokio::test]
async fn no_token_or_key_material_leaks() {
    let d = start().await;
    let (alpha, _) = claim(&d, "alpha", 1).await;
    let (_beta, _) = claim(&d, "beta", 2).await;
    let mut bodies = Vec::new();
    for (path, body) in [
        ("/v1/send", json!({"to": "beta@acct", "body_b64": B64.encode(b"x")})),
        ("/v1/fetch", json!({})),
        ("/v1/heartbeat", json!({"summary": "working"})),
        ("/v1/ack", json!({"message_id": "nope"})),
        ("/v1/send", json!({"to": "not a role", "body_b64": ""})),
    ] {
        bodies.push(post(&d, path, Some(&alpha), body).await.2);
    }
    bodies.push(
        client().get(d.url("/v1/list")).bearer_auth(&alpha).send().await.unwrap().text().await.unwrap(),
    );
    for text in bodies {
        assert!(!text.contains(&alpha), "a response carried the session token: {text}");
        assert!(!text.contains("PRIVATE KEY"), "a response carried key material: {text}");
    }
    d.stop().await;
}

#[tokio::test]
async fn list_shows_claimed_roles_with_presence() {
    let d = start().await;
    let (alpha, _) = claim(&d, "alpha", 1).await;
    let (s, _, _) = post(&d, "/v1/heartbeat", Some(&alpha), json!({"summary": "writing M5"})).await;
    assert_eq!(s, 200);
    let reply: Value = client().get(d.url("/v1/list")).bearer_auth(&alpha).send().await.unwrap().json().await.unwrap();
    let roles = reply["roles"].as_array().unwrap();
    let me = roles.iter().find(|r| r["global_id"] == "alpha@acct").expect("alpha listed");
    assert_eq!(me["online"], true);
    assert_eq!(me["summary"], "writing M5");
    d.stop().await;
}
