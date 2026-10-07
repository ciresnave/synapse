// SPDX-License-Identifier: MIT OR Apache-2.0
//! Hardening P7: the email servers limit, lock out and record failed logins (audit rows 14 and 18), and
//! the SMTP server limits inbound connections and messages per source (row 13). Plan:
//! docs/superpowers/plans/2026-10-07-hardening-p7-email.md.
//!
//! Every test pairs a negative control (the attempt is refused or locked out, and an event is written)
//! with a positive control (a legitimate caller is unaffected). Alert delivery awaits board 131.

use std::collections::HashMap;
use std::sync::{Arc, Mutex};

use chrono::Duration;
use synapse::email_server::security::{InboundLimits, LoginLimits};
use synapse::email_server::{
    AuthHandler, ImapServerConfig, SmtpServerConfig, SynapseAuthHandler, SynapseImapServer,
    SynapseSmtpServer, UserPermissions,
};
use synapse::security_events::{LimiterConfig, SecurityEvent, SecurityEventKind, SecuritySink};
use synapse::transport::{EmailTransportFactory, TransportManagerBuilder, TransportType};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::TcpStream;
use tokio::net::tcp::{OwnedReadHalf, OwnedWriteHalf};

const ALICE_PASSWORD: &str = "ALICE-PASSWORD-MUST-NOT-APPEAR";
const BOB_PASSWORD: &str = "BOB-PASSWORD-MUST-NOT-APPEAR";
const WRONG_PASSWORD: &str = "WRONG-PASSWORD-MUST-NOT-APPEAR";

#[derive(Default)]
struct Capture(Mutex<Vec<SecurityEvent>>);

impl SecuritySink for Capture {
    fn record(&self, event: &SecurityEvent) {
        self.0.lock().unwrap().push(event.clone());
    }
}

impl Capture {
    fn all(&self) -> Vec<SecurityEvent> {
        self.0.lock().unwrap().clone()
    }
    fn on(&self, surface: &str) -> Vec<SecurityEvent> {
        self.all()
            .into_iter()
            .filter(|e| e.surface == surface)
            .collect()
    }
    fn of(&self, surface: &str, kind: SecurityEventKind) -> Vec<SecurityEvent> {
        self.on(surface)
            .into_iter()
            .filter(|e| e.kind == kind)
            .collect()
    }
}

/// Every event field, joined, for the no-secret scans.
fn fields(e: &SecurityEvent) -> String {
    format!(
        "{}|{}|{}|{}",
        e.surface,
        e.subject,
        e.source.clone().unwrap_or_default(),
        e.detail
    )
}

fn assert_no_secret(capture: &Capture, known_field: &str) {
    let all = capture.all();
    assert!(!all.is_empty(), "the scan needs events to scan");
    // Positive control: the scan does find a field that is meant to be there.
    assert!(
        all.iter().any(|e| fields(e).contains(known_field)),
        "the scan found no {known_field:?}, so it is not reading the events"
    );
    for e in &all {
        let f = fields(e);
        for secret in [ALICE_PASSWORD, BOB_PASSWORD, WRONG_PASSWORD] {
            assert!(!f.contains(secret), "a password leaked into {e:?}");
        }
        for user in ["alice", "bob"] {
            for password in [ALICE_PASSWORD, BOB_PASSWORD, WRONG_PASSWORD] {
                let blob = plain_blob(user, password);
                assert!(!f.contains(&blob), "an AUTH PLAIN blob leaked into {e:?}");
            }
        }
    }
}

fn auth_handler() -> Arc<dyn AuthHandler + Send + Sync> {
    let handler = SynapseAuthHandler::new();
    let perms = || UserPermissions {
        can_send: true,
        can_receive: true,
        can_relay: false,
        is_admin: false,
    };
    handler
        .add_user_with_password("alice", ALICE_PASSWORD, "alice@synapse.local", perms())
        .unwrap();
    handler
        .add_user_with_password("bob", BOB_PASSWORD, "bob@synapse.local", perms())
        .unwrap();
    Arc::new(handler)
}

fn limiter(lockout_after: u32) -> LimiterConfig {
    LimiterConfig {
        free_failures: 1,
        window: Duration::minutes(15),
        base_delay: Duration::milliseconds(1),
        max_delay: Duration::milliseconds(5),
        lockout_after,
        lockout: Duration::minutes(15),
        max_keys: 1_000,
    }
}

/// Tuned for tests: lock a username after `per_user` failures, a source after `per_source`.
fn login_limits(per_user: u32, per_source: u32) -> LoginLimits {
    LoginLimits {
        per_user: limiter(per_user),
        per_source: limiter(per_source),
        event_interval: Duration::zero(),
    }
}

