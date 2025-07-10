//! High-performance IMAP server for EMRP

use crate::error::{EmrpError, Result};
use crate::types::SecureMessage;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use tokio::net::{TcpListener, TcpStream};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use std::time::{SystemTime, UNIX_EPOCH};
use tracing::{info, error, debug};

/// High-performance IMAP server optimized for EMRP
pub struct EmrpImapServer {
    /// Server configuration
    config: ImapServerConfig,
    /// Message store (shared with SMTP server)
    message_store: Arc<Mutex<HashMap<String, Vec<SecureMessage>>>>,
    /// Connected clients
    clients: Arc<Mutex<HashMap<String, ImapSession>>>,
    /// Authorization handler
    auth_handler: Arc<dyn super::smtp_server::AuthHandler + Send + Sync>,
}

#[derive(Debug, Clone)]
pub struct ImapServerConfig {
    /// IMAP port (usually 143 or 993)
    pub port: u16,
    /// TLS configuration
    pub tls_config: Option<super::smtp_server::TlsConfig>,
    /// Enable IDLE extension for push notifications
    pub enable_idle: bool,
    /// Performance optimization
    pub performance: ImapPerformanceConfig,
}

#[derive(Debug, Clone)]
pub struct ImapPerformanceConfig {
    /// Maximum concurrent connections
    pub max_connections: usize,
    /// IDLE timeout (seconds)
    pub idle_timeout: u64,
    /// Compression support
    pub enable_compression: bool,
}

#[derive(Debug)]
struct ImapSession {
    #[allow(dead_code)]
    id: String,
    state: ImapState,
    authenticated_user: Option<String>,
    selected_mailbox: Option<String>,
    #[allow(dead_code)]
    tag: u32,
    idle_mode: bool,
}

#[derive(Debug, PartialEq)]
enum ImapState {
    NotAuthenticated,
    Authenticated,
    Selected,
    Logout,
}

impl Default for ImapServerConfig {
    fn default() -> Self {
        Self {
            port: 1143, // Non-privileged port for development
            tls_config: None,
            enable_idle: true,
            performance: ImapPerformanceConfig {
                max_connections: 100,
                idle_timeout: 1740, // 29 minutes (RFC requirement)
                enable_compression: true,
            },
        }
    }
}

impl EmrpImapServer {
    /// Create a new IMAP server
    pub fn new(
        config: ImapServerConfig,
        message_store: Arc<Mutex<HashMap<String, Vec<SecureMessage>>>>,
        auth_handler: Arc<dyn super::smtp_server::AuthHandler + Send + Sync>,
    ) -> Self {
        Self {
            config,
            message_store,
            clients: Arc::new(Mutex::new(HashMap::new())),
            auth_handler,
        }
    }

    /// Start the IMAP server
    pub async fn start(&self) -> Result<()> {
        let addr = format!("0.0.0.0:{}", self.config.port);
        let listener = TcpListener::bind(&addr).await
            .map_err(|e| EmrpError::Network(format!("Failed to bind IMAP server to {}: {}", addr, e)))?;

        info!("EMRP IMAP Server listening on {}", addr);

        loop {
            match listener.accept().await {
                Ok((stream, addr)) => {
                    info!("New IMAP connection from {}", addr);

                    // Handle connection in background task
                    let server = self.clone();
                    tokio::spawn(async move {
                        if let Err(e) = server.handle_connection(stream).await {
                            error!("IMAP connection error: {}", e);
                        }
                    });
                }
                Err(e) => {
                    error!("Failed to accept IMAP connection: {}", e);
                }
            }
        }
    }

