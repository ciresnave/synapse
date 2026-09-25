// SPDX-License-Identifier: MIT OR Apache-2.0
//! Simplified Email Transport implementation.
//!
//! This transport touches no network in either direction. Until the email
//! slice builds real SMTP/IMAP support, it refuses to operate rather than
//! pretend: it validates addresses honestly, but every send/receive/connect
//! path returns an explicit "not implemented yet" error instead of a faked
//! success.

use super::abstraction::*;
use crate::{
    error::{Result, SynapseError},
    types::{EmailConfig, SecureMessage},
};
use async_trait::async_trait;
use std::{collections::HashMap, sync::Arc, time::Duration};
use tracing::info;

/// The one address validator every path in this transport uses: exactly one
/// `@`, a non-empty local part, and a domain containing a `.` with
/// non-empty labels on each side of it.
fn valid_address(address: &str) -> bool {
    let mut parts = address.split('@');
    let Some(local) = parts.next() else {
        return false;
    };
    let Some(domain) = parts.next() else {
        return false;
    };
    if parts.next().is_some() {
        // More than one '@'.
        return false;
    }
    if local.is_empty() || domain.is_empty() {
        return false;
    }
    domain.contains('.') && domain.split('.').all(|label| !label.is_empty())
}

/// The error returned by every operation this transport cannot honestly
/// perform yet.
const NOT_IMPLEMENTED: &str =
    "email transport is not implemented yet; it arrives in the email slice";

/// Simplified Email Transport implementation
pub struct SimpleEmailTransport {
    /// Email configuration
    config: EmailConfig,
    /// Connection timeout
    #[expect(
        dead_code,
        reason = "set to 30s and never applied to any connection; see CAPABILITY_INVENTORY.md"
    )]
    connection_timeout: Duration,
}

impl SimpleEmailTransport {
    /// Create new simplified email transport
    pub fn new(config: EmailConfig) -> Result<Self> {
        Ok(Self {
            config,
            connection_timeout: Duration::from_secs(30),
        })
    }
}

#[async_trait]
impl Transport for SimpleEmailTransport {
    fn transport_type(&self) -> TransportType {
        TransportType::Email
    }

    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            max_message_size: 10 * 1024 * 1024, // 10MB typical email limit
            reliable: true,
            real_time: false,
            broadcast: false,
            bidirectional: true,
            encrypted: true, // Can use TLS
            network_spanning: true,
            supported_urgencies: vec![
                MessageUrgency::Background,
                MessageUrgency::Batch,
                MessageUrgency::Interactive,
            ],
            features: vec![
                "async_messaging".to_string(),
                "persistent_delivery".to_string(),
                "cross_network".to_string(),
                "store_and_forward".to_string(),
            ],
            // Every send/receive/connect path refuses until the email slice lands, so there is no
            // `metrics()` observation for either field to be honest or dishonest about yet.
            unmeasured_metrics: vec![
                UnmeasuredMetric::AverageLatency,
                UnmeasuredMetric::ReliabilityScore,
            ],
        }
    }

    async fn can_reach(&self, target: &TransportTarget) -> bool {
        valid_address(&target.identifier)
    }

    async fn estimate_metrics(&self, _target: &TransportTarget) -> Result<TransportEstimate> {
        // Every send refuses until the email slice lands, so this transport is not available
        // for any target, however well-formed its address (matching `metrics()`).
        Ok(TransportEstimate {
            latency: Duration::from_secs(60),
            reliability: 0.0,
            bandwidth: 0,
            cost: 0.1,
            available: false,
            confidence: 0.9,
        })
    }

    async fn send_message(
        &self,
        target: &TransportTarget,
        _message: &SecureMessage,
    ) -> Result<DeliveryReceipt> {
        // Never claim a delivery this transport did not make.
        if !valid_address(&target.identifier) {
            return Err(SynapseError::TransportError(
                "invalid email address".to_string(),
            ));
        }
        Err(SynapseError::TransportError(NOT_IMPLEMENTED.to_string()))
    }

    async fn test_connectivity(&self, target: &TransportTarget) -> Result<ConnectivityResult> {
        let email_address = &target.identifier;

        if !valid_address(email_address) {
            return Ok(ConnectivityResult {
                connected: false,
                rtt: None,
                error: Some("invalid email address".to_string()),
                quality: 0.0,
                details: {
                    let mut map = HashMap::new();
                    map.insert("error".to_string(), "invalid email address".to_string());
                    map
                },
            });
        }

        // Never claim a connection this transport did not make.
        Err(SynapseError::TransportError(NOT_IMPLEMENTED.to_string()))
    }

    async fn start(&self) -> Result<()> {
        info!("Starting simplified email transport");

        // Test basic configuration
        if self.config.smtp.host.is_empty() {
            return Err(SynapseError::ConfigurationError(
                "SMTP host not configured".to_string(),
            ));
        }

        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        info!("Stopping simplified email transport");
        Ok(())
    }

    async fn status(&self) -> TransportStatus {
        TransportStatus::Running
    }

    async fn metrics(&self) -> TransportMetrics {
        TransportMetrics {
            transport_type: TransportType::Email,
            messages_sent: 0,
            messages_received: 0,
            send_failures: 0,
            receive_failures: 0,
            bytes_sent: 0,
            bytes_received: 0,
            average_latency_ms: 60000, // 60 seconds
            // Every send/receive/connect path refuses until the email slice lands, and connection
            // offers take the trait default, which refuses too, so nothing this transport does
            // succeeds.
            reliability_score: 0.0,
            active_connections: 0,
            last_updated_timestamp: std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap_or_default()
                .as_secs(),
            custom_metrics: HashMap::new(),
        }
    }
}