fn plain_blob(user: &str, password: &str) -> String {
    use base64::Engine;
    base64::engine::general_purpose::STANDARD.encode(format!("\0{user}\0{password}"))
}

struct Client {
    reader: BufReader<OwnedReadHalf>,
    writer: OwnedWriteHalf,
}

impl Client {
    async fn connect(port: u16) -> (Client, String) {
        let stream = TcpStream::connect(("127.0.0.1", port))
            .await
            .expect("connect");
        let (read, writer) = stream.into_split();
        let mut client = Client {
            reader: BufReader::new(read),
            writer,
        };
        let greeting = client.line().await;
        (client, greeting)
    }

    async fn line(&mut self) -> String {
        let mut line = String::new();
        tokio::time::timeout(
            std::time::Duration::from_secs(10),
            self.reader.read_line(&mut line),
        )
        .await
        .expect("a reply within 10 s")
        .expect("read");
        line
    }

    /// Sends one command and returns the final reply line (SMTP multi-line replies end at `NNN `).
    async fn smtp(&mut self, command: &str) -> String {
        self.writer
            .write_all(format!("{command}\r\n").as_bytes())
            .await
            .unwrap();
        loop {
            let line = self.line().await;
            if line.len() < 4 || line.as_bytes()[3] != b'-' {
                return line;
            }
        }
    }

    /// Sends one tagged IMAP command and returns its tagged reply.
    async fn imap(&mut self, tag: &str, command: &str) -> String {
        self.writer
            .write_all(format!("{tag} {command}\r\n").as_bytes())
            .await
            .unwrap();
        loop {
            let line = self.line().await;
            if line.starts_with(tag) || line.is_empty() {
                return line;
            }
        }
    }
}

async fn bound() -> (tokio::net::TcpListener, u16) {
    let listener = tokio::net::TcpListener::bind(("127.0.0.1", 0))
        .await
        .expect("bind");
    let port = listener.local_addr().unwrap().port();
    (listener, port)
}

async fn imap_server(limits: LoginLimits, sink: Option<Arc<Capture>>) -> u16 {
    let server = SynapseImapServer::new(
        ImapServerConfig::default(),
        Arc::new(Mutex::new(HashMap::new())),
        auth_handler(),
    )
    .with_login_limits(limits);
    if let Some(sink) = sink {
        server.set_security_sink(sink);
    }
    let (listener, port) = bound().await;
    tokio::spawn(async move {
        let _ = server.serve(listener).await;
    });
    port
}

async fn smtp_server(
    require_auth: bool,
    login: LoginLimits,
    inbound: InboundLimits,
    sink: Option<Arc<Capture>>,
) -> u16 {
    let server = SynapseSmtpServer::new(
        SmtpServerConfig {
            require_auth,
            ..Default::default()
        },
        auth_handler(),
        Arc::new(Mutex::new(HashMap::new())),
    )
    .with_login_limits(login)
    .with_inbound_limits(inbound);
    if let Some(sink) = sink {
        server.set_security_sink(sink);
    }
    let (listener, port) = bound().await;
    tokio::spawn(async move {
        let _ = server.serve(listener).await;
    });
    port
}

fn inbound(connections: u32, messages: u32) -> InboundLimits {
    InboundLimits {
        connections_per_minute: connections,
        messages_per_minute: messages,
        event_interval: Duration::zero(),
    }
}

async fn imap_login(port: u16, user: &str, password: &str) -> String {
    let (mut c, _) = Client::connect(port).await;
    c.imap("a1", &format!("LOGIN {user} {password}")).await
}

async fn smtp_auth(port: u16, user: &str, password: &str) -> String {
    let (mut c, _) = Client::connect(port).await;
    c.smtp("EHLO test").await;
    c.smtp(&format!("AUTH PLAIN {}", plain_blob(user, password)))
        .await
}

#[tokio::test]
async fn imap_bad_login_is_an_event() {
    let capture = Arc::new(Capture::default());
    let port = imap_server(login_limits(10, 100), Some(capture.clone())).await;

    // Positive control: the right password logs in and writes nothing.
    let ok = imap_login(port, "alice", ALICE_PASSWORD).await;
    assert!(ok.starts_with("a1 OK"), "got {ok:?}");
    assert!(
        capture.all().is_empty(),
        "a good login wrote {:?}",
        capture.all()
    );

    // Negative control: a wrong password is refused and recorded.
    let no = imap_login(port, "alice", WRONG_PASSWORD).await;
    assert!(no.starts_with("a1 NO"), "got {no:?}");
    let failures = capture.of("email/imap-login", SecurityEventKind::AuthFailure);
    assert_eq!(failures.len(), 1, "{:?}", capture.all());
    assert_eq!(failures[0].subject, "alice");
    assert_eq!(failures[0].source.as_deref(), Some("127.0.0.1"));
    assert!(
        failures[0].detail.contains("bad_credentials"),
        "{:?}",
        failures[0]
    );
}

