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
use synapse::roles::{sign_claim, sign_claim_for};
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
    // The daemon announces its address and instance id before serving; claims need the id.
    let announce = home.path().join("synapsed.json");
    for _ in 0..200 {
        if announce.exists() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(10)).await;
    }
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
    let req = sign_claim_for(&identity, &instance_id(d), [nonce; 16], signed_at).unwrap();
    json!({
        "audience": req.audience,
        "chain_pem": chain_to_pem(&req.chain),
        "nonce_hex": req.nonce.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        "signed_at": req.signed_at.to_rfc3339(),
        "signature_b64": B64.encode(&req.signature),
    })
}

/// The daemon's instance id, as a same-user client learns it: from the owner-only announce file.
fn instance_id(d: &Running) -> String {
    let text = std::fs::read_to_string(d.home.path().join("synapsed.json")).expect("announce file");
    let v: Value = serde_json::from_str(&text).unwrap();
    v["instance_id"].as_str().unwrap().to_string()
}

async fn claim(d: &Running, role: &str, nonce: u8) -> (String, u64) {
    let (status, reply, text) = post(d, "/v1/claim", None, claim_body(d, role, nonce)).await;
    assert_eq!(status, 200, "claim refused: {text}");
    (
        reply["session"]
            .as_str()
            .expect("a session token")
            .to_string(),
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
    let reply: Value = client()
        .get(d.url("/v1/health"))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    assert_eq!(reply["ok"], true);
    assert!(reply["started_at"].is_string());
    assert!(reply.get("proof").is_none(), "no challenge, no proof");
    d.stop().await;
}

fn unhex(s: &str) -> Vec<u8> {
    (0..s.len())
        .step_by(2)
        .map(|i| u8::from_str_radix(&s[i..i + 2], 16).unwrap())
        .collect()
}

/// Whether `proof` is HMAC-SHA256(instance id, message for `addr` and `challenge`).
fn proof_holds(instance_id: &str, addr: SocketAddr, challenge: &[u8; 16], proof: &str) -> bool {
    let key = ring::hmac::Key::new(ring::hmac::HMAC_SHA256, &unhex(instance_id));
    let msg = synapsed::health_proof_message(addr, challenge);
    ring::hmac::verify(&key, &msg, &unhex(proof)).is_ok()
}

/// #74: a challenged health proves the daemon holds the announced instance id, bound to the
/// challenge and to the address it serves on.
#[tokio::test]
async fn health_proves_the_instance_id() {
    let d = start().await;
    let id = instance_id(&d);
    let challenge = [0x5a; 16];
    let hexed: String = challenge.iter().map(|b| format!("{b:02x}")).collect();
    let reply: Value = client()
        .get(d.url(&format!("/v1/health?challenge={hexed}")))
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let proof = reply["proof"].as_str().expect("a proof");
    assert!(proof_holds(&id, d.addr, &challenge, proof), "{reply}");
    // The same proof is worthless for another challenge, another address, or another instance.
    assert!(!proof_holds(&id, d.addr, &[0xa5; 16], proof));
    let elsewhere: SocketAddr = format!("127.0.0.1:{}", d.addr.port() ^ 1).parse().unwrap();
    assert!(!proof_holds(&id, elsewhere, &challenge, proof));
    assert!(!proof_holds(&"00".repeat(16), d.addr, &challenge, proof));

    for bad in [
        "challenge=",
        "challenge=zz",
        "challenge=+aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa",
        "challenge=5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a5a",
        "other=1",
    ] {
        let resp = client()
            .get(d.url(&format!("/v1/health?{bad}")))
            .send()
            .await
            .unwrap();
        assert_eq!(resp.status(), 400, "{bad}");
        let body: Value = resp.json().await.unwrap();
        assert_eq!(body["error"], "malformed", "{bad}");
    }
    d.stop().await;
}

/// #74: the daemon removes its announce file when it stops, but only while the file still names it.
#[tokio::test]
async fn stopping_removes_only_its_own_announce_file() {
    // Stop by hand rather than with `Running::stop`, which would drop the home with the file.
    async fn stop_keeping_home(d: Running) -> tempfile::TempDir {
        let Running {
            home, stop, task, ..
        } = d;
        stop.unwrap().send(()).unwrap();
        task.unwrap().await.unwrap().unwrap();
        home
    }

    let d = start().await;
    let path = d.home.path().join("synapsed.json");
    assert!(path.exists(), "announced while serving");
    let home = stop_keeping_home(d).await;
    assert!(!path.exists(), "removed on graceful stop");
    drop(home);

    // A file that names another instance is not ours to remove.
    let d = start().await;
    let path = d.home.path().join("synapsed.json");
    let other =
        json!({"addr": "127.0.0.1:1", "instance_id": "00".repeat(16), "pid": 1}).to_string();
    std::fs::write(&path, &other).unwrap();
    let home = stop_keeping_home(d).await;
    assert_eq!(std::fs::read_to_string(&path).unwrap(), other);
    drop(home);
}

#[tokio::test]
async fn a_round_trip_between_two_roles() {
    let d = start().await;
    let (alpha, _) = claim(&d, "alpha", 1).await;
    let (beta, _) = claim(&d, "beta", 2).await;
    let body = B64.encode(b"hello beta");
    let (s, sent, _) = post(
        &d,
        "/v1/send",
        Some(&alpha),
        json!({"to": "beta@acct", "body_b64": body}),
    )
    .await;
    assert_eq!(s, 200, "{sent}");
    assert_eq!(sent["outcome"], "queued");
    let id = sent["message_id"].as_str().unwrap().to_string();

    let (s, got, _) = post(&d, "/v1/fetch", Some(&beta), json!({})).await;
    assert_eq!(s, 200, "{got}");
    let msg = &got["messages"][0];
    assert_eq!(msg["message_id"], id.as_str());
    assert_eq!(
        msg["from"], "alpha@acct",
        "from is the sender's verified role"
    );
    assert_eq!(
        B64.decode(msg["body_b64"].as_str().unwrap()).unwrap(),
        b"hello beta"
    );

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
        let plus = format!("+a{}", "0".repeat(62)); // u8::from_str_radix accepts "+a"
        for token in [
            None,
            Some(bad.as_str()),
            Some("not-hex"),
            Some(plus.as_str()),
        ] {
            let (s, _, _) = post(&d, path, token, json!({})).await;
            assert_eq!(s, 401, "{path} with token {token:?}");
        }
    }
    let s = client()
        .get(d.url("/v1/list"))
        .send()
        .await
        .unwrap()
        .status()
        .as_u16();
    assert_eq!(s, 401, "/v1/list without a token");
    d.stop().await;
}

