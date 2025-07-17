//! Enhanced Metrics and Monitoring System for Synapse
//! 
//! Provides comprehensive monitoring, metrics collection, and health
//! diagnostics for all transport layers and system components.

use crate::{
    error::Result,
    transport::abstraction::TransportMetrics,
};
use std::{
    sync::Arc,
    time::{Duration, Instant, SystemTime},
    collections::{HashMap, VecDeque},
};
use tokio::sync::{RwLock, broadcast};
use serde::{Serialize, Deserialize};
use tracing::{info, debug};

/// Central metrics collection and monitoring system
#[derive(Debug)]
pub struct MetricsCollector {
    transport_metrics: Arc<RwLock<HashMap<String, TransportMetrics>>>,
    system_metrics: Arc<RwLock<SystemMetrics>>,
    performance_history: Arc<RwLock<PerformanceHistory>>,
    event_broadcaster: broadcast::Sender<MetricEvent>,
    alerts: Arc<RwLock<Vec<Alert>>>,
    config: MetricsConfig,
}

/// System-wide performance metrics
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SystemMetrics {
    pub uptime: Duration,
    pub cpu_usage_percent: f64,
    pub memory_usage_bytes: u64,
    pub network_bandwidth_in: u64,
    pub network_bandwidth_out: u64,
    pub active_connections: u32,
    pub message_throughput_per_second: f64,
    pub average_response_time: Duration,
    pub error_rate_percent: f64,
    pub last_updated: SystemTime,
}

/// Historical performance data for trend analysis
#[derive(Debug, Clone)]
pub struct PerformanceHistory {
    pub message_rates: VecDeque<(SystemTime, f64)>,
    pub latency_samples: VecDeque<(SystemTime, Duration)>,
    pub error_counts: VecDeque<(SystemTime, u32)>,
    pub bandwidth_samples: VecDeque<(SystemTime, u64, u64)>, // (time, in, out)
    pub connection_counts: VecDeque<(SystemTime, u32)>,
    pub max_samples: usize,
}

/// Metric events for real-time monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum MetricEvent {
    MessageSent { transport: String, target: String, size: usize, latency: Duration },
    MessageReceived { transport: String, source: String, size: usize },
    ConnectionEstablished { transport: String, peer: String },
    ConnectionLost { transport: String, peer: String, reason: String },
    TransportError { transport: String, error: String },
    PerformanceAlert { alert_type: AlertType, severity: AlertSeverity, message: String },
    SystemResourceAlert { resource: String, usage_percent: f64, threshold: f64 },
}

/// Alert types for automated monitoring
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum AlertType {
    HighLatency,
    HighErrorRate,
    ResourceExhaustion,
    ConnectionFailure,
    CircuitBreakerOpen,
    SecurityEvent,
    PerformanceDegradation,
}

/// Alert severity levels
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq, PartialOrd, Ord)]
pub enum AlertSeverity {
    Info,
    Warning,
    Critical,
    Emergency,
}

/// Individual alert instance
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Alert {
    pub id: String,
    pub alert_type: AlertType,
    pub severity: AlertSeverity,
    pub message: String,
    pub timestamp: SystemTime,
    pub transport: Option<String>,
    pub resolved: bool,
    pub resolution_time: Option<SystemTime>,
}

/// Configuration for metrics collection
#[derive(Debug, Clone)]
pub struct MetricsConfig {
    pub collection_interval: Duration,
    pub history_retention: Duration,
    pub max_history_samples: usize,
    pub latency_alert_threshold: Duration,
    pub error_rate_alert_threshold: f64,
    pub cpu_alert_threshold: f64,
    pub memory_alert_threshold: u64,
    pub enable_real_time_events: bool,
    pub enable_performance_profiling: bool,
}

impl Default for MetricsConfig {
    fn default() -> Self {
        Self {
            collection_interval: Duration::from_secs(10),
            history_retention: Duration::from_secs(24 * 3600), // 24 hours
            max_history_samples: 8640, // 24 hours at 10s intervals
            latency_alert_threshold: Duration::from_millis(1000),
            error_rate_alert_threshold: 5.0, // 5%
            cpu_alert_threshold: 80.0, // 80%
            memory_alert_threshold: 1024 * 1024 * 1024, // 1GB
            enable_real_time_events: true,
            enable_performance_profiling: true,
        }
    }
}

impl MetricsCollector {
    /// Create a new metrics collector
    pub fn new(config: MetricsConfig) -> Self {
        let (event_broadcaster, _) = broadcast::channel(1000);
        
        Self {
            transport_metrics: Arc::new(RwLock::new(HashMap::new())),
            system_metrics: Arc::new(RwLock::new(SystemMetrics::default())),
            performance_history: Arc::new(RwLock::new(PerformanceHistory::new(config.max_history_samples))),
            event_broadcaster,
            alerts: Arc::new(RwLock::new(Vec::new())),
            config,
        }
    }
    
