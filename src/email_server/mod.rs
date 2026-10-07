// SPDX-License-Identifier: MIT OR Apache-2.0
//! Synapse Email Server Implementation
//!
//! High-performance SMTP and IMAP servers optimized for low-latency communication

pub mod auth;
pub mod connectivity;
pub mod imap_server;
pub mod security;
pub mod smtp_server;

pub use auth::{SynapseAuthHandler, UserAccount, UserPermissions};
pub use connectivity::{ConnectivityAssessment, ConnectivityDetector, ServerRecommendation};
pub use imap_server::{ImapServerConfig, SynapseImapServer};
pub use smtp_server::{AuthHandler, SmtpServerConfig, SynapseSmtpServer};

use crate::error::Result;
use std::collections::HashMap;
use std::sync::{Arc, Mutex};
use tracing::{info, warn};

/// Complete Synapse email server with both SMTP and IMAP
pub struct SynapseEmailServer {
    smtp_server: SynapseSmtpServer,
    imap_server: SynapseImapServer,
    connectivity: ConnectivityAssessment,
    auth_handler: Arc<SynapseAuthHandler>,
    /// The limiters and sink both servers share (hardening P7).
    security: security::EmailSecurity,
}

impl SynapseEmailServer {
    /// Create a new email server with automatic configuration, listening on loopback only.
    pub async fn new() -> Result<Self> {
        Self::new_with_scope(crate::network_scope::BindScope::default()).await
    }

    /// Create a new email server that probes and listens in `bind_scope`.
    pub async fn new_with_scope(bind_scope: crate::network_scope::BindScope) -> Result<Self> {
        // Assess connectivity first
        let detector = ConnectivityDetector::default().with_bind_scope(bind_scope);
        let connectivity = detector.assess_connectivity().await?;

        info!(
            "Email server connectivity assessment: {:?}",
            connectivity.recommended_config
        );

        // Create auth handler
        let auth_handler = Arc::new(SynapseAuthHandler::new());

        // Configure SMTP server
        let smtp_config = match &connectivity.recommended_config {
            ServerRecommendation::RunLocalServer { smtp_port, .. } => SmtpServerConfig {
                port: *smtp_port,
                bind_scope,
                ..Default::default()
            },
            _ => SmtpServerConfig {
                bind_scope,
                ..Default::default()
            },
        };

        // Configure IMAP server
        let imap_config = match &connectivity.recommended_config {
            ServerRecommendation::RunLocalServer { imap_port, .. } => ImapServerConfig {
                port: *imap_port,
                bind_scope,
                ..Default::default()
            },
            _ => ImapServerConfig {
                bind_scope,
                ..Default::default()
            },
        };

        Ok(Self::assemble(
            smtp_config,
            imap_config,
            connectivity,
            auth_handler,
        ))
    }

    /// Create email server with custom configuration
    pub fn with_config(
        smtp_config: SmtpServerConfig,
        imap_config: ImapServerConfig,
        connectivity: ConnectivityAssessment,
    ) -> Result<Self> {
        let auth_handler = Arc::new(SynapseAuthHandler::new());
        Ok(Self::assemble(
            smtp_config,
            imap_config,
            connectivity,
            auth_handler,
        ))
    }

    /// Builds the pair both constructors return. The servers share one message store (previously
    /// the SMTP server held its own private store that the IMAP server, and everything else, could
    /// never read) and one login guard, so guesses split across SMTP and IMAP count against one
    /// budget.
    fn assemble(
        smtp_config: SmtpServerConfig,
        imap_config: ImapServerConfig,
        connectivity: ConnectivityAssessment,
        auth_handler: Arc<SynapseAuthHandler>,
    ) -> Self {
        let message_store = Arc::new(Mutex::new(HashMap::new()));
        let security = security::EmailSecurity::default();

        let smtp_server = SynapseSmtpServer::new(
            smtp_config,
            Arc::clone(&auth_handler) as Arc<dyn AuthHandler + Send + Sync>,
            Arc::clone(&message_store),
        )
        .with_security(security.clone());
        let imap_server = SynapseImapServer::new(
            imap_config,
            Arc::clone(&message_store),
            Arc::clone(&auth_handler) as Arc<dyn AuthHandler + Send + Sync>,
        )
        .with_security(security.clone());

        Self {
            smtp_server,
            imap_server,
            connectivity,
            auth_handler,
            security,
        }
    }

    /// Where both servers' security events go (hardening P7). The first call wins.
    #[must_use]
    pub fn with_security_sink(self, sink: Arc<dyn crate::security_events::SecuritySink>) -> Self {
        self.security.set_sink(sink);
        self
    }

    /// Replaces the login guard's tuning for both servers, which keep sharing one budget.
    #[must_use]
    pub fn with_login_limits(mut self, limits: security::LoginLimits) -> Self {
        self.security = self.security.with_login(limits);
        self.smtp_server = self.smtp_server.with_security(self.security.clone());
        self.imap_server = self.imap_server.with_security(self.security.clone());
        self
    }