#[tokio::test]
async fn an_old_epoch_token_is_409_after_takeover() {
    let d = start().await;
    let (old, first) = claim(&d, "alpha", 1).await;
    let (_new, second) = claim(&d, "alpha", 2).await;
    assert_eq!(second, first + 1);
    let (s, reply, _) = post(
        &d,
        "/v1/send",
        Some(&old),
        json!({"to": "beta@acct", "body_b64": ""}),
    )
    .await;
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
    assert!(
        reply.get("session").is_none(),
        "no token for an unproven claim"
    );
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
        let reply = tokio::task::spawn_blocking(move || raw(addr, &req))
            .await
            .unwrap();
        assert!(reply.starts_with("HTTP/1.1 421"), "Host {host}: {reply}");
    }
    let good =
        format!("GET /v1/health HTTP/1.1\r\nHost: localhost:{port}\r\nConnection: close\r\n\r\n");
    let reply = tokio::task::spawn_blocking(move || raw(addr, &good))
        .await
        .unwrap();
    assert!(
        reply.starts_with("HTTP/1.1 200"),
        "control, localhost:port: {reply}"
    );
    d.stop().await;
}

#[tokio::test]
async fn no_response_carries_cors_headers() {
    let d = start().await;
    let (alpha, _) = claim(&d, "alpha", 1).await;
    let responses = vec![
        client()
            .get(d.url("/v1/health"))
            .header("Origin", "https://evil.example")
            .send()
            .await
            .unwrap(),
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
    assert!(
        Daemon::open(&config(d.home.path())).is_err(),
        "the store lock keeps one daemon"
    );
    d.stop().await;
}

#[tokio::test]
async fn no_token_or_key_material_leaks() {
    let d = start().await;
    let (alpha, _) = claim(&d, "alpha", 1).await;
    let (_beta, _) = claim(&d, "beta", 2).await;
    let mut bodies = Vec::new();
    for (path, body) in [
        (
            "/v1/send",
            json!({"to": "beta@acct", "body_b64": B64.encode(b"x")}),
        ),
        ("/v1/fetch", json!({})),
        ("/v1/heartbeat", json!({"summary": "working"})),
        ("/v1/ack", json!({"message_id": "nope"})),
        ("/v1/send", json!({"to": "not a role", "body_b64": ""})),
    ] {
        bodies.push(post(&d, path, Some(&alpha), body).await.2);
    }
    bodies.push(
        client()
            .get(d.url("/v1/list"))
            .bearer_auth(&alpha)
            .send()
            .await
            .unwrap()
            .text()
            .await
            .unwrap(),
    );
    for text in bodies {
        assert!(
            !text.contains(&alpha),
            "a response carried the session token: {text}"
        );
        assert!(
            !text.contains("PRIVATE KEY"),
            "a response carried key material: {text}"
        );
    }
    d.stop().await;
}

#[tokio::test]
async fn list_shows_claimed_roles_with_presence() {
    let d = start().await;
    let (alpha, _) = claim(&d, "alpha", 1).await;
    let (s, _, _) = post(
        &d,
        "/v1/heartbeat",
        Some(&alpha),
        json!({"summary": "writing M5"}),
    )
    .await;
    assert_eq!(s, 200);
    let reply: Value = client()
        .get(d.url("/v1/list"))
        .bearer_auth(&alpha)
        .send()
        .await
        .unwrap()
        .json()
        .await
        .unwrap();
    let roles = reply["roles"].as_array().unwrap();
    let me = roles
        .iter()
        .find(|r| r["global_id"] == "alpha@acct")
        .expect("alpha listed");
    assert_eq!(me["online"], true);
    assert_eq!(me["summary"], "writing M5");
    d.stop().await;
}

// ---- Final-review fixes (Opus review of M5a) ----

/// Review I3: a claim is bound to this daemon's instance id. A claim for another audience, or an
/// unbound v1 claim, gets no token, so a claim captured by a port-squatter can't be relayed here.
#[tokio::test]
async fn a_claim_for_another_daemon_or_no_daemon_is_refused() {
    let d = start().await;
    let store = Keystore::open(d.home.path()).unwrap();
    let identity = store.role("alpha", Utc::now()).unwrap();
    let signed_at = Utc::now().max(d.started_at + chrono::Duration::seconds(1));
    for req in [
        sign_claim_for(&identity, "another-daemon", [7; 16], signed_at).unwrap(),
        sign_claim(&identity, [8; 16], signed_at).unwrap(),
    ] {
        let body = json!({
            "audience": req.audience,
            "chain_pem": chain_to_pem(&req.chain),
            "nonce_hex": req.nonce.iter().map(|b| format!("{b:02x}")).collect::<String>(),
            "signed_at": req.signed_at.to_rfc3339(),
            "signature_b64": B64.encode(&req.signature),
        });
        let (s, reply, _) = post(&d, "/v1/claim", None, body).await;
        assert_eq!(s, 403, "audience {:?}: {reply}", req.audience);
        assert!(reply.get("session").is_none());
    }
    d.stop().await;
}

/// Review I3: the instance id is announced only through an owner-only file, never over HTTP.
#[tokio::test]
async fn the_instance_id_is_announced_only_in_an_owner_only_file() {
    let d = start().await;
    let id = instance_id(&d);
    assert_eq!(id.len(), 32);
    let health = client()
        .get(d.url("/v1/health"))
        .send()
        .await
        .unwrap()
        .text()
        .await
        .unwrap();
    assert!(
        !health.contains(&id),
        "health must not reveal the instance id"
    );
    #[cfg(unix)]
    {
        use std::os::unix::fs::PermissionsExt;
        let mode = std::fs::metadata(d.home.path().join("synapsed.json"))
            .unwrap()
            .permissions()
            .mode();
        assert_eq!(mode & 0o777, 0o600);
    }
    d.stop().await;
}

/// Review I1: message ids are scoped by sender, so one role can't take another's id first.
#[tokio::test]
async fn a_squatted_message_id_does_not_suppress_another_senders_message() {
    let d = start().await;
    let (alpha, _) = claim(&d, "alpha", 1).await;
    let (beta, _) = claim(&d, "beta", 2).await;
    let (gamma, _) = claim(&d, "gamma", 3).await;
    let send = |token: String, body: &'static [u8]| {
        let d = &d;
        async move {
            post(
                d,
                "/v1/send",
                Some(&token),
                json!({"to": "beta@acct", "message_id": "m1", "body_b64": B64.encode(body)}),
            )
            .await
        }
    };
    let (_, g, _) = send(gamma.clone(), b"forged").await;
    let (_, a, _) = send(alpha.clone(), b"real").await;
    assert_eq!(g["outcome"], "queued");
    assert_eq!(
        a["outcome"], "queued",
        "alpha's m1 must not be a duplicate of gamma's: {a}"
    );
    assert_ne!(a["message_id"], g["message_id"]);
    let (_, got, _) = post(&d, "/v1/fetch", Some(&beta), json!({})).await;
    let froms: Vec<&str> = got["messages"]
        .as_array()
        .unwrap()
        .iter()
        .map(|m| m["from"].as_str().unwrap())
        .collect();
    assert_eq!(froms.len(), 2, "{got}");
    assert!(froms.contains(&"alpha@acct") && froms.contains(&"gamma@acct"));
    // A resend by the same sender with the same id is still a duplicate.
    let (_, again, _) = send(alpha, b"real").await;
    assert_eq!(again["outcome"], "duplicate");
    d.stop().await;
}

