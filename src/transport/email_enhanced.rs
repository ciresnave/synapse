//! Enhanced email transport with SMTP sending capabilities

use super::SecureMessage;
use crate::circuit_breaker::{CircuitBreaker, CircuitBreakerConfig, CircuitStats};
use crate::error::{Result, SynapseError};
use crate::transport::abstraction::*;
use crate::types::{EmailConfig, ImapConfig, SmtpConfig};
use async_trait::async_trait;
use std::collections::HashMap;
use std::sync::Arc;
use std::time::{Duration, Instant};
use tokio::sync::RwLock;
use tracing::{debug, info, warn};

/// Enhanced email transport implementation with SMTP support
pub struct EmailEnhancedTransport {
    config: EmailConfig,
    circuit_breaker: CircuitBreaker,
    metrics: Arc<RwLock<TransportMetrics>>,
    status: Arc<RwLock<TransportStatus>>,
}

impl EmailEnhancedTransport {
    pub fn new(config: EmailConfig) -> Self {
        Self {
            config,
            circuit_breaker: CircuitBreaker::new(CircuitBreakerConfig::default()),
            metrics: Arc::new(RwLock::new(TransportMetrics::default())),
            status: Arc::new(RwLock::new(TransportStatus::Stopped)),
        }
    }

    pub fn get_circuit_breaker(&self) -> &CircuitBreaker {
        &self.circuit_breaker
    }

    pub fn get_circuit_breaker_stats(&self) -> CircuitStats {
        self.circuit_breaker.get_stats()
    }

    /// Send an email using SMTP
    async fn send_smtp_email(&self, to: &str, message: &SecureMessage) -> Result<()> {
        use lettre::{
            Message, SmtpTransport, Transport,
            message::{MultiPart, SinglePart, header},
            transport::smtp::authentication::Credentials,
        };

        let smtp_cfg = &self.config.smtp;

        // Create SMTP transport
        let smtp = SmtpTransport::relay(&smtp_cfg.host)
            .map_err(|e| SynapseError::TransportError(format!("SMTP relay error: {}", e)))?
            .port(smtp_cfg.port)
            .credentials(Credentials::new(
                smtp_cfg.username.clone(),
                smtp_cfg.password.clone(),
            ))
            .build();

        // Serialize message content
        let body = serde_json::to_string_pretty(message)
            .map_err(|e| SynapseError::SerializationError(e.to_string()))?;

        // Create email message
        let email = Message::builder()
            .from(smtp_cfg.username.parse().map_err(|e| {
                SynapseError::TransportError(format!("Invalid from address: {}", e))
            })?)
            .to(to
                .parse()
                .map_err(|e| SynapseError::TransportError(format!("Invalid to address: {}", e)))?)
            .subject("Synapse Network Message")
            .multipart(
                MultiPart::mixed()
                    .singlepart(
                        SinglePart::builder()
                            .header(header::ContentType::TEXT_PLAIN)
                            .body("This message contains a Synapse network payload.".to_string()),
                    )
                    .singlepart(
                        SinglePart::builder()
                            .header(header::ContentType::TEXT_PLAIN)
                            .body(body),
                    ),
            )
            .map_err(|e| SynapseError::TransportError(format!("Email build error: {}", e)))?;

        // Send email
        smtp.send(&email)
            .map_err(|e| SynapseError::TransportError(format!("SMTP send error: {}", e)))?;

        info!("Email sent successfully to {}", to);
        Ok(())
    }

    /// Update transport metrics
    async fn update_metrics(&self, operation: &str, duration: Duration, success: bool) {
        let mut metrics = self.metrics.write().await;
        metrics.total_requests += 1;
        if success {
            metrics.successful_requests += 1;
        } else {
            metrics.failed_requests += 1;
        }
        metrics.average_latency =
            (metrics.average_latency + duration) / metrics.total_requests.max(1) as u32;
        metrics.last_updated = Instant::now();

        debug!(
            "Email transport {} took {:?}, success: {}",
            operation, duration, success
        );
    }

    /// Test SMTP connection without sending a message
    async fn test_smtp_connection(&self) -> Result<()> {
        use lettre::{SmtpTransport, Transport, transport::smtp::authentication::Credentials};

        let smtp_cfg = &self.config.smtp;

        let smtp = SmtpTransport::relay(&smtp_cfg.host)
            .map_err(|e| SynapseError::TransportError(format!("SMTP relay error: {}", e)))?
            .port(smtp_cfg.port)
            .credentials(Credentials::new(
                smtp_cfg.username.clone(),
                smtp_cfg.password.clone(),
            ))
            .build();

        // Test connection (this will authenticate)
        smtp.test_connection().map_err(|e| {
            SynapseError::TransportError(format!("SMTP connection test failed: {}", e))
        })?;

        Ok(())
    }
}