#[tokio::test]
async fn imap_lockout_refuses_the_right_password() {
    let capture = Arc::new(Capture::default());
    let port = imap_server(login_limits(3, 100), Some(capture.clone())).await;

    for _ in 0..3 {
        let no = imap_login(port, "alice", WRONG_PASSWORD).await;
        assert!(no.starts_with("a1 NO"), "got {no:?}");
    }
    let lockouts = capture.of("email/imap-login", SecurityEventKind::Lockout);
    assert_eq!(lockouts.len(), 1, "{:?}", capture.all());
    assert_eq!(lockouts[0].subject, "alice");
    assert!(lockouts[0].detail.contains("per_user"), "{:?}", lockouts[0]);

    // Negative control: locked, so even the right password is refused, exactly like a wrong one.
    let locked = imap_login(port, "alice", ALICE_PASSWORD).await;
    assert_eq!(locked.trim_end(), "a1 NO LOGIN failed");
    let last = capture.all().pop().unwrap();
    assert_eq!(last.kind, SecurityEventKind::AuthFailure);
    assert!(last.detail.contains("locked"), "{last:?}");

    // Positive control: another user, from the same source, still logs in.
    let ok = imap_login(port, "bob", BOB_PASSWORD).await;
    assert!(ok.starts_with("a1 OK"), "got {ok:?}");
}

#[tokio::test]
async fn smtp_auth_lockout() {
    let capture = Arc::new(Capture::default());
    let port = smtp_server(
        true,
        login_limits(3, 100),
        inbound(1_000, 1_000),
        Some(capture.clone()),
    )
    .await;

    for _ in 0..3 {
        let no = smtp_auth(port, "alice", WRONG_PASSWORD).await;
        assert!(no.starts_with("535"), "got {no:?}");
    }
    assert_eq!(
        capture
            .of("email/smtp-auth", SecurityEventKind::AuthFailure)
            .len(),
        3,
        "{:?}",
        capture.all()
    );
    assert_eq!(
        capture
            .of("email/smtp-auth", SecurityEventKind::Lockout)
            .len(),
        1,
        "{:?}",
        capture.all()
    );

    // Negative control: locked, so the right password reads exactly like a wrong one.
    let locked = smtp_auth(port, "alice", ALICE_PASSWORD).await;
    assert!(locked.starts_with("535"), "got {locked:?}");

    // Positive control: another user still authenticates.
    let ok = smtp_auth(port, "bob", BOB_PASSWORD).await;
    assert!(ok.starts_with("235"), "got {ok:?}");
}

#[tokio::test]
async fn per_source_backstop() {
    let capture = Arc::new(Capture::default());
    let port = imap_server(login_limits(100, 4), Some(capture.clone())).await;

    for i in 0..4 {
        let no = imap_login(port, &format!("sprayed-{i}"), WRONG_PASSWORD).await;
        assert!(no.starts_with("a1 NO"), "got {no:?}");
    }
    let lockouts = capture.of("email/imap-login", SecurityEventKind::Lockout);
    assert_eq!(lockouts.len(), 1, "{:?}", capture.all());
    assert!(
        lockouts[0].detail.contains("per_source"),
        "{:?}",
        lockouts[0]
    );

    // The backstop refuses that source, a real user included: that is what it is for.
    let refused = imap_login(port, "bob", BOB_PASSWORD).await;
    assert!(refused.starts_with("a1 NO"), "got {refused:?}");
}

#[tokio::test]
async fn inbound_connection_limit() {
    let capture = Arc::new(Capture::default());
    let port = smtp_server(
        false,
        login_limits(10, 100),
        inbound(2, 1_000),
        Some(capture.clone()),
    )
    .await;

    // Positive control: connections within the limit are greeted.
    let (_c1, g1) = Client::connect(port).await;
    let (_c2, g2) = Client::connect(port).await;
    assert!(
        g1.starts_with("220") && g2.starts_with("220"),
        "{g1:?} {g2:?}"
    );
    assert!(capture.all().is_empty(), "{:?}", capture.all());

    // Negative control: the next one is refused and recorded.
    let (_c3, g3) = Client::connect(port).await;
    assert!(g3.starts_with("421"), "got {g3:?}");
    let limited = capture.of("email/smtp-inbound", SecurityEventKind::RateLimited);
    assert_eq!(limited.len(), 1, "{:?}", capture.all());
    assert_eq!(limited[0].subject, "127.0.0.1");
}