    /// Start the metrics collection system
    pub async fn start(&self) -> Result<()> {
        info!("Starting metrics collection system");
        
        let _transport_metrics = self.transport_metrics.clone();
        let system_metrics = self.system_metrics.clone();
        let performance_history = self.performance_history.clone();
        let event_broadcaster = self.event_broadcaster.clone();
        let alerts = self.alerts.clone();
        let config = self.config.clone();
        
        // Spawn metrics collection task
        tokio::spawn(async move {
            let mut interval = tokio::time::interval(config.collection_interval);
            let start_time = Instant::now();
            
            loop {
                interval.tick().await;
                
                // Collect system metrics
                let current_metrics = Self::collect_system_metrics(start_time).await;
                
                // Update system metrics
                {
                    let mut metrics = system_metrics.write().await;
                    *metrics = current_metrics.clone();
                }
                
                // Update performance history
                {
                    let mut history = performance_history.write().await;
                    history.add_sample(current_metrics.clone());
                }
                
                // Check for alerts
                Self::check_alerts(&current_metrics, &alerts, &event_broadcaster, &config).await;
                
                debug!("Collected metrics: {:?}", current_metrics);
            }
        });
        
        info!("Metrics collection system started");
        Ok(())
    }
    
    /// Record transport metrics
    pub async fn record_transport_metrics(&self, transport_name: &str, metrics: TransportMetrics) {
        let mut transport_metrics = self.transport_metrics.write().await;
        transport_metrics.insert(transport_name.to_string(), metrics.clone());
        
        // Emit event if enabled
        if self.config.enable_real_time_events {
            let latency = Duration::from_millis(metrics.average_latency_ms);
            let _ = self.event_broadcaster.send(MetricEvent::MessageSent {
                transport: transport_name.to_string(),
                target: "unknown".to_string(), // Would be filled by caller
                size: metrics.bytes_sent as usize,
                latency,
            });
        }
    }
    
    /// Record a message event
    pub async fn record_message_event(&self, transport: &str, event_type: &str, size: usize, latency: Option<Duration>) {
        if self.config.enable_real_time_events {
            let event = match event_type {
                "sent" => MetricEvent::MessageSent {
                    transport: transport.to_string(),
                    target: "unknown".to_string(),
                    size,
                    latency: latency.unwrap_or_default(),
                },
                "received" => MetricEvent::MessageReceived {
                    transport: transport.to_string(),
                    source: "unknown".to_string(),
                    size,
                },
                _ => return,
            };
            
            let _ = self.event_broadcaster.send(event);
        }
    }
    
    /// Subscribe to metric events
    pub fn subscribe_to_events(&self) -> broadcast::Receiver<MetricEvent> {
        self.event_broadcaster.subscribe()
    }
    
    /// Get current system metrics
    pub async fn get_system_metrics(&self) -> SystemMetrics {
        self.system_metrics.read().await.clone()
    }
    
    /// Get transport metrics for all transports
    pub async fn get_transport_metrics(&self) -> HashMap<String, TransportMetrics> {
        self.transport_metrics.read().await.clone()
    }
    
    /// Get performance history
    pub async fn get_performance_history(&self) -> PerformanceHistory {
        self.performance_history.read().await.clone()
    }
    
    /// Get active alerts
    pub async fn get_active_alerts(&self) -> Vec<Alert> {
        let alerts = self.alerts.read().await;
        alerts.iter().filter(|a| !a.resolved).cloned().collect()
    }
    
    /// Generate a comprehensive health report
    pub async fn generate_health_report(&self) -> HealthReport {
        let system_metrics = self.get_system_metrics().await;
        let transport_metrics = self.get_transport_metrics().await;
        let active_alerts = self.get_active_alerts().await;
        let performance_history = self.get_performance_history().await;
        
        let overall_health = Self::calculate_health_score(&system_metrics, &transport_metrics, &active_alerts);
        
        HealthReport {
            overall_health_score: overall_health,
            system_metrics: system_metrics.clone(),
            transport_metrics: transport_metrics.clone(),
            active_alerts: active_alerts.clone(),
            performance_trends: Self::analyze_performance_trends(&performance_history),
            recommendations: Self::generate_recommendations(&system_metrics, &transport_metrics, &active_alerts),
            generated_at: SystemTime::now(),
        }
    }
    