    /// Handle individual IMAP connection
    async fn handle_connection(&self, stream: TcpStream) -> Result<()> {
        let (read_half, write_half) = stream.into_split();
        let mut reader = BufReader::new(read_half);
        let mut writer = write_half;
        
        let mut session = ImapSession {
            id: format!("imap_{}", SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_millis()),
            state: ImapState::NotAuthenticated,
            authenticated_user: None,
            selected_mailbox: None,
            tag: 0,
            idle_mode: false,
        };

        // Send greeting
        writer.write_all(b"* OK EMRP IMAP Server Ready\r\n").await?;
        writer.flush().await?;

        let mut line = String::new();
        while reader.read_line(&mut line).await? > 0 {
            let command = line.trim();
            debug!("IMAP command: {}", command);

            let responses = self.process_imap_command(command, &mut session).await?;
            
            for response in responses {
                writer.write_all(response.as_bytes()).await?;
            }
            writer.flush().await?;

            // Check if connection should close
            if session.state == ImapState::Logout {
                break;
            }

            line.clear();
        }

        Ok(())
    }

    /// Process IMAP command
    async fn process_imap_command(&self, command: &str, session: &mut ImapSession) -> Result<Vec<String>> {
        let parts: Vec<&str> = command.split_whitespace().collect();
        if parts.len() < 2 {
            return Ok(vec!["* BAD Syntax error\r\n".to_string()]);
        }

        let tag = parts[0];
        let cmd = parts[1].to_uppercase();
        
        match cmd.as_str() {
            "CAPABILITY" => {
                let mut responses = vec![
                    "* CAPABILITY IMAP4rev1 IDLE AUTH=PLAIN AUTH=LOGIN UIDPLUS\r\n".to_string(),
                ];
                responses.push(format!("{} OK CAPABILITY completed\r\n", tag));
                Ok(responses)
            }
            "LOGIN" => {
                if parts.len() < 4 {
                    return Ok(vec![format!("{} BAD LOGIN requires username and password\r\n", tag)]);
                }
                
                let username = parts[2].trim_matches('"');
                let password = parts[3].trim_matches('"');
                
                match self.auth_handler.authenticate(username, password) {
                    Ok(true) => {
                        session.state = ImapState::Authenticated;
                        session.authenticated_user = Some(username.to_string());
                        Ok(vec![format!("{} OK LOGIN completed\r\n", tag)])
                    }
                    Ok(false) => {
                        Ok(vec![format!("{} NO LOGIN failed\r\n", tag)])
                    }
                    Err(_) => {
                        Ok(vec![format!("{} NO LOGIN failed\r\n", tag)])
                    }
                }
            }
            "LIST" => {
                if session.state == ImapState::NotAuthenticated {
                    return Ok(vec![format!("{} NO Not authenticated\r\n", tag)]);
                }
                
                let mut responses = vec![
                    "* LIST () \"/\" \"INBOX\"\r\n".to_string(),
                ];
                responses.push(format!("{} OK LIST completed\r\n", tag));
                Ok(responses)
            }
            "SELECT" => {
                if session.state == ImapState::NotAuthenticated {
                    return Ok(vec![format!("{} NO Not authenticated\r\n", tag)]);
                }
                
                if parts.len() < 3 {
                    return Ok(vec![format!("{} BAD SELECT requires mailbox name\r\n", tag)]);
                }
                
                let mailbox = parts[2].trim_matches('"');
                
                // Get message count for this user
                let message_count = {
                    let store = self.message_store.lock().unwrap();
                    if let Some(user) = &session.authenticated_user {
                        store.get(user).map(|msgs| msgs.len()).unwrap_or(0)
                    } else {
                        0
                    }
                };
                
                session.state = ImapState::Selected;
                session.selected_mailbox = Some(mailbox.to_string());
                
                let mut responses = vec![
                    format!("* {} EXISTS\r\n", message_count),
                    "* 0 RECENT\r\n".to_string(),
                    "* OK [UIDVALIDITY 1] UIDs valid\r\n".to_string(),
                    "* OK [UIDNEXT 1] Predicted next UID\r\n".to_string(),
                    "* FLAGS (\\Answered \\Flagged \\Deleted \\Seen \\Draft)\r\n".to_string(),
                    "* OK [PERMANENTFLAGS (\\Deleted \\Seen \\*)] Limited\r\n".to_string(),
                ];
                responses.push(format!("{} OK [READ-WRITE] SELECT completed\r\n", tag));
                Ok(responses)
            }
            "FETCH" => {
                if session.state != ImapState::Selected {
                    return Ok(vec![format!("{} NO Not in selected state\r\n", tag)]);
                }
                
                if parts.len() < 4 {
                    return Ok(vec![format!("{} BAD FETCH requires sequence set and items\r\n", tag)]);
                }
                
                let sequence_set = parts[2];
                let items = parts[3..].join(" ");
                
                // Get messages for this user
                let messages = {
                    let store = self.message_store.lock().unwrap();
                    if let Some(user) = &session.authenticated_user {
                        store.get(user).cloned().unwrap_or_default()
                    } else {
                        Vec::new()
                    }
                };
                
                let mut responses = Vec::new();
                
                // Parse sequence set (simplified - just handle "1:*" and single numbers)
                let seq_nums: Vec<usize> = if sequence_set == "1:*" {
                    (1..=messages.len()).collect()
                } else if let Ok(num) = sequence_set.parse::<usize>() {
                    if num > 0 && num <= messages.len() {
                        vec![num]
                    } else {
                        Vec::new()
                    }
                } else {
                    Vec::new()
                };
                
                for seq_num in seq_nums {
                    if let Some(message) = messages.get(seq_num - 1) {
                        if items.contains("RFC822") || items.contains("BODY[]") {
                            let email_content = self.format_as_email(message);
                            responses.push(format!("* {} FETCH (RFC822 {{{}}}\r\n", seq_num, email_content.len()));
                            responses.push(format!("{}\r\n", email_content));
                            responses.push(")\r\n".to_string());
                        } else if items.contains("FLAGS") {
                            responses.push(format!("* {} FETCH (FLAGS ())\r\n", seq_num));
                        }
                    }
                }
                
                responses.push(format!("{} OK FETCH completed\r\n", tag));
                Ok(responses)
            }
            "IDLE" => {
                if session.state != ImapState::Selected {
                    return Ok(vec![format!("{} NO Not in selected state\r\n", tag)]);
                }
                
                if !self.config.enable_idle {
                    return Ok(vec![format!("{} NO IDLE not supported\r\n", tag)]);
                }
                
                session.idle_mode = true;
                Ok(vec!["+ idling\r\n".to_string()])
            }
            "DONE" => {
                if session.idle_mode {
                    session.idle_mode = false;
                    Ok(vec![format!("{} OK IDLE terminated\r\n", tag)])
                } else {
                    Ok(vec![format!("{} BAD Not in IDLE\r\n", tag)])
                }
            }
            "LOGOUT" => {
                session.state = ImapState::Logout;
                let mut responses = vec![
                    "* BYE EMRP IMAP Server logging out\r\n".to_string(),
                ];
                responses.push(format!("{} OK LOGOUT completed\r\n", tag));
                Ok(responses)
            }
            _ => {
                Ok(vec![format!("{} BAD Command not recognized\r\n", tag)])
            }
        }
    }

    /// Format SecureMessage as email content
    fn format_as_email(&self, message: &SecureMessage) -> String {
        let content = String::from_utf8_lossy(&message.encrypted_content);
        format!(
            "From: {}\r\nTo: {}\r\nSubject: EMRP Message\r\nDate: {:?}\r\n\r\n{}",
            message.from_global_id,
            message.to_global_id,
            message.timestamp,
            content
        )
    }
}

impl Clone for EmrpImapServer {
    fn clone(&self) -> Self {
        Self {
            config: self.config.clone(),
            message_store: Arc::clone(&self.message_store),
            clients: Arc::clone(&self.clients),
            auth_handler: Arc::clone(&self.auth_handler),
        }
    }
}
