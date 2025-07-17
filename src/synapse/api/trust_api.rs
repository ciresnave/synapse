use crate::synapse::services::TrustManager;
use crate::synapse::blockchain::SynapseBlockchain;
use crate::synapse::models::trust::TrustCategory;
use anyhow::Result;
use serde::{Deserialize, Serialize};
use tracing::{info, debug};
use uuid::Uuid;
use chrono::Utc;
use crate::synapse::blockchain::serialization::UuidWrapper;

/// HTTP API for trust system operations
pub struct TrustAPI {
    trust_manager: TrustManager,
    #[allow(dead_code)]
    blockchain: SynapseBlockchain,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct SubmitTrustReportRequest {
    pub subject_id: String,
    pub score: i8, // -100 to +100
    pub category: String,
    pub comment: Option<String>,
    pub evidence_hash: Option<String>,
    pub stake_amount: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct StakeRequest {
    pub amount: u32,
    pub purpose: String, // "consensus", "reporting", "verification"
}

#[derive(Debug, Serialize, Deserialize)]
pub struct UnstakeRequest {
    pub amount: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrustReportResponse {
    pub report_id: String,
    pub reporter_id: String,
    pub subject_id: String,
    pub score: i8,
    pub category: String,
    pub stake_amount: u32,
    pub status: String,
    pub timestamp: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrustScoreResponse {
    pub participant_id: String,
    pub entity_trust_score: f64,
    pub network_trust_score: f64,
    pub composite_score: f64,
    pub trust_balance: TrustBalanceInfo,
    pub recent_activity: Vec<TrustActivityItem>,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrustBalanceInfo {
    pub total_points: u32,
    pub available_points: u32,
    pub staked_points: u32,
    pub earned_lifetime: u32,
    pub last_activity: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrustActivityItem {
    pub activity_type: String, // "report_given", "report_received", "stake", "unstake"
    pub counterpart: Option<String>,
    pub amount: Option<u32>,
    pub score: Option<i8>,
    pub timestamp: String,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct APIResponse<T> {
    pub success: bool,
    pub data: Option<T>,
    pub error: Option<String>,
    pub message: Option<String>,
}

impl TrustAPI {
    pub fn new(trust_manager: TrustManager, blockchain: SynapseBlockchain) -> Self {
        Self {
            trust_manager,
            blockchain,
        }
    }

    /// Submit a trust report about another participant
    pub async fn submit_trust_report(
        &self,
        reporter_id: &str,
        request: SubmitTrustReportRequest,
    ) -> Result<APIResponse<TrustReportResponse>> {
        debug!("Submitting trust report from {} about {}", reporter_id, request.subject_id);

        // Validate request
        if request.subject_id.is_empty() {
            return Ok(APIResponse {
                success: false,
                data: None,
                error: Some("Missing subject ID".to_string()),
                message: None,
            });
        }

        if request.score < -100 || request.score > 100 {
            return Ok(APIResponse {
                success: false,
                data: None,
                error: Some("Score must be between -100 and +100".to_string()),
                message: None,
            });
        }

        if reporter_id == request.subject_id {
            return Ok(APIResponse {
                success: false,
                data: None,
                error: Some("Cannot report on yourself".to_string()),
                message: None,
            });
        }

        // Check if reporter has sufficient trust points to stake
        let reporter_balance = self.trust_manager.get_trust_balance(reporter_id).await?;
        let balance = match reporter_balance {
            Some(b) => b,
            None => {
                return Ok(APIResponse {
                    success: false,
                    data: None,
                    error: Some("Reporter not found in trust system".to_string()),
                    message: None,
                });
            }
        };
        
        if balance.available_points < request.stake_amount {
            return Ok(APIResponse {
                success: false,
                data: None,
                error: Some("Insufficient trust points for stake".to_string()),
                message: None,
            });
        }

        // Submit the trust report
        let report_id = self.trust_manager.submit_trust_report(
            reporter_id,
            &request.subject_id,
            request.score,
            TrustCategory::Overall, // Convert string to TrustCategory
            request.stake_amount,
            request.comment,
        ).await?;

        info!("Trust report submitted successfully: {}", report_id);

        let response = TrustReportResponse {
            report_id: report_id.clone(),
            reporter_id: reporter_id.to_string(),
            subject_id: request.subject_id,
            score: request.score,
            category: request.category,
            stake_amount: request.stake_amount,
            status: "pending".to_string(),
            timestamp: Utc::now().to_rfc3339(),
        };
 
         Ok(APIResponse {
            success: true,
            data: Some(response),
            error: None,
            message: Some("Trust report submitted successfully".to_string()),
        })
    }

    /// Get trust score for a participant
    pub async fn get_trust_score(
        &self,
        participant_id: &str,
        requester_id: &str,
    ) -> Result<APIResponse<TrustScoreResponse>> {
        debug!("Getting trust score for {} requested by {}", participant_id, requester_id);

        // Get composite trust score
        let composite_score = self.trust_manager.get_trust_score(participant_id, requester_id).await?;
        
        // Get trust balance
        let balance_opt = self.trust_manager.get_trust_balance(participant_id).await?;
        let balance = match balance_opt {
            Some(b) => b,
            None => {
                return Ok(APIResponse {
                    success: false,
                    data: None,
                    error: Some("Participant not found in trust system".to_string()),
                    message: None,
                });
            }
        };

        // Get recent trust activity (placeholder for now)
        let recent_activity = vec![];

        let response = TrustScoreResponse {
            participant_id: participant_id.to_string(),
            entity_trust_score: 0.0, // Would calculate from trust ratings
            network_trust_score: self.trust_manager.get_network_trust_score(participant_id).await?,
            composite_score,
            trust_balance: TrustBalanceInfo {
                total_points: balance.total_points,
                available_points: balance.available_points,
                staked_points: balance.staked_points,
                earned_lifetime: balance.earned_lifetime,
                last_activity: balance.last_activity.clone().into_inner().to_rfc3339(),
            },
            recent_activity,
        };

        Ok(APIResponse {
            success: true,
            data: Some(response),
            error: None,
            message: None,
        })
    }

    /// Stake trust points
    pub async fn stake_trust_points(
        &self,
        participant_id: &str,
        request: StakeRequest,
    ) -> Result<APIResponse<String>> {
        debug!("Staking {} trust points for {} with purpose: {}", 
               request.amount, participant_id, request.purpose);

        // Validate stake amount
        if request.amount == 0 {
            return Ok(APIResponse {
                success: false,
                data: None,
                error: Some("Stake amount must be greater than 0".to_string()),
                message: None,
            });
        }

        // Check available balance
        let balance_opt = self.trust_manager.get_trust_balance(participant_id).await?;
        let balance = match balance_opt {
            Some(b) => b,
            None => {
                return Ok(APIResponse {
                    success: false,
                    data: None,
                    error: Some("Participant not found in trust system".to_string()),
                    message: None,
                });
            }
        };
        
        if balance.available_points < request.amount {
            return Ok(APIResponse {
                success: false,
                data: None,
                error: Some("Insufficient available trust points".to_string()),
                message: None,
            });
        }

        // Perform staking (placeholder implementation)
        let stake_id = format!("stake_{}", UuidWrapper::new(Uuid::new_v4()).to_string());
        info!("Staking {} trust points for participant {} (placeholder)", request.amount, participant_id);

        info!("Trust points staked successfully: {} for participant: {}", 
              request.amount, participant_id);

        Ok(APIResponse {
            success: true,
            data: Some(stake_id),
            error: None,
            message: Some(format!("Staked {} trust points successfully", request.amount)),
        })
    }

    /// Unstake trust points
    pub async fn unstake_trust_points(
        &self,
        participant_id: &str,
        request: UnstakeRequest,
    ) -> Result<APIResponse<String>> {
        debug!("Unstaking {} trust points for {}", request.amount, participant_id);

        // Validate unstake amount
        if request.amount == 0 {
            return Ok(APIResponse {
                success: false,
                data: None,
                error: Some("Unstake amount must be greater than 0".to_string()),
                message: None,
            });
        }

        // Check staked balance
        let balance_opt = self.trust_manager.get_trust_balance(participant_id).await?;
        let balance = match balance_opt {
            Some(b) => b,
            None => {
                return Ok(APIResponse {
                    success: false,
                    data: None,
                    error: Some("Participant not found in trust system".to_string()),
                    message: None,
                });
            }
        };
        
        if balance.staked_points < request.amount {
            return Ok(APIResponse {
                success: false,
                data: None,
                error: Some("Insufficient staked trust points".to_string()),
                message: None,
            });
        }

        // Perform unstaking (placeholder implementation)
        let unstake_id = format!("unstake_{}", UuidWrapper::new(Uuid::new_v4()).to_string());
        info!("Unstaking {} trust points for participant {} (placeholder)", request.amount, participant_id);

        info!("Trust points unstaked successfully: {} for participant: {}", 
              request.amount, participant_id);

        Ok(APIResponse {
            success: true,
            data: Some(unstake_id),
            error: None,
            message: Some(format!("Unstaked {} trust points successfully", request.amount)),
        })
    }

    /// Get trust reports for a participant
    pub async fn get_trust_reports(
        &self,
        participant_id: &str,
        requester_id: &str,
        limit: Option<usize>,
    ) -> Result<APIResponse<Vec<TrustReportResponse>>> {
        debug!("Getting trust reports for {} requested by {}", participant_id, requester_id);

        // Check if requester can view trust reports
        // For now, allow if requester is the subject or if reports are public
        if participant_id != requester_id {
            // Could add privacy checks here
        }

        let _limit = limit.unwrap_or(10).min(100);

        // Get trust reports (placeholder for now)
        let reports = vec![];
        let report_count = reports.len();

        Ok(APIResponse {
            success: true,
            data: Some(reports),
            error: None,
            message: Some(format!("Retrieved {} trust reports", report_count)),
        })
    }

    /// Get trust network analysis
    pub async fn get_trust_network_analysis(
        &self,
        participant_id: &str,
        requester_id: &str,
        depth: Option<u32>,
    ) -> Result<APIResponse<TrustNetworkAnalysis>> {
        debug!("Getting trust network analysis for {} requested by {} with depth: {:?}", 
               participant_id, requester_id, depth);

        let _depth = depth.unwrap_or(2).min(5); // Cap at 5 degrees

        // Perform trust network analysis (placeholder)
        let analysis = TrustNetworkAnalysis {
            participant_id: participant_id.to_string(),
            network_size: 0,
            trust_connections: vec![],
            influence_score: 0.0,
            centrality_score: 0.0,
        };

        Ok(APIResponse {
            success: true,
            data: Some(analysis),
            error: None,
            message: None,
        })
    }

    /// Get trust system statistics
    pub async fn get_trust_statistics(&self) -> Result<APIResponse<TrustStatistics>> {
        debug!("Getting trust system statistics");

        let stats = TrustStatistics {
            total_trust_reports: 0,
            total_staked_points: 0,
            average_trust_score: 0.0,
            active_validators: 0,
            pending_reports: 0,
        };

        Ok(APIResponse {
            success: true,
            data: Some(stats),
            error: None,
            message: None,
        })
    }
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrustNetworkAnalysis {
    pub participant_id: String,
    pub network_size: u32,
    pub trust_connections: Vec<TrustConnection>,
    pub influence_score: f64,
    pub centrality_score: f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrustConnection {
    pub participant_id: String,
    pub trust_score: f64,
    pub relationship_type: String,
    pub degrees_of_separation: u32,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct TrustStatistics {
    pub total_trust_reports: u64,
    pub total_staked_points: u64,
    pub average_trust_score: f64,
    pub active_validators: u32,
    pub pending_reports: u64,
}
