//! EMRP Email Server Implementation
//! 
//! High-performance SMTP and IMAP servers optimized for low-latency communication

pub mod smtp_server;
pub mod imap_server;
pub mod connectivity;
pub mod auth;

pub use smtp_server::{EmrpSmtpServer, SmtpServerConfig, AuthHandler};
pub use imap_server::{EmrpImapServer, ImapServerConfig};
pub use connectivity::{ConnectivityDetector, ConnectivityAssessment, ServerRecommendation};
pub use auth::{EmrpAuthHandler, UserAccount, UserPermissions, create_test_auth_handler};

use crate::error::Result;
use std::sync::{Arc, Mutex};
use std::collections::HashMap;
use tracing::{info, warn};

/// Complete EMRP email server with both SMTP and IMAP
pub struct EmrpEmailServer {
    smtp_server: EmrpSmtpServer,
    imap_server: EmrpImapServer,
    connectivity: ConnectivityAssessment,
    auth_handler: Arc<EmrpAuthHandler>,
}

impl EmrpEmailServer {
    /// Create a new email server with automatic configuration
    pub async fn new() -> Result<Self> {
        // Assess connectivity first
        let detector = ConnectivityDetector::default();
        let connectivity = detector.assess_connectivity().await?;
        
        info!("Email server connectivity assessment: {:?}", connectivity.recommended_config);
        
        // Create auth handler
        let auth_handler = Arc::new(EmrpAuthHandler::new());
        
        // Configure SMTP server
        let smtp_config = match &connectivity.recommended_config {
            ServerRecommendation::RunLocalServer { smtp_port, .. } => {
                SmtpServerConfig {
                    port: *smtp_port,
                    ..Default::default()
                }
            }
            _ => SmtpServerConfig::default(),
        };
        
        // Configure IMAP server
        let imap_config = match &connectivity.recommended_config {
            ServerRecommendation::RunLocalServer { imap_port, .. } => {
                ImapServerConfig {
                    port: *imap_port,
                    ..Default::default()
                }
            }
            _ => ImapServerConfig::default(),
        };
        
        // Shared message store
        let message_store = Arc::new(Mutex::new(HashMap::new()));
        
        // Create servers
        let smtp_server = EmrpSmtpServer::new(smtp_config, Arc::clone(&auth_handler) as Arc<dyn AuthHandler + Send + Sync>);
        let imap_server = EmrpImapServer::new(imap_config, Arc::clone(&message_store), Arc::clone(&auth_handler) as Arc<dyn AuthHandler + Send + Sync>);
        
        Ok(Self {
            smtp_server,
            imap_server,
            connectivity,
            auth_handler,
        })
    }

    /// Create email server with custom configuration
    pub fn with_config(
        smtp_config: SmtpServerConfig,
        imap_config: ImapServerConfig,
        connectivity: ConnectivityAssessment,
    ) -> Result<Self> {
        let auth_handler = Arc::new(EmrpAuthHandler::new());
        let message_store = Arc::new(Mutex::new(HashMap::new()));
        
        let smtp_server = EmrpSmtpServer::new(smtp_config, Arc::clone(&auth_handler) as Arc<dyn AuthHandler + Send + Sync>);
        let imap_server = EmrpImapServer::new(imap_config, Arc::clone(&message_store), Arc::clone(&auth_handler) as Arc<dyn AuthHandler + Send + Sync>);
        
        Ok(Self {
            smtp_server,
            imap_server,
            connectivity,
            auth_handler,
        })
    }

