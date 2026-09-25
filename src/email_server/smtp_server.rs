// SPDX-License-Identifier: MIT OR Apache-2.0
/// High-performance SMTP server for EMRP
use crate::error::{Result, SynapseError};
use crate::types::SecureMessage;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use std::time::{SystemTime, UNIX_EPOCH};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::net::{TcpListener, TcpStream};
use tracing::{debug, error, info};

/// High-performance SMTP server optimized for EMRP
pub struct SynapseSmtpServer {
    /// Server configuration
    config: SmtpServerConfig,
    /// Message storage
    message_store: Arc<Mutex<HashMap<String, Vec<SecureMessage>>>>,
    /// Connected clients
    clients: Arc<Mutex<HashMap<String, ClientSession>>>,
    /// Authorization handler
    auth_handler: Arc<dyn AuthHandler + Send + Sync>,
    /// Performance metrics
    metrics: Arc<Mutex<ServerMetrics>>,
}

#[derive(Debug, Clone)]
pub struct SmtpServerConfig {
    /// SMTP port (usually 25, 587, or 2525)
    pub port: u16,
    /// Maximum message size in bytes
    pub max_message_size: usize,
    /// Authentication required
    pub require_auth: bool,
    /// TLS configuration
    pub tls_config: Option<TlsConfig>,
    /// Performance optimization settings
    pub performance: PerformanceConfig,
    /// Which interfaces the server listens on (loopback by default)
    pub bind_scope: crate::network_scope::BindScope,
}

#[derive(Debug, Clone)]
pub struct PerformanceConfig {
    /// Connection pool size
    pub max_connections: usize,
    /// Keep-alive timeout
    pub keep_alive_timeout: u64,
    /// Pipeline multiple commands
    pub enable_pipelining: bool,
    /// Compression for large messages
    pub enable_compression: bool,
}

#[derive(Debug, Clone)]
pub struct TlsConfig {
    pub cert_path: String,
    pub key_path: String,
    pub require_tls: bool,
}

#[derive(Debug)]
struct ClientSession {
    #[allow(dead_code)]
    id: String,
    authenticated: bool,
    current_message: Option<SmtpMessage>,
    #[allow(dead_code)]
    connected_at: SystemTime,
}

#[derive(Debug)]
struct SmtpMessage {
    #[allow(dead_code)]
    from: String,
    to: Vec<String>,
    data: Vec<u8>,
}

#[derive(Debug, Clone)]
pub struct ServerMetrics {
    #[allow(dead_code)]
    messages_received: u64,
    #[allow(dead_code)]
    messages_delivered: u64,
    connections_accepted: u64,
    #[allow(dead_code)]
    average_processing_time_ms: f64,
    #[allow(dead_code)]
    last_reset: SystemTime,
}

impl Default for ServerMetrics {
    fn default() -> Self {
        Self {
            messages_received: 0,
            messages_delivered: 0,
            connections_accepted: 0,
            average_processing_time_ms: 0.0,
            last_reset: SystemTime::now(),
        }
    }
}

pub trait AuthHandler {
    fn authenticate(&self, username: &str, password: &str) -> Result<bool>;
    fn is_authorized_sender(&self, email: &str) -> Result<bool>;
    fn is_authorized_recipient(&self, email: &str) -> Result<bool>;
}

impl Default for SmtpServerConfig {
    fn default() -> Self {
        Self {
            port: 2525,                         // Non-privileged port for development
            max_message_size: 25 * 1024 * 1024, // 25MB
            require_auth: true,
            tls_config: None,
            performance: PerformanceConfig {
                max_connections: 100,
                keep_alive_timeout: 300,
                enable_pipelining: true,
                enable_compression: true,
            },
            bind_scope: crate::network_scope::BindScope::default(),
        }
    }
}

impl SynapseSmtpServer {
    /// Create a new SMTP server. `message_store` is shared with whatever else needs to read
    /// received mail (an `EmailTransportImpl`'s `receive_raw`, or a paired `SynapseImapServer` for
    /// the relay-out/external proxy case) -- previously this server held its own private store
    /// that nothing else could ever read (see this plan's "Deviations" section).
    pub fn new(
        config: SmtpServerConfig,
        auth_handler: Arc<dyn AuthHandler + Send + Sync>,
        message_store: Arc<Mutex<HashMap<String, Vec<SecureMessage>>>>,
    ) -> Self {
        Self {
            config,
            message_store,
            clients: Arc::new(Mutex::new(HashMap::new())),
            auth_handler,
            metrics: Arc::new(Mutex::new(ServerMetrics::default())),
        }
    }