#[async_trait]
impl TransportReceive for SimpleEmailTransport {
    async fn receive_raw(&self, _inbox: &mut RawInbox) -> Result<()> {
        // Never claim to have checked for mail this transport did not check.
        Err(SynapseError::TransportError(NOT_IMPLEMENTED.to_string()))
    }
}

/// Factory for creating simplified email transports
pub struct SimpleEmailTransportFactory;

impl SimpleEmailTransportFactory {
    pub fn new() -> Self {
        Self
    }

    pub async fn create_transport(&self, config: EmailConfig) -> Result<Arc<SimpleEmailTransport>> {
        let transport = SimpleEmailTransport::new(config)?;
        Ok(Arc::new(transport))
    }
}

impl Default for SimpleEmailTransportFactory {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::types::{ImapConfig, SecretString, SecurityLevel, SmtpConfig};

    fn test_config() -> EmailConfig {
        EmailConfig {
            smtp: SmtpConfig {
                host: "smtp.example.com".to_string(),
                port: 587,
                username: "test@example.com".to_string(),
                password: SecretString::new("password"),
                use_tls: true,
                use_ssl: false,
            },
            imap: ImapConfig {
                host: "imap.example.com".to_string(),
                port: 993,
                username: "test@example.com".to_string(),
                password: SecretString::new("password"),
                use_ssl: true,
            },
        }
    }

    #[tokio::test]
    async fn test_simple_email_transport_creation() {
        let transport = SimpleEmailTransport::new(test_config());
        assert!(transport.is_ok());
    }

    #[test]
    fn one_validator_governs_every_path() {
        for bad in [
            "invalid@",
            "@example.com",
            "a@@example.com",
            "a@example",
            "a@.com",
            "a@example.",
            "a@b..c",
            "a@b.c.",
        ] {
            assert!(!valid_address(bad), "{bad} must be refused");
        }
        for good in ["user@example.com", "first.last@mail.example.org"] {
            assert!(valid_address(good), "{good} must be accepted");
        }
    }

    #[tokio::test]
    async fn a_malformed_address_is_refused_by_connectivity_and_send() {
        let transport = SimpleEmailTransport::new(test_config()).unwrap();
        let target = TransportTarget::new("invalid@".to_string());
        assert!(
            !transport
                .test_connectivity(&target)
                .await
                .unwrap()
                .connected
        );
        let message = SecureMessage::new(
            "invalid@",
            "me@example.com",
            b"hi".to_vec(),
            SecurityLevel::Public,
        );
        let err = transport.send_message(&target, &message).await.unwrap_err();
        assert!(err.to_string().contains("invalid email address"), "{err}");
    }

    #[tokio::test]
    async fn a_well_formed_address_is_refused_honestly_rather_than_faked() {
        let transport = SimpleEmailTransport::new(test_config()).unwrap();
        let target = TransportTarget::new("user@example.com".to_string());
        let message = SecureMessage::new(
            "user@example.com",
            "me@example.com",
            b"hi".to_vec(),
            SecurityLevel::Public,
        );
        let err = transport.send_message(&target, &message).await.unwrap_err();
        assert!(err.to_string().contains("email slice"), "{err}");
        assert!(transport.receive_raw(&mut RawInbox::new()).await.is_err());
        // The transport must never claim a connection it did not make.
        assert!(transport.test_connectivity(&target).await.is_err());
    }

    #[tokio::test]
    async fn a_connection_offer_is_refused_rather_than_faked() {
        use crate::transport::router::ConnectionOffer;
        let transport = SimpleEmailTransport::new(test_config()).unwrap();
        let offer = ConnectionOffer {
            from_entity: "me@example.com".to_string(),
            to_entity: "user@example.com".to_string(),
            transport_type: TransportType::Email,
            capabilities: transport.capabilities(),
            valid_until: 0,
        };
        let err = transport
            .send_connection_offer("user@example.com", offer)
            .await
            .unwrap_err();
        assert!(err.to_string().contains("not supported"), "{err}");
    }

    #[tokio::test]
    async fn estimates_do_not_advertise_a_transport_that_cannot_send() {
        let transport = SimpleEmailTransport::new(test_config()).unwrap();
        let target = TransportTarget::new("user@example.com".to_string());
        let estimate = transport.estimate_metrics(&target).await.unwrap();
        assert!(!estimate.available);
        assert_eq!(estimate.reliability, 0.0);
    }
}