impl Default for EmailEnhancedTransport {
    fn default() -> Self {
        Self {
            config: EmailConfig {
                smtp: SmtpConfig {
                    host: "smtp.gmail.com".to_string(),
                    port: 587,
                    username: "synapse@example.com".to_string(),
                    password: "password".to_string(),
                    use_tls: true,
                    use_ssl: false,
                },
                imap: ImapConfig {
                    host: "imap.gmail.com".to_string(),
                    port: 993,
                    username: "synapse@example.com".to_string(),
                    password: "password".to_string(),
                    use_ssl: true,
                },
            },
            circuit_breaker: CircuitBreaker::new(CircuitBreakerConfig::default()),
            metrics: Arc::new(RwLock::new(TransportMetrics::default())),
            status: Arc::new(RwLock::new(TransportStatus::Stopped)),
        }
    }
}

#[async_trait]
impl Transport for EmailEnhancedTransport {
    fn transport_type(&self) -> TransportType {
        TransportType::Email
    }

    fn capabilities(&self) -> TransportCapabilities {
        TransportCapabilities {
            max_message_size: 25_000_000, // 25MB typical email limit
            reliable: true,
            real_time: false,
            broadcast: false,
            bidirectional: true,
            encrypted: false, // Depends on server config
            network_spanning: true,
            supported_urgencies: vec![MessageUrgency::Background, MessageUrgency::Batch],
            features: vec![
                "store_and_forward".to_string(),
                "attachments".to_string(),
                "global_reach".to_string(),
                "authentication".to_string(),
            ],
        }
    }

    async fn can_reach(&self, target: &TransportTarget) -> bool {
        target.identifier.contains('@') && target.identifier.contains('.')
    }

    async fn estimate_metrics(&self, _target: &TransportTarget) -> Result<TransportEstimate> {
        Ok(TransportEstimate {
            latency: Duration::from_secs(30), // Typical email latency
            reliability: 0.99,
            bandwidth: 100_000, // 100KB/s
            cost: 0.01,         // Very low cost
            available: true,
            confidence: 0.95,
        })
    }

    async fn send_message(
        &self,
        target: &TransportTarget,
        message: &SecureMessage,
    ) -> Result<DeliveryReceipt> {
        let email = &target.identifier;

        if !email.contains('@') {
            return Err(SynapseError::TransportError(
                "Invalid email address".to_string(),
            ));
        }

        let start_time = Instant::now();

        match self.send_smtp_email(email, message).await {
            Ok(()) => {
                let duration = start_time.elapsed();
                self.update_metrics("send", duration, true).await;

                Ok(DeliveryReceipt {
                    message_id: uuid::Uuid::new_v4().to_string(),
                    transport_used: TransportType::Email,
                    delivery_time: duration,
                    target_reached: email.clone(),
                    confirmation: DeliveryConfirmation::Sent,
                    metadata: HashMap::new(),
                })
            }
            Err(e) => {
                let duration = start_time.elapsed();
                self.update_metrics("send", duration, false).await;
                Err(e)
            }
        }
    }

    async fn receive_messages(&self) -> Result<Vec<IncomingMessage>> {
        // IMAP receiving would be implemented here
        // For now, return empty vector as full IMAP integration requires
        // more complex async library integration
        warn!("Email receiving not yet fully implemented - use email_unified.rs for receiving");
        Ok(Vec::new())
    }

    async fn test_connectivity(&self, target: &TransportTarget) -> Result<ConnectivityResult> {
        if !self.can_reach(target).await {
            return Ok(ConnectivityResult {
                connected: false,
                rtt: None,
                error: Some("Invalid email address".to_string()),
                quality: 0.0,
                details: HashMap::new(),
            });
        }

        // Test SMTP connectivity
        let start_time = Instant::now();

        match self.test_smtp_connection().await {
            Ok(()) => {
                let rtt = start_time.elapsed();
                Ok(ConnectivityResult {
                    connected: true,
                    rtt: Some(rtt),
                    error: None,
                    quality: 0.95,
                    details: {
                        let mut details = HashMap::new();
                        details.insert("transport".to_string(), "email".to_string());
                        details.insert("smtp_host".to_string(), self.config.smtp.host.clone());
                        details
                    },
                })
            }
            Err(e) => Ok(ConnectivityResult {
                connected: false,
                rtt: Some(start_time.elapsed()),
                error: Some(e.to_string()),
                quality: 0.0,
                details: HashMap::new(),
            }),
        }
    }

    async fn start(&self) -> Result<()> {
        let mut status = self.status.write().await;
        *status = TransportStatus::Running;
        info!("Email transport started");
        Ok(())
    }

    async fn stop(&self) -> Result<()> {
        let mut status = self.status.write().await;
        *status = TransportStatus::Stopped;
        info!("Email transport stopped");
        Ok(())
    }

    async fn status(&self) -> TransportStatus {
        *self.status.read().await
    }

    async fn metrics(&self) -> TransportMetrics {
        self.metrics.read().await.clone()
    }
}