    /// Start the SMTP server: bind `config.port` and serve it. Callers that need to know the port
    /// is actually held before this returns (e.g. a `Transport::start` contract) should bind their
    /// own listener and call [`Self::serve`] directly instead -- see `email_unified.rs`.
    pub async fn start(&self) -> Result<()> {
        let addr = self.config.bind_scope.listen_addr(self.config.port);
        let listener = TcpListener::bind(&addr).await.map_err(|e| {
            SynapseError::NetworkError(format!("Failed to bind SMTP server to {addr}: {e}"))
        })?;

        info!("EMRP SMTP Server listening on {}", addr);

        self.serve(listener).await
    }

    /// Accept connections on an already-bound `listener` forever. Split out of `start` so a caller
    /// that must guarantee the port is held before returning (matching every other transport's
    /// bind-in-constructor convention -- see `tcp_unified.rs`'s `start_server` comment) can bind
    /// the listener itself and hand it here.
    pub async fn serve(&self, listener: TcpListener) -> Result<()> {
        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    info!("New SMTP connection from {}", addr);

                    // Update metrics
                    {
                        let mut metrics = self.metrics.lock().unwrap();
                        metrics.connections_accepted += 1;
                    }

                    // Handle connection in background task
                    let server = self.clone();
                    tokio::spawn(async move {
                        if let Err(e) = server.handle_connection(stream).await {
                            error!("SMTP connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept SMTP connection: {}", e);
                }
            }
        }
    }