#[tokio::test]
async fn inbound_message_limit() {
    let capture = Arc::new(Capture::default());
    let port = smtp_server(
        false,
        login_limits(10, 100),
        inbound(1_000, 1),
        Some(capture.clone()),
    )
    .await;

    let (mut c, _) = Client::connect(port).await;
    c.smtp("EHLO test").await;
    // Positive control: the first message is accepted.
    let first = c.smtp("MAIL FROM:<alice@synapse.local>").await;
    assert!(first.starts_with("250"), "got {first:?}");
    c.smtp("RSET").await;

    // Negative control: the second, in the same minute, is refused and recorded.
    let second = c.smtp("MAIL FROM:<alice@synapse.local>").await;
    assert!(second.starts_with("451"), "got {second:?}");
    let limited = capture.of("email/smtp-inbound", SecurityEventKind::RateLimited);
    assert_eq!(limited.len(), 1, "{:?}", capture.all());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn direct_mode_inherits_the_manager_sink() {
    let capture = Arc::new(Capture::default());
    let port = std::net::TcpListener::bind("127.0.0.1:0")
        .unwrap()
        .local_addr()
        .unwrap()
        .port();
    let config = HashMap::from([
        ("email_mode".to_string(), "direct".to_string()),
        ("local_port".to_string(), port.to_string()),
        (
            "email_inbound_connections_per_minute".to_string(),
            "1".to_string(),
        ),
    ]);
    let mut builder = TransportManagerBuilder::new();
    for other in [
        TransportType::Tcp,
        TransportType::Udp,
        TransportType::Http,
        TransportType::WebSocket,
        TransportType::AutoDiscovery,
        TransportType::Quic,
        TransportType::NatTraversal,
    ] {
        builder = builder.disable_transport(other);
    }
    let manager = builder
        .enable_transport(TransportType::Email)
        .transport_config(TransportType::Email, config)
        .security_sink(capture.clone())
        .build();
    manager
        .register_factory(Box::new(EmailTransportFactory))
        .await
        .unwrap();
    manager.start().await.unwrap();

    // Positive control: the first connection is greeted.
    let (_c1, g1) = Client::connect(port).await;
    assert!(g1.starts_with("220"), "got {g1:?}");
    // Negative control: the second is refused, and the manager's sink holds the event.
    let (_c2, g2) = Client::connect(port).await;
    assert!(g2.starts_with("421"), "got {g2:?}");
    assert_eq!(
        capture
            .of("email/smtp-inbound", SecurityEventKind::RateLimited)
            .len(),
        1,
        "{:?}",
        capture.all()
    );
    manager.stop().await.unwrap();
}

#[tokio::test]
async fn no_secret_in_any_event() {
    let capture = Arc::new(Capture::default());
    let imap = imap_server(login_limits(2, 100), Some(capture.clone())).await;
    let smtp = smtp_server(
        true,
        login_limits(2, 100),
        inbound(1_000, 1_000),
        Some(capture.clone()),
    )
    .await;
    for _ in 0..3 {
        imap_login(imap, "alice", WRONG_PASSWORD).await;
        smtp_auth(smtp, "alice", WRONG_PASSWORD).await;
    }
    imap_login(imap, "alice", ALICE_PASSWORD).await;
    smtp_auth(smtp, "alice", ALICE_PASSWORD).await;
    assert_no_secret(&capture, "alice");
}

#[tokio::test]
async fn no_sink_still_limits() {
    let port = imap_server(login_limits(3, 100), None).await;
    for _ in 0..3 {
        imap_login(port, "alice", WRONG_PASSWORD).await;
    }
    // Negative control: locked with no sink attached: the countermeasure does not depend on one.
    let locked = imap_login(port, "alice", ALICE_PASSWORD).await;
    assert!(locked.starts_with("a1 NO"), "got {locked:?}");
    // Positive control.
    let ok = imap_login(port, "bob", BOB_PASSWORD).await;
    assert!(ok.starts_with("a1 OK"), "got {ok:?}");
}

#[tokio::test]
async fn usernames_are_bounded_before_keying() {
    let capture = Arc::new(Capture::default());
    let port = imap_server(login_limits(2, 100), Some(capture.clone())).await;
    let long = "u".repeat(10_000);
    for _ in 0..2 {
        imap_login(port, &long, WRONG_PASSWORD).await;
    }
    // Both attempts land on one key, so the second locks it, and no field exceeds the bound.
    assert_eq!(
        capture
            .of("email/imap-login", SecurityEventKind::Lockout)
            .len(),
        1,
        "{:?}",
        capture.all()
    );
    assert!(
        capture
            .all()
            .iter()
            .all(|e| e.subject.chars().count() <= 256)
    );
}