    /// Start both SMTP and IMAP servers
    pub async fn start(&self) -> Result<()> {
        match &self.connectivity.recommended_config {
            ServerRecommendation::RunLocalServer { smtp_port, imap_port, external_ip } => {
                info!("Starting local email server on {}:{}/{}", external_ip, smtp_port, imap_port);
                
                // Start SMTP server in background
                let smtp_server = self.smtp_server.clone();
                tokio::spawn(async move {
                    if let Err(e) = smtp_server.start().await {
                        warn!("SMTP server error: {}", e);
                    }
                });
                
                // Start IMAP server in background
                let imap_server = self.imap_server.clone();
                tokio::spawn(async move {
                    if let Err(e) = imap_server.start().await {
                        warn!("IMAP server error: {}", e);
                    }
                });
                
                info!("Email servers started successfully");
                Ok(())
            }
            ServerRecommendation::RelayOnly { reason } => {
                warn!("Email server in relay-only mode: {}", reason);
                
                // Start SMTP server only for outgoing mail
                let smtp_server = self.smtp_server.clone();
                tokio::spawn(async move {
                    if let Err(e) = smtp_server.start().await {
                        warn!("SMTP relay server error: {}", e);
                    }
                });
                
                Ok(())
            }
            ServerRecommendation::ExternalProvider { reason } => {
                warn!("Using external email provider: {}", reason);
                // No local servers to start
                Ok(())
            }
        }
    }

    /// Get connectivity assessment
    pub fn get_connectivity(&self) -> &ConnectivityAssessment {
        &self.connectivity
    }

    /// Get auth handler for configuration
    pub fn get_auth_handler(&self) -> Arc<EmrpAuthHandler> {
        Arc::clone(&self.auth_handler)
    }

    /// Add user account
    pub fn add_user(&self, user: UserAccount) -> Result<()> {
        self.auth_handler.add_user(user)
    }

    /// Add local domain for receiving email
    pub fn add_local_domain(&self, domain: &str) -> Result<()> {
        self.auth_handler.add_local_domain(domain)
    }

    /// Add relay domain for forwarding email
    pub fn add_relay_domain(&self, domain: &str) -> Result<()> {
        self.auth_handler.add_relay_domain(domain)
    }

    /// Check if server should be used based on connectivity
    pub fn should_use_local_server(&self) -> bool {
        matches!(
            self.connectivity.recommended_config,
            ServerRecommendation::RunLocalServer { .. }
        )
    }

    /// Check if server can relay for remote clients
    pub fn can_relay_for_clients(&self) -> bool {
        matches!(
            self.connectivity.recommended_config,
            ServerRecommendation::RunLocalServer { .. } | ServerRecommendation::RelayOnly { .. }
        )
    }
}

/// Create a test email server for development
pub async fn create_test_email_server() -> Result<EmrpEmailServer> {
    let auth_handler = Arc::new(create_test_auth_handler());
    
    // Use test configuration
    let smtp_config = SmtpServerConfig {
        port: 2525,
        require_auth: false, // Easier for testing
        ..Default::default()
    };
    
    let imap_config = ImapServerConfig {
        port: 1143,
        ..Default::default()
    };
    
    // Mock connectivity assessment for testing
    let connectivity = ConnectivityAssessment {
        can_bind_smtp: true,
        can_bind_imap: true,
        has_external_ip: false,
        external_ip: None,
        firewall_status: connectivity::FirewallStatus::Unknown,
        recommended_config: ServerRecommendation::RunLocalServer {
            smtp_port: 2525,
            imap_port: 1143,
            external_ip: std::net::IpAddr::V4(std::net::Ipv4Addr::new(127, 0, 0, 1)),
        },
    };
    
    let message_store = Arc::new(Mutex::new(HashMap::new()));
    let smtp_server = EmrpSmtpServer::new(smtp_config, Arc::clone(&auth_handler) as Arc<dyn AuthHandler + Send + Sync>);
    let imap_server = EmrpImapServer::new(imap_config, Arc::clone(&message_store), Arc::clone(&auth_handler) as Arc<dyn AuthHandler + Send + Sync>);
    
    Ok(EmrpEmailServer {
        smtp_server,
        imap_server,
        connectivity,
        auth_handler,
    })
}