    /// Handle individual SMTP connection
    async fn handle_connection(&self, stream: TcpStream) -> Result<()> {
        let (read_half, write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut writer = write_half;

        let mut session = ClientSession {
            id: format!(
                "smtp_{}",
                SystemTime::now()
                    .duration_since(UNIX_EPOCH)
                    .unwrap()
                    .as_millis()
            ),
            authenticated: false,
            current_message: None,
            connected_at: SystemTime::now(),
        };

        // Send greeting
        writer.write_all(b"220 EMRP SMTP Server Ready\r\n").await?;
        writer.flush().await?;

        let mut line = String::new();
        while reader.read_line(&mut line).await? > 0 {
            let command = line.trim();
            debug!("SMTP command: {}", command);

            let response = self.process_smtp_command(command, &mut session).await?;

            writer.write_all(response.as_bytes()).await?;
            writer.flush().await?;

            // Check if connection should close
            if response.starts_with("221") {
                break;
            }

            // A "354" response to DATA switches the connection into message-body mode: read raw
            // lines (not SMTP commands) until a line containing only "." (RFC 5321 §4.1.1.4),
            // un-stuffing a leading ".." back to "." per §4.5.2. Previously nothing read this body
            // at all: `process_smtp_command` returned 354 and the outer loop went straight back to
            // treating the next line as a command, so `store_message` was never called (hence its
            // `#[allow(dead_code)]`).
            if response.starts_with("354") {
                let mut raw_message = Vec::new();
                let mut data_line = String::new();
                loop {
                    data_line.clear();
                    let bytes_read = reader.read_line(&mut data_line).await?;
                    if bytes_read == 0 {
                        // Connection closed mid-DATA; nothing more to do.
                        break;
                    }
                    let content = data_line.trim_end_matches(['\r', '\n']);
                    if content == "." {
                        break;
                    }
                    let unstuffed = content.strip_prefix('.').unwrap_or(content);
                    raw_message.extend_from_slice(unstuffed.as_bytes());
                    raw_message.extend_from_slice(b"\r\n");
                }

                // What DATA carries is the whole RFC 5322 message: MIME headers, a blank line,
                // then the body, which `lettre` may transfer-encode (quoted-printable or base64)
                // to keep lines within SMTP's length limit. `store_message`'s own JSON parse
                // (and its test in `mod tests`) expects to receive the bare `SecureMessage` JSON,
                // not this envelope, so the envelope is stripped and any transfer encoding
                // reversed here -- once, in the wire-format layer -- via `mail_parser`, which
                // decodes whichever encoding was actually used rather than assuming one.
                let body = mail_parser::MessageParser::default()
                    .parse(&raw_message)
                    .and_then(|parsed| {
                        parsed
                            .body_text(0)
                            .map(|text| text.into_owned().into_bytes())
                    })
                    .unwrap_or(raw_message);

                let data_response = match session.current_message.take() {
                    Some(mut msg) => {
                        msg.data = body;
                        match self.store_message(msg).await {
                            Ok(()) => "250 Ok: message accepted\r\n".to_string(),
                            Err(e) => {
                                error!("Failed to store SMTP message: {e}");
                                "554 Transaction failed: message body did not parse\r\n".to_string()
                            }
                        }
                    }
                    None => "503 Bad sequence of commands\r\n".to_string(),
                };

                writer.write_all(data_response.as_bytes()).await?;
                writer.flush().await?;
            }

            line.clear();
        }

        Ok(())
    }

    /// Process SMTP command
    async fn process_smtp_command(
        &self,
        command: &str,
        session: &mut ClientSession,
    ) -> Result<String> {
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.is_empty() {
            return Ok("500 Syntax error\r\n".to_string());
        }

        let cmd = parts[0].to_uppercase();

        match cmd.as_str() {
            "HELO" | "EHLO" => {
                let hostname = parts.get(1).unwrap_or(&"unknown");
                if cmd == "EHLO" {
                    let mut response = format!("250-Hello {hostname}\r\n");
                    response.push_str("250-PIPELINING\r\n");
                    response.push_str("250-8BITMIME\r\n");
                    if self.config.require_auth {
                        response.push_str("250-AUTH PLAIN LOGIN\r\n");
                    }
                    response.push_str("250 Ok\r\n");
                    Ok(response)
                } else {
                    Ok(format!("250 Hello {hostname}\r\n"))
                }
            }
            "AUTH" => {
                if parts.len() < 2 {
                    return Ok("500 AUTH mechanism required\r\n".to_string());
                }

                let mechanism = parts[1].to_uppercase();
                match mechanism.as_str() {
                    "PLAIN" => {
                        if parts.len() >= 3 {
                            // Inline AUTH PLAIN
                            if let Ok(auth_data) = base64_decode(parts[2])
                                && let Ok(auth_str) = String::from_utf8(auth_data)
                            {
                                let auth_parts: Vec<&str> = auth_str.split('\0').collect();
                                if auth_parts.len() >= 3 {
                                    let username = auth_parts[1];
                                    let password = auth_parts[2];

                                    match self.auth_handler.authenticate(username, password) {
                                        Ok(true) => {
                                            session.authenticated = true;
                                            return Ok(
                                                "235 Authentication successful\r\n".to_string()
                                            );
                                        }
                                        Ok(false) => {
                                            return Ok("535 Authentication failed\r\n".to_string());
                                        }
                                        Err(_) => {
                                            return Ok("454 Temporary authentication failure\r\n"
                                                .to_string());
                                        }
                                    }
                                }
                            }
                        }
                        Ok("334 \r\n".to_string()) // Request auth data
                    }
                    _ => Ok("504 Unrecognized authentication type\r\n".to_string()),
                }
            }
            "MAIL" => {
                if self.config.require_auth && !session.authenticated {
                    return Ok("530 Authentication required\r\n".to_string());
                }

                if let Some(from_addr) = self.extract_email_from_mail_from(command) {
                    // Verify sender authorization
                    match self.auth_handler.is_authorized_sender(&from_addr) {
                        Ok(true) => {
                            session.current_message = Some(SmtpMessage {
                                from: from_addr,
                                to: Vec::new(),
                                data: Vec::new(),
                            });
                            Ok("250 Ok\r\n".to_string())
                        }
                        Ok(false) => Ok("550 Sender not authorized\r\n".to_string()),
                        Err(_) => Ok("451 Temporary failure\r\n".to_string()),
                    }
                } else {
                    Ok("501 Syntax error in MAIL command\r\n".to_string())
                }
            }
            "RCPT" => {
                if session.current_message.is_none() {
                    return Ok("503 Need MAIL command first\r\n".to_string());
                }

                if let Some(to_addr) = self.extract_email_from_rcpt_to(command) {
                    // Verify recipient authorization
                    match self.auth_handler.is_authorized_recipient(&to_addr) {
                        Ok(true) => {
                            if let Some(ref mut msg) = session.current_message {
                                msg.to.push(to_addr);
                            }
                            Ok("250 Ok\r\n".to_string())
                        }
                        Ok(false) => Ok("550 Recipient not authorized\r\n".to_string()),
                        Err(_) => Ok("451 Temporary failure\r\n".to_string()),
                    }
                } else {
                    Ok("501 Syntax error in RCPT command\r\n".to_string())
                }
            }
            "DATA" => {
                if session.current_message.is_none() {
                    return Ok("503 Need RCPT command first\r\n".to_string());
                }

                if let Some(ref msg) = session.current_message
                    && msg.to.is_empty()
                {
                    return Ok("503 Need RCPT command first\r\n".to_string());
                }

                Ok("354 Start mail input; end with <CRLF>.<CRLF>\r\n".to_string())
            }
            "QUIT" => Ok("221 Bye\r\n".to_string()),
            _ => Ok("502 Command not implemented\r\n".to_string()),
        }
    }

    /// Extract email address from MAIL FROM command
    fn extract_email_from_mail_from(&self, command: &str) -> Option<String> {
        // Parse "MAIL FROM:<email@domain.com>"
        if let Some(start) = command.find('<')
            && let Some(end) = command.find('>')
            && end > start
        {
            return Some(command[start + 1..end].to_string());
        }
        None
    }

    /// Extract email address from RCPT TO command
    fn extract_email_from_rcpt_to(&self, command: &str) -> Option<String> {
        // Parse "RCPT TO:<email@domain.com>"
        if let Some(start) = command.find('<')
            && let Some(end) = command.find('>')
            && end > start
        {
            return Some(command[start + 1..end].to_string());
        }
        None
    }

    /// Store a message received over SMTP. The `DATA` command's body is the whole `SecureMessage`
    /// as JSON (per the transport contract's wire format -- see this plan's Global Constraints),
    /// not raw content to wrap: hand-picking fields here was the exact bug NAT traversal's PR B
    /// fixed, and this server had the same one, just never reachable until now.
    async fn store_message(&self, message: SmtpMessage) -> Result<()> {
        let start_time = SystemTime::now();

        let secure_message: SecureMessage = serde_json::from_slice(&message.data).map_err(|e| {
            SynapseError::InvalidMessageFormat(format!(
                "SMTP DATA body did not parse as a SecureMessage: {e}"
            ))
        })?;

        // Store message for each recipient
        {
            let mut store = self.message_store.lock().unwrap();
            for recipient in &message.to {
                let messages = store.entry(recipient.clone()).or_default();
                messages.push(secure_message.clone());
            }
        }

        // Update metrics
        {
            let mut metrics = self.metrics.lock().unwrap();
            metrics.messages_received += 1;

            if let Ok(elapsed) = start_time.elapsed() {
                let processing_time = elapsed.as_millis() as f64;
                metrics.average_processing_time_ms =
                    (metrics.average_processing_time_ms + processing_time) / 2.0;
            }
        }

        info!(
            "Stored message from {} to {:?}",
            secure_message.from_global_id, message.to
        );
        Ok(())
    }

    /// Get messages for a recipient, without removing them.
    pub fn get_messages(&self, recipient: &str) -> Result<Vec<SecureMessage>> {
        let store = self.message_store.lock().unwrap();
        Ok(store.get(recipient).cloned().unwrap_or_default())
    }

    /// Take every message queued for `recipient`, removing them from the store. `receive_raw`
    /// callers that track a specific local recipient address call this, not `get_messages` (which
    /// is non-destructive and used elsewhere for inspection/tests).
    pub fn drain_messages(&self, recipient: &str) -> Result<Vec<SecureMessage>> {
        let mut store = self.message_store.lock().unwrap();
        Ok(store.remove(recipient).unwrap_or_default())
    }

    /// Take every message queued for every recipient, removing them from the store. Used by a
    /// single-tenant caller (an `EmailTransportImpl` running its own dedicated `SynapseSmtpServer`
    /// in Direct mode) that has no configured local recipient address to filter by -- everything
    /// that landed on this server's port belongs to it.
    pub fn drain_all_messages(&self) -> Result<Vec<SecureMessage>> {
        let mut store = self.message_store.lock().unwrap();
        let mut all = Vec::new();
        for (_, mut messages) in store.drain() {
            all.append(&mut messages);
        }
        Ok(all)
    }

    /// Get server metrics
    pub fn get_metrics(&self) -> ServerMetrics {
        self.metrics.lock().unwrap().clone()
    }
}

impl Clone for SynapseSmtpServer {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            message_store: Arc::clone(&self.message_store),
            clients: Arc::clone(&self.clients),
            auth_handler: Arc::clone(&self.auth_handler),
            metrics: Arc::clone(&self.metrics),
        }
    }
}