    /// Collect current system metrics
    async fn collect_system_metrics(start_time: Instant) -> SystemMetrics {
        // This would integrate with system monitoring libraries
        // For now, providing mock data structure
        SystemMetrics {
            uptime: start_time.elapsed(),
            cpu_usage_percent: 45.2, // Would use sysinfo crate
            memory_usage_bytes: 512 * 1024 * 1024, // Would use sysinfo crate
            network_bandwidth_in: 1024 * 1024, // Would use network monitoring
            network_bandwidth_out: 2048 * 1024,
            active_connections: 12,
            message_throughput_per_second: 150.5,
            average_response_time: Duration::from_millis(45),
            error_rate_percent: 1.2,
            last_updated: SystemTime::now(),
        }
    }
    
    /// Check for alert conditions
    async fn check_alerts(
        metrics: &SystemMetrics,
        alerts: &Arc<RwLock<Vec<Alert>>>,
        broadcaster: &broadcast::Sender<MetricEvent>,
        config: &MetricsConfig,
    ) {
        let mut new_alerts = Vec::new();
        
        // Check latency alert
        if metrics.average_response_time > config.latency_alert_threshold {
            new_alerts.push(Alert {
                id: format!("latency-{}", SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis()),
                alert_type: AlertType::HighLatency,
                severity: AlertSeverity::Warning,
                message: format!("High latency detected: {:?}", metrics.average_response_time),
                timestamp: SystemTime::now(),
                transport: None,
                resolved: false,
                resolution_time: None,
            });
        }
        
        // Check error rate alert
        if metrics.error_rate_percent > config.error_rate_alert_threshold {
            new_alerts.push(Alert {
                id: format!("error-{}", SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis()),
                alert_type: AlertType::HighErrorRate,
                severity: AlertSeverity::Critical,
                message: format!("High error rate: {:.1}%", metrics.error_rate_percent),
                timestamp: SystemTime::now(),
                transport: None,
                resolved: false,
                resolution_time: None,
            });
        }
        
        // Check CPU alert
        if metrics.cpu_usage_percent > config.cpu_alert_threshold {
            new_alerts.push(Alert {
                id: format!("cpu-{}", SystemTime::now().duration_since(SystemTime::UNIX_EPOCH).unwrap().as_millis()),
                alert_type: AlertType::ResourceExhaustion,
                severity: AlertSeverity::Warning,
                message: format!("High CPU usage: {:.1}%", metrics.cpu_usage_percent),
                timestamp: SystemTime::now(),
                transport: None,
                resolved: false,
                resolution_time: None,
            });
        }
        
        // Add new alerts and broadcast them
        if !new_alerts.is_empty() {
            let mut alerts_vec = alerts.write().await;
            for alert in new_alerts {
                // Broadcast alert event
                let _ = broadcaster.send(MetricEvent::PerformanceAlert {
                    alert_type: alert.alert_type.clone(),
                    severity: alert.severity.clone(),
                    message: alert.message.clone(),
                });
                
                alerts_vec.push(alert);
            }
        }
    }
    
    /// Calculate overall health score (0-100)
    fn calculate_health_score(
        system_metrics: &SystemMetrics,
        _transport_metrics: &HashMap<String, TransportMetrics>,
        active_alerts: &[Alert],
    ) -> f64 {
        let mut score = 100.0;
        
        // Deduct for high error rate
        if system_metrics.error_rate_percent > 1.0 {
            score -= system_metrics.error_rate_percent * 5.0;
        }
        
        // Deduct for high latency
        if system_metrics.average_response_time > Duration::from_millis(100) {
            let latency_ms = system_metrics.average_response_time.as_millis() as f64;
            score -= (latency_ms - 100.0) / 10.0;
        }
        
        // Deduct for resource usage
        if system_metrics.cpu_usage_percent > 70.0 {
            score -= (system_metrics.cpu_usage_percent - 70.0) / 2.0;
        }
        
        // Deduct for active alerts
        for alert in active_alerts {
            match alert.severity {
                AlertSeverity::Emergency => score -= 25.0,
                AlertSeverity::Critical => score -= 15.0,
                AlertSeverity::Warning => score -= 5.0,
                AlertSeverity::Info => score -= 1.0,
            }
        }
        
        score.max(0.0).min(100.0)
    }
    
