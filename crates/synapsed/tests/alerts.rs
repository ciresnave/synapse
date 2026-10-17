// SPDX-License-Identifier: MIT OR Apache-2.0
//! Alert delivery by Synapse mail (ROADMAP row 9): with alert roles set, `synapsed` queues each
//! alert the policy raises into those roles' mailboxes; with none set it changes nothing.

use std::net::SocketAddr;
use std::sync::Arc;
use std::time::Duration;

use base64::Engine;
use base64::engine::general_purpose::STANDARD as B64;
use chrono::Utc;
use serde_json::{Value, json};
use synapse::certificate::chain_to_pem;
use synapse::keystore::Keystore;
use synapse::roles::sign_claim_for;
use synapsed::{Daemon, DaemonConfig, Pause, SECURITY_EVENTS_FILE, SecurityConfig};

struct Running {
    addr: SocketAddr,
    home: tempfile::TempDir,
    started_at: chrono::DateTime<Utc>,
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

/// A daemon whose failure delays are skipped and whose event file writes every event, so a flood
/// of bad bearers reaches the alert policy's threshold at once.
async fn start(alert_roles: Option<Vec<String>>) -> Running {
    let home = tempfile::tempdir().unwrap();
    Keystore::init_account(home.path(), "acct").expect("init account");
    let cfg = DaemonConfig {
        home: home.path().to_path_buf(),
        addr: "127.0.0.1:0".parse().unwrap(),
        sweep_every: Duration::from_secs(3600),
    };
    let security = SecurityConfig {
        event_interval: Duration::ZERO,
        ..SecurityConfig::default()
    };
    let no_wait: Pause = Arc::new(|_| Box::pin(async {}));
    let mut daemon = Daemon::open(&cfg)
        .expect("open daemon")
        .with_security(security)
        .with_pause(no_wait);
    if let Some(roles) = alert_roles {
        daemon = daemon.with_alert_roles(roles);
    }
    let started_at = daemon.started_at();
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let addr = listener.local_addr().unwrap();
    let (tx, rx) = tokio::sync::oneshot::channel::<()>();
    let task = tokio::spawn(daemon.serve(listener, async move {
        let _ = rx.await;
    }));
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

async fn claim(d: &Running, role: &str) -> String {
    let announce = std::fs::read_to_string(d.home.path().join("synapsed.json")).unwrap();
    let instance: Value = serde_json::from_str(&announce).unwrap();
    let store = Keystore::open(d.home.path()).unwrap();
    let identity = store.role(role, Utc::now()).unwrap();
    let signed_at = Utc::now().max(d.started_at + chrono::Duration::seconds(1));
    let nonce = *uuid::Uuid::new_v4().as_bytes();
    let req = sign_claim_for(
        &identity,
        instance["instance_id"].as_str().unwrap(),
        nonce,
        signed_at,
    )
    .unwrap();
    let body = json!({
        "audience": req.audience,
        "chain_pem": chain_to_pem(&req.chain),
        "nonce_hex": req.nonce.iter().map(|b| format!("{b:02x}")).collect::<String>(),
        "signed_at": req.signed_at.to_rfc3339(),
        "signature_b64": B64.encode(&req.signature),
    });
    let reply = reqwest::Client::new()
        .post(d.url("/v1/claim"))
        .json(&body)
        .send()
        .await
        .unwrap();
    assert_eq!(reply.status().as_u16(), 200, "claim refused");
    let reply: Value = reply.json().await.unwrap();
    reply["session"].as_str().unwrap().to_string()
}

/// Ten bad bearers: the `AuthFailure` threshold (10 in 5 minutes) of the default alert policy.
async fn flood_bad_bearers(d: &Running) {
    for _ in 0..10 {
        let reply = reqwest::Client::new()
            .get(d.url("/v1/list"))
            .bearer_auth("not-a-session")
            .send()
            .await
            .unwrap();
        assert_eq!(reply.status().as_u16(), 401);
    }
}

async fn fetch(d: &Running, token: &str) -> Vec<Value> {
    let reply = reqwest::Client::new()
        .post(d.url("/v1/fetch"))
        .bearer_auth(token)
        .json(&json!({}))
        .send()
        .await
        .unwrap();
    assert_eq!(reply.status().as_u16(), 200);
    let reply: Value = reply.json().await.unwrap();
    reply["messages"].as_array().unwrap().clone()
}

fn events_file(d: &Running) -> String {
    std::fs::read_to_string(d.home.path().join(SECURITY_EVENTS_FILE)).unwrap_or_default()
}

/// A planted alert (a flood of bad bearers) is queued for the configured role, from a sender no
/// role can claim, with the event's fields in the body.
#[tokio::test]
async fn a_planted_alert_reaches_the_mailbox_of_the_alert_role() {
    let d = start(Some(vec!["ops".to_string()])).await;
    let ops = claim(&d, "ops").await;
    flood_bad_bearers(&d).await;

    let mut got = Vec::new();
    for _ in 0..100 {
        got = fetch(&d, &ops).await;
        if !got.is_empty() {
            break;
        }
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    assert!(
        !got.is_empty(),
        "no alert reached ops; events: {}",
        events_file(&d)
    );
    let msg = &got[0];
    assert_eq!(
        msg["from"], "daemon:alerts@acct",
        "a sender no role can claim"
    );
    let body = String::from_utf8(B64.decode(msg["body_b64"].as_str().unwrap()).unwrap()).unwrap();
    assert!(body.contains("SECURITY ALERT"), "{body}");
    assert!(body.contains("AuthFailure"), "{body}");
    assert!(body.contains("synapsed/bearer"), "{body}");
    assert!(
        !body.chars().any(|c| c.is_control() && c != '\n'),
        "{body:?}"
    );
    d.stop().await;
}

/// With no alert roles (the default), the same flood queues nothing. The control: the events were
/// produced (the file holds them), so the silence is the setting, not a flood that never happened.
#[tokio::test]
async fn no_alert_is_queued_when_no_alert_role_is_set() {
    for roles in [None, Some(Vec::new())] {
        let d = start(roles).await;
        let ops = claim(&d, "ops").await;
        flood_bad_bearers(&d).await;
        tokio::time::sleep(Duration::from_millis(500)).await;
        assert!(
            fetch(&d, &ops).await.is_empty(),
            "an alert was queued with no alert role"
        );
        let failures = events_file(&d)
            .lines()
            .filter(|l| l.contains("AuthFailure") || l.contains("auth_failure"))
            .count();
        assert!(
            failures >= 10,
            "the flood was not recorded ({failures} lines)"
        );
        d.stop().await;
    }
}