/// Review I2: the daemon serves its own account's claimed roles only; anything else would let one
/// role fill the disk with queues nobody can ever read.
#[tokio::test]
async fn mail_for_a_foreign_account_or_an_unclaimed_role_is_refused() {
    let d = start().await;
    let (alpha, _) = claim(&d, "alpha", 1).await;
    for to in ["x@other", "never-claimed@acct"] {
        let (s, reply, _) = post(
            &d,
            "/v1/send",
            Some(&alpha),
            json!({"to": to, "body_b64": B64.encode(b"x")}),
        )
        .await;
        assert_eq!(s, 404, "{to}: {reply}");
        assert_eq!(reply["error"], "unknown_recipient");
    }
    d.stop().await;
}

/// Review M1: an absurd lease is a 400, not a panic in the handler.
#[tokio::test]
async fn an_oversized_lease_is_400_not_a_panic() {
    let d = start().await;
    let (alpha, _) = claim(&d, "alpha", 1).await;
    let (s, reply, _) = post(
        &d,
        "/v1/fetch",
        Some(&alpha),
        json!({"lease_secs": i64::MAX}),
    )
    .await;
    assert_eq!(s, 400, "{reply}");
    d.stop().await;
}

/// Review M2: serve() itself refuses a non-loopback listener, whatever the config said.
#[tokio::test]
async fn serve_refuses_a_non_loopback_listener() {
    let home = tempfile::tempdir().unwrap();
    Keystore::init_account(home.path(), "acct").unwrap();
    let daemon = Daemon::open(&config(home.path())).unwrap();
    let listener = tokio::net::TcpListener::bind("0.0.0.0:0").await.unwrap();
    let result = daemon.serve(listener, async {}).await;
    assert!(result.is_err(), "serving on 0.0.0.0 must fail");
}

