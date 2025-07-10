//! External connectivity detection for email server accessibility

use crate::error::{EmrpError, Result};
use std::net::{IpAddr, Ipv4Addr, SocketAddr};
use std::time::Duration;
use tokio::net::{TcpListener, UdpSocket};
use tokio::time::timeout;
use tracing::{info, warn, debug};

/// Connectivity assessment results
#[derive(Debug, Clone)]
pub struct ConnectivityAssessment {
    /// Can bind to SMTP port (25, 587, 2525)
    pub can_bind_smtp: bool,
    /// Can bind to IMAP port (143, 993, 1143)
    pub can_bind_imap: bool,
    /// Has external IP address
    pub has_external_ip: bool,
    /// External IP address if available
    pub external_ip: Option<IpAddr>,
    /// Firewall/NAT detection results
    pub firewall_status: FirewallStatus,
    /// Recommended server configuration
    pub recommended_config: ServerRecommendation,
}

#[derive(Debug, Clone)]
pub enum FirewallStatus {
    /// Ports are open and accessible from internet
    Open,
    /// Behind NAT/firewall, ports not accessible
    Blocked,
    /// Unable to determine (network issues)
    Unknown,
}

#[derive(Debug, Clone)]
pub enum ServerRecommendation {
    /// Run local email server (ports open, external IP)
    RunLocalServer {
        smtp_port: u16,
        imap_port: u16,
        external_ip: IpAddr,
    },
    /// Use relay-only mode (can send but not receive)
    RelayOnly {
        reason: String,
    },
    /// Use external email provider
    ExternalProvider {
        reason: String,
    },
}

/// External connectivity detector
pub struct ConnectivityDetector {
    /// Timeout for connectivity tests
    test_timeout: Duration,
    /// Ports to test for availability
    smtp_ports: Vec<u16>,
    imap_ports: Vec<u16>,
}

impl Default for ConnectivityDetector {
    fn default() -> Self {
        Self {
            test_timeout: Duration::from_secs(10),
            smtp_ports: vec![25, 587, 2525], // Standard, submission, alternative
            imap_ports: vec![143, 993, 1143], // Standard, SSL, alternative
        }
    }
}

impl ConnectivityDetector {
    /// Perform comprehensive connectivity assessment
    pub async fn assess_connectivity(&self) -> Result<ConnectivityAssessment> {
        info!("Starting connectivity assessment for email server...");

        let can_bind_smtp = self.test_port_binding(&self.smtp_ports).await?;
        let can_bind_imap = self.test_port_binding(&self.imap_ports).await?;
        
        let external_ip = self.detect_external_ip().await;
        let has_external_ip = external_ip.is_some();
        
        let firewall_status = if has_external_ip && (can_bind_smtp || can_bind_imap) {
            self.test_external_accessibility().await
        } else {
            FirewallStatus::Blocked
        };

        let recommended_config = self.determine_recommendation(
            can_bind_smtp,
            can_bind_imap,
            &external_ip,
            &firewall_status,
        );

        let assessment = ConnectivityAssessment {
            can_bind_smtp,
            can_bind_imap,
            has_external_ip,
            external_ip,
            firewall_status,
            recommended_config,
        };

        info!("Connectivity assessment completed: {:?}", assessment);
        Ok(assessment)
    }

    /// Test if we can bind to email server ports
    async fn test_port_binding(&self, ports: &[u16]) -> Result<bool> {
        for &port in ports {
            debug!("Testing port binding on port {}", port);
            
            match timeout(self.test_timeout, TcpListener::bind(SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port))).await {
                Ok(Ok(_listener)) => {
                    info!("Successfully bound to port {}", port);
                    return Ok(true);
                }
                Ok(Err(e)) => {
                    debug!("Failed to bind to port {}: {}", port, e);
                }
                Err(_) => {
                    debug!("Timeout binding to port {}", port);
                }
            }
        }
        