// Simple base64 decode function
fn base64_decode(input: &str) -> Result<Vec<u8>> {
    // Simple base64 decoding - in production, use a proper base64 library
    const CHARS: &[u8] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";

    let input = input.trim();
    let mut result = Vec::new();
    let mut buffer = 0u32;
    let mut bits = 0;

    for byte in input.bytes() {
        if byte == b'=' {
            break;
        }

        let value = CHARS
            .iter()
            .position(|&x| x == byte)
            .ok_or_else(|| SynapseError::Crypto("Invalid base64 character".to_string()))?;

        buffer = (buffer << 6) | (value as u32);
        bits += 6;

        if bits >= 8 {
            result.push((buffer >> (bits - 8)) as u8);
            bits -= 8;
        }
    }

    Ok(result)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::email_server::auth::create_test_auth_handler;

    /// A valid JSON `SecureMessage` body must round-trip byte-identical out of the store, not be
    /// re-derived (the old code hand-picked the raw `DATA` bytes into `encrypted_content` instead
    /// of parsing the body as a whole `SecureMessage`).
    #[tokio::test]
    async fn store_message_round_trips_a_json_secure_message_byte_identical() {
        let auth_handler: Arc<dyn AuthHandler + Send + Sync> = Arc::new(create_test_auth_handler());
        let message_store = Arc::new(Mutex::new(HashMap::new()));
        let server = SynapseSmtpServer::new(
            SmtpServerConfig::default(),
            auth_handler,
            Arc::clone(&message_store),
        );

        let original = SecureMessage::new(
            "bob@synapse.local",
            "alice@synapse.local",
            b"hello".to_vec(),
            crate::types::SecurityLevel::Public,
        );
        let body = serde_json::to_vec(&original).unwrap();

        server
            .store_message(SmtpMessage {
                from: "alice@synapse.local".to_string(),
                to: vec!["bob@synapse.local".to_string()],
                data: body,
            })
            .await
            .expect("a valid JSON SecureMessage must be accepted");

        let stored = server.drain_messages("bob@synapse.local").unwrap();
        assert_eq!(stored.len(), 1);
        assert_eq!(
            stored[0].message_id.0, original.message_id.0,
            "must round-trip byte-identical, not re-derived"
        );
        assert_eq!(stored[0].encrypted_content, original.encrypted_content);
    }

    /// A non-JSON body must be refused with `InvalidMessageFormat`, not silently wrapped into
    /// `encrypted_content` as the old code did.
    #[tokio::test]
    async fn store_message_refuses_a_non_json_body() {
        let auth_handler: Arc<dyn AuthHandler + Send + Sync> = Arc::new(create_test_auth_handler());
        let message_store = Arc::new(Mutex::new(HashMap::new()));
        let server =
            SynapseSmtpServer::new(SmtpServerConfig::default(), auth_handler, message_store);

        let err = server
            .store_message(SmtpMessage {
                from: "alice@synapse.local".to_string(),
                to: vec!["bob@synapse.local".to_string()],
                data: b"not json at all".to_vec(),
            })
            .await
            .expect_err(
                "a non-JSON body must be refused, not silently wrapped as encrypted_content",
            );
        assert!(matches!(err, SynapseError::InvalidMessageFormat(_)));
    }
}