/// Review M3: more than one Host header is refused, whichever comes first.
#[tokio::test]
async fn duplicate_host_headers_are_421() {
    let d = start().await;
    let addr = d.addr;
    let port = addr.port();
    let req = format!(
        "GET /v1/health HTTP/1.1\r\nHost: 127.0.0.1:{port}\r\nHost: evil.example:{port}\r\nConnection: close\r\n\r\n"
    );
    let reply = tokio::task::spawn_blocking(move || raw(addr, &req))
        .await
        .unwrap();
    assert!(reply.starts_with("HTTP/1.1 421"), "{reply}");
    d.stop().await;
}

/// Queue one message to `to` with the session `token`'s role as sender; returns its full id.
async fn send_to(d: &Running, token: &str, to: &str, body: &[u8]) -> String {
    let (s, sent, text) = post(
        d,
        "/v1/send",
        Some(token),
        json!({"to": to, "body_b64": B64.encode(body)}),
    )
    .await;
    assert_eq!(s, 200, "{text}");
    assert_eq!(sent["outcome"], "queued");
    sent["message_id"].as_str().unwrap().to_string()
}

/// #77 D1: acking a message the caller's epoch does not hold the lease on is 403 `not_your_lease`.
/// Negative: a queued-but-never-fetched message, and a message leased by a superseded epoch of the
/// same role. Positive: the epoch that holds the lease acks it.
#[tokio::test]
async fn acking_a_message_you_do_not_hold_the_lease_on_is_403_not_your_lease() {
    let d = start().await;
    let (alpha, _) = claim(&d, "alpha", 1).await;
    let (beta, _) = claim(&d, "beta", 2).await;

    // Never fetched, so no lease at all.
    let unleased = send_to(&d, &alpha, "beta@acct", b"one").await;
    let (s, reply, _) = post(&d, "/v1/ack", Some(&beta), json!({"message_id": unleased})).await;
    assert_eq!(s, 403, "{reply}");
    assert_eq!(reply["error"], "not_your_lease");

    // Positive control: once beta fetches it (taking the lease), the same ack succeeds.
    let (s, got, _) = post(&d, "/v1/fetch", Some(&beta), json!({})).await;
    assert_eq!(s, 200, "{got}");
    assert_eq!(got["messages"][0]["message_id"], unleased.as_str());
    let (s, acked, _) = post(&d, "/v1/ack", Some(&beta), json!({"message_id": unleased})).await;
    assert_eq!(s, 200, "{acked}");
    assert_eq!(acked["outcome"], "removed");

    // Leased by beta's first epoch; after a takeover the new epoch does not hold that lease.
    let leased = send_to(&d, &alpha, "beta@acct", b"two").await;
    let (_, got, _) = post(&d, "/v1/fetch", Some(&beta), json!({})).await;
    assert_eq!(got["messages"][0]["message_id"], leased.as_str());
    let (beta2, _) = claim(&d, "beta", 3).await;
    let (s, reply, _) = post(&d, "/v1/ack", Some(&beta2), json!({"message_id": leased})).await;
    assert_eq!(s, 403, "{reply}");
    assert_eq!(reply["error"], "not_your_lease");
    d.stop().await;
}