    /// Start both SMTP and IMAP servers
    pub async fn start(&self) -> Result<()> {
        match &self.connectivity.recommended_config {
            ServerRecommendation::RunLocalServer {
                smtp_port,
                imap_port,
                external_ip,
            } => {
                info!(
                    "Starting local email server on {}:{}/{}",
                    external_ip, smtp_port, imap_port
                );

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
    pub fn get_auth_handler(&self) -> Arc<SynapseAuthHandler> {
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::security_events::LimiterConfig;
    use chrono::{Duration, TimeZone, Utc};
    use std::net::{IpAddr, Ipv4Addr};

    fn t0() -> chrono::DateTime<Utc> {
        Utc.with_ymd_and_hms(2026, 10, 7, 12, 0, 0).unwrap()
    }

    fn pair() -> SynapseEmailServer {
        let connectivity = ConnectivityAssessment {
            can_bind_smtp: false,
            can_bind_imap: false,
            has_external_ip: false,
            external_ip: Some(IpAddr::V4(Ipv4Addr::LOCALHOST)),
            firewall_status: connectivity::FirewallStatus::Unknown,
            recommended_config: ServerRecommendation::ExternalProvider {
                reason: "test".into(),
            },
        };
        SynapseEmailServer::with_config(
            SmtpServerConfig::default(),
            ImapServerConfig::default(),
            connectivity,
        )
        .unwrap()
    }

    /// One failed login on each server, alternating SMTP `AUTH` and IMAP `LOGIN`.
    fn fail_alternately(server: &SynapseEmailServer, failures: u32, now: chrono::DateTime<Utc>) {
        for i in 0..failures {
            let (security, surface) = if i % 2 == 0 {
                (server.smtp_server.security(), "smtp_auth")
            } else {
                (server.imap_server.security(), "imap_login")
            };
            security
                .begin_login(surface, "alice", "10.0.0.1", now)
                .expect("refused before the shared budget was spent")
                .failed(now);
        }
    }

    fn refused_on_both(
        server: &SynapseEmailServer,
        user: &str,
        now: chrono::DateTime<Utc>,
    ) -> bool {
        let smtp = server.smtp_server.security();
        let imap = server.imap_server.security();
        smtp.begin_login("smtp_auth", user, "10.0.0.1", now)
            .is_none()
            && imap
                .begin_login("imap_login", user, "10.0.0.1", now)
                .is_none()
    }

    fn accepted_on_both(
        server: &SynapseEmailServer,
        user: &str,
        now: chrono::DateTime<Utc>,
    ) -> bool {
        // Each attempt drops at once, which releases its slot without recording a failure.
        let smtp = server.smtp_server.security();
        let imap = server.imap_server.security();
        smtp.begin_login("smtp_auth", user, "10.0.0.1", now)
            .is_some()
            && imap
                .begin_login("imap_login", user, "10.0.0.1", now)
                .is_some()
    }

    // P7 follow-up (a2): an attacker who splits guesses between SMTP and IMAP gets one budget, not
    // one per server. The default per-user lockout is ten failures.
    #[test]
    fn smtp_and_imap_failures_share_one_login_budget() {
        let server = pair();
        let now = t0();
        fail_alternately(&server, 9, now);
        // Positive control: nine failures, five on SMTP and four on IMAP, leave one attempt.
        assert!(accepted_on_both(&server, "alice", now));
        let imap = server.imap_server.security();
        imap.begin_login("imap_login", "alice", "10.0.0.1", now)
            .unwrap()
            .failed(now);
        // Negative control: the tenth failure locks alice on both servers, though neither saw ten.
        assert!(refused_on_both(&server, "alice", now));
        // Positive control: another user is unaffected.
        assert!(accepted_on_both(&server, "bob", now));
    }

    // `with_login_limits` replaces the guard; both servers must take the same replacement.
    #[test]
    fn new_login_limits_keep_the_budget_shared() {
        let limiter = LimiterConfig {
            free_failures: 1,
            window: Duration::minutes(15),
            base_delay: Duration::milliseconds(1),
            max_delay: Duration::milliseconds(5),
            lockout_after: 2,
            lockout: Duration::minutes(15),
            max_keys: 100,
        };
        let server = pair().with_login_limits(security::LoginLimits {
            per_user: limiter.clone(),
            per_source: LimiterConfig {
                lockout_after: 1_000,
                ..limiter
            },
            event_interval: Duration::zero(),
        });
        let now = t0();
        fail_alternately(&server, 1, now);
        // Positive control: one SMTP failure of two leaves IMAP an attempt.
        assert!(accepted_on_both(&server, "alice", now));
        let imap = server.imap_server.security();
        imap.begin_login("imap_login", "alice", "10.0.0.1", now)
            .unwrap()
            .failed(now);
        // Negative control: one failure on each server reaches the new limit of two.
        assert!(refused_on_both(&server, "alice", now));
    }
}