    /// Analyze performance trends
    fn analyze_performance_trends(history: &PerformanceHistory) -> Vec<String> {
        let mut trends = Vec::new();
        
        if history.latency_samples.len() > 10 {
            let recent_avg = history.latency_samples.iter().rev().take(5)
                .map(|(_, d)| d.as_millis() as f64)
                .sum::<f64>() / 5.0;
            let older_avg = history.latency_samples.iter().rev().skip(5).take(5)
                .map(|(_, d)| d.as_millis() as f64)
                .sum::<f64>() / 5.0;
            
            if recent_avg > older_avg * 1.2 {
                trends.push("Latency is increasing".to_string());
            } else if recent_avg < older_avg * 0.8 {
                trends.push("Latency is improving".to_string());
            }
        }
        
        if history.message_rates.len() > 10 {
            let recent_avg = history.message_rates.iter().rev().take(5)
                .map(|(_, r)| *r)
                .sum::<f64>() / 5.0;
            let older_avg = history.message_rates.iter().rev().skip(5).take(5)
                .map(|(_, r)| *r)
                .sum::<f64>() / 5.0;
            
            if recent_avg > older_avg * 1.2 {
                trends.push("Message throughput is increasing".to_string());
            } else if recent_avg < older_avg * 0.8 {
                trends.push("Message throughput is decreasing".to_string());
            }
        }
        
        trends
    }
    
    /// Generate recommendations based on current metrics
    fn generate_recommendations(
        system_metrics: &SystemMetrics,
        transport_metrics: &HashMap<String, TransportMetrics>,
        active_alerts: &[Alert],
    ) -> Vec<String> {
        let mut recommendations = Vec::new();
        
        if system_metrics.cpu_usage_percent > 80.0 {
            recommendations.push("Consider scaling up CPU resources or optimizing message processing".to_string());
        }
        
        if system_metrics.error_rate_percent > 5.0 {
            recommendations.push("High error rate detected - check transport reliability and network conditions".to_string());
        }
        
        if system_metrics.average_response_time > Duration::from_millis(500) {
            recommendations.push("Consider enabling UDP transport for low-latency scenarios".to_string());
        }
        
        if active_alerts.iter().any(|a| matches!(a.alert_type, AlertType::CircuitBreakerOpen)) {
            recommendations.push("Circuit breakers are open - check network connectivity and target availability".to_string());
        }
        
        let total_failed = transport_metrics.values().map(|m| m.send_failures).sum::<u64>();
        let total_sent = transport_metrics.values().map(|m| m.messages_sent).sum::<u64>();
        if total_sent > 0 && (total_failed as f64 / total_sent as f64) > 0.1 {
            recommendations.push("High transport failure rate - consider enabling additional transport redundancy".to_string());
        }
        
        recommendations
    }
}

impl PerformanceHistory {
    fn new(max_samples: usize) -> Self {
        Self {
            message_rates: VecDeque::with_capacity(max_samples),
            latency_samples: VecDeque::with_capacity(max_samples),
            error_counts: VecDeque::with_capacity(max_samples),
            bandwidth_samples: VecDeque::with_capacity(max_samples),
            connection_counts: VecDeque::with_capacity(max_samples),
            max_samples,
        }
    }
    
    fn add_sample(&mut self, metrics: SystemMetrics) {
        let timestamp = metrics.last_updated;
        
        // Add samples and maintain size limit
        self.message_rates.push_back((timestamp, metrics.message_throughput_per_second));
        if self.message_rates.len() > self.max_samples {
            self.message_rates.pop_front();
        }
        
        self.latency_samples.push_back((timestamp, metrics.average_response_time));
        if self.latency_samples.len() > self.max_samples {
            self.latency_samples.pop_front();
        }
        
        self.bandwidth_samples.push_back((timestamp, metrics.network_bandwidth_in, metrics.network_bandwidth_out));
        if self.bandwidth_samples.len() > self.max_samples {
            self.bandwidth_samples.pop_front();
        }
        
        self.connection_counts.push_back((timestamp, metrics.active_connections));
        if self.connection_counts.len() > self.max_samples {
            self.connection_counts.pop_front();
        }
    }
}

impl Default for SystemMetrics {
    fn default() -> Self {
        Self {
            uptime: Duration::from_secs(0),
            cpu_usage_percent: 0.0,
            memory_usage_bytes: 0,
            network_bandwidth_in: 0,
            network_bandwidth_out: 0,
            active_connections: 0,
            message_throughput_per_second: 0.0,
            average_response_time: Duration::from_millis(0),
            error_rate_percent: 0.0,
            last_updated: SystemTime::now(),
        }
    }
}

/// Comprehensive health report
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct HealthReport {
    pub overall_health_score: f64,
    pub system_metrics: SystemMetrics,
    pub transport_metrics: HashMap<String, TransportMetrics>,
    pub active_alerts: Vec<Alert>,
    pub performance_trends: Vec<String>,
    pub recommendations: Vec<String>,
    pub generated_at: SystemTime,
}