/// #77 D3: a client `message_id` containing `/` or a control character is 400 `malformed` (ids are
/// scoped as `<sender>/<id>`, so a `/` would forge another namespace). `ab` is queued.
#[tokio::test]
async fn a_message_id_with_a_slash_or_control_char_is_400_malformed() {
    let d = start().await;
    let (alpha, _) = claim(&d, "alpha", 1).await;
    let (_beta, _) = claim(&d, "beta", 2).await;
    for bad in ["a/b", "a\u{7}"] {
        let (s, reply, _) = post(
            &d,
            "/v1/send",
            Some(&alpha),
            json!({"to": "beta@acct", "message_id": bad, "body_b64": B64.encode(b"x")}),
        )
        .await;
        assert_eq!(s, 400, "{bad:?}: {reply}");
        assert_eq!(reply["error"], "malformed", "{bad:?}");
    }
    let (s, reply, _) = post(
        &d,
        "/v1/send",
        Some(&alpha),
        json!({"to": "beta@acct", "message_id": "ab", "body_b64": B64.encode(b"x")}),
    )
    .await;
    assert_eq!(s, 200, "{reply}");
    assert_eq!(reply["outcome"], "queued");
    d.stop().await;
}

/// The presence summary `list` reports for `role`.
async fn summary_of(d: &Running, token: &str, role: &str) -> Value {
    let (s, listed, _) = post_get_list(d, token).await;
    assert_eq!(s, 200, "{listed}");
    listed["roles"]
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["global_id"] == role)
        .expect("role listed")["summary"]
        .clone()
}