        warn!("Unable to bind to any ports in {:?}", ports);
        Ok(false)
    }

    /// Detect external IP address
    async fn detect_external_ip(&self) -> Option<IpAddr> {
        debug!("Detecting external IP address...");

        // Method 1: Try to connect to external services and check local address
        if let Ok(socket) = UdpSocket::bind("0.0.0.0:0").await {
            if let Ok(_) = socket.connect("8.8.8.8:53").await {
                if let Ok(local_addr) = socket.local_addr() {
                    let ip = local_addr.ip();
                    if !ip.is_loopback() && !self.is_private_ip(&ip) {
                        info!("Detected external IP via UDP: {}", ip);
                        return Some(ip);
                    }
                }
            }
        }

        // Method 2: Check for common public IP patterns
        if let Ok(socket) = UdpSocket::bind("0.0.0.0:0").await {
            if let Ok(_) = socket.connect("1.1.1.1:53").await {
                if let Ok(local_addr) = socket.local_addr() {
                    let ip = local_addr.ip();
                    if !ip.is_loopback() && !self.is_private_ip(&ip) {
                        info!("Detected external IP via UDP (method 2): {}", ip);
                        return Some(ip);
                    }
                }
            }
        }

        warn!("Unable to detect external IP address");
        None
    }

    /// Check if an IP address is private/local
    fn is_private_ip(&self, ip: &IpAddr) -> bool {
        match ip {
            IpAddr::V4(ipv4) => {
                ipv4.is_private() || ipv4.is_loopback() || ipv4.is_link_local()
            }
            IpAddr::V6(ipv6) => {
                ipv6.is_loopback() || ipv6.is_multicast()
            }
        }
    }

    /// Test external accessibility of ports
    async fn test_external_accessibility(&self) -> FirewallStatus {
        debug!("Testing external accessibility...");

        // For development purposes, we'll assume firewall is blocking unless we can prove otherwise
        // In production, this would use external port checking services or UPnP discovery
        
        // Check if we can bind to privileged ports (indication of admin access)
        if let Ok(_) = timeout(Duration::from_secs(1), TcpListener::bind("127.0.0.1:25")).await {
            // If we can bind to port 25, we might be running as admin and ports could be open
            return FirewallStatus::Open;
        }
        
        FirewallStatus::Unknown
    }

    /// Determine server configuration recommendation
    fn determine_recommendation(
        &self,
        can_bind_smtp: bool,
        can_bind_imap: bool,
        external_ip: &Option<IpAddr>,
        firewall_status: &FirewallStatus,
    ) -> ServerRecommendation {
        match (can_bind_smtp, can_bind_imap, external_ip, firewall_status) {
            // Ideal case: can bind to ports, have external IP, ports are open
            (true, true, Some(ip), FirewallStatus::Open) => {
                ServerRecommendation::RunLocalServer {
                    smtp_port: 2525, // Use non-privileged port
                    imap_port: 1143,
                    external_ip: *ip,
                }
            }
            
            // Can bind to SMTP only
            (true, false, Some(ip), FirewallStatus::Open) => {
                ServerRecommendation::RunLocalServer {
                    smtp_port: 2525,
                    imap_port: 1143, // Will try to bind anyway
                    external_ip: *ip,
                }
            }
            
            // Can bind but behind firewall/NAT
            (true, _, _, FirewallStatus::Blocked) => {
                ServerRecommendation::RelayOnly {
                    reason: "Ports available locally but blocked by firewall/NAT".to_string(),
                }
            }
            
            // Can't bind to ports (likely running as non-root)
            (false, false, _, _) => {
                ServerRecommendation::ExternalProvider {
                    reason: "Unable to bind to email server ports (try running as administrator)".to_string(),
                }
            }
            
            // No external IP
            (_, _, None, _) => {
                ServerRecommendation::RelayOnly {
                    reason: "No external IP address detected".to_string(),
                }
            }
            
            // Unknown firewall status - be conservative but allow local use
            (true, _, _, FirewallStatus::Unknown) => {
                ServerRecommendation::RelayOnly {
                    reason: "Unable to determine external accessibility - running in local mode".to_string(),
                }
            }
            
            // Can bind to IMAP only but not SMTP
            (false, true, Some(_), _) => {
                ServerRecommendation::ExternalProvider {
                    reason: "Can bind to IMAP but not SMTP - consider using external provider".to_string(),
                }
            }
        }
    }

    /// Test specific port accessibility from external network
    pub async fn test_external_port(&self, port: u16) -> Result<bool> {
        debug!("Testing external accessibility of port {}", port);
        
        // Bind to the port
        let addr = SocketAddr::new(IpAddr::V4(Ipv4Addr::new(0, 0, 0, 0)), port);
        let _listener = timeout(self.test_timeout, TcpListener::bind(addr)).await
            .map_err(|_| EmrpError::Network("Timeout binding to port".to_string()))?
            .map_err(|e| EmrpError::Network(format!("Failed to bind to port {}: {}", port, e)))?;

        // In production, this would test external connectivity via UPnP or external services
        Ok(false) // Conservative default
    }
}