async fn post_get_list(d: &Running, token: &str) -> (u16, Value, String) {
    let resp = client()
        .get(d.url("/v1/list"))
        .bearer_auth(token)
        .send()
        .await
        .unwrap();
    let status = resp.status().as_u16();
    let text = resp.text().await.unwrap();
    (
        status,
        serde_json::from_str(&text).unwrap_or(Value::Null),
        text,
    )
}

/// #77 D4: a heartbeat summary over 500 bytes, or containing a control character, is 400
/// `malformed` and leaves the stored summary unchanged. Exactly 500 characters is accepted.
#[tokio::test]
async fn a_heartbeat_summary_too_long_or_with_a_control_char_is_400_malformed() {
    let d = start().await;
    let (alpha, _) = claim(&d, "alpha", 1).await;
    for bad in ["x".repeat(501), "line one\nline two".to_string()] {
        let (s, reply, _) = post(&d, "/v1/heartbeat", Some(&alpha), json!({"summary": bad})).await;
        assert_eq!(s, 400, "{reply}");
        assert_eq!(reply["error"], "malformed");
    }
    assert_eq!(summary_of(&d, &alpha, "alpha@acct").await, Value::Null);

    let ok = "x".repeat(500);
    let (s, reply, _) = post(&d, "/v1/heartbeat", Some(&alpha), json!({"summary": ok})).await;
    assert_eq!(s, 200, "{reply}");
    assert_eq!(summary_of(&d, &alpha, "alpha@acct").await, json!(ok));
    d.stop().await;
}

/// #77 D5: once a recipient's mailbox holds `max_bytes` (64 MiB) of bodies, the next send is 413
/// `mailbox_full` (the daemon's own limit, not axum's 2 MB request cap: each request body here is
/// about 2 MB of base64 and every one before the full mailbox is accepted). Every send before
/// that is queued, and a small message that still fits is queued afterwards.
#[tokio::test]
async fn a_full_mailbox_is_413_mailbox_full() {
    const BODY: usize = 1_500_000; // base64 inflates to 2_000_000 < axum's 2 MiB request limit
    const CAP: usize = 64 * 1024 * 1024;
    let d = start().await;
    let (alpha, _) = claim(&d, "alpha", 1).await;
    let (_beta, _) = claim(&d, "beta", 2).await;
    let body = vec![7u8; BODY];

    let fits = CAP / BODY;
    for _ in 0..fits {
        send_to(&d, &alpha, "beta@acct", &body).await;
    }
    let (s, reply, text) = post(
        &d,
        "/v1/send",
        Some(&alpha),
        json!({"to": "beta@acct", "body_b64": B64.encode(&body)}),
    )
    .await;
    assert_eq!(s, 413, "{text}");
    assert_eq!(reply["error"], "mailbox_full");

    // The cap is on bytes: a small message that still fits is accepted.
    send_to(&d, &alpha, "beta@acct", b"small").await;
    d.stop().await;
}
