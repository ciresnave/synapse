// Manual impls for RelationshipType (as discriminant)
use crate::models::participant::RelationshipType;

impl bincode::Encode for RelationshipType {
    fn encode<E: bincode::enc::Encoder>(
        &self,
        encoder: &mut E,
    ) -> Result<(), bincode::error::EncodeError> {
        (self.clone() as u8).encode(encoder)
    }
}
impl<Context> bincode::Decode<Context> for RelationshipType {
    fn decode<D: bincode::de::Decoder>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        let v = u8::decode(decoder)?;
        Ok(match v {
            0 => RelationshipType::Boss,
            1 => RelationshipType::DirectReport,
            2 => RelationshipType::Colleague,
            3 => RelationshipType::Collaborator,
            4 => RelationshipType::TeamMember,
            5 => RelationshipType::Family,
            6 => RelationshipType::Friend,
            7 => RelationshipType::Acquaintance,
            8 => RelationshipType::ServiceProvider,
            9 => RelationshipType::Customer,
            10 => RelationshipType::Support,
            11 => RelationshipType::Bot,
            12 => RelationshipType::Service,
            13 => RelationshipType::Monitor,
            _ => {
                return Err(bincode::error::DecodeError::Other(
                    "Invalid RelationshipType discriminant",
                ));
            }
        })
    }
}
impl<'de, Context> bincode::BorrowDecode<'de, Context> for RelationshipType {
    fn borrow_decode<D: bincode::de::BorrowDecoder<'de>>(
        decoder: &mut D,
    ) -> Result<Self, bincode::error::DecodeError> {
        <Self as bincode::Decode<<D as bincode::de::Decoder>::Context>>::decode(decoder)
    }
}
// Manual bincode Encode/Decode/BorrowDecode implementations for types that cannot derive them
mod bincode_impls {
    use super::super::*;
    use bincode::{BorrowDecode, Decode, Encode};
    use chrono::TimeDelta;
    use dashmap::DashMap;

    // --- Add manual impls for VerifiableActionSummary ---
    impl Encode for VerifiableActionSummary {
        fn encode<E: bincode::enc::Encoder>(
            &self,
            encoder: &mut E,
        ) -> Result<(), bincode::error::EncodeError> {
            self.action_id.encode(encoder)?;
            // Use manual encoding for ActionType as discriminant
            (self.action_type.clone() as u8).encode(encoder)?;
            self.impact_score.encode(encoder)?;
            self.timestamp.encode(encoder)?;
            self.blockchain_hash.encode(encoder)?;
            self.consensus_score.encode(encoder)?;
            Ok(())
        }
    }
    impl<Context> Decode<Context> for VerifiableActionSummary {
        fn decode<D: bincode::de::Decoder>(
            decoder: &mut D,
        ) -> Result<Self, bincode::error::DecodeError> {
            let action_id = Decode::decode(decoder)?;
            let action_type_val = u8::decode(decoder)?;
            let action_type = match action_type_val {
                0 => ActionType::HelpfulResponse,
                1 => ActionType::KnowledgeSharing,
                2 => ActionType::BugReport,
                3 => ActionType::Mentoring,
                4 => ActionType::Collaboration,
                5 => ActionType::TrustValidation,
                6 => ActionType::Spam,
                7 => ActionType::Harassment,
                8 => ActionType::Misinformation,
                9 => ActionType::BadFaith,
                10 => ActionType::Abuse,
                11 => ActionType::FalseReport,
                _ => {
                    return Err(bincode::error::DecodeError::Other(
                        "Invalid ActionType discriminant",
                    ));
                }
            };
            let impact_score = Decode::decode(decoder)?;
            let timestamp = Decode::decode(decoder)?;
            let blockchain_hash = Decode::decode(decoder)?;
            let consensus_score = Decode::decode(decoder)?;
            Ok(VerifiableActionSummary {
                action_id,
                action_type,
                impact_score,
                timestamp,
                blockchain_hash,
                consensus_score,
            })
        }
    }
    impl<'de, Context> BorrowDecode<'de, Context> for VerifiableActionSummary {
        fn borrow_decode<D: bincode::de::BorrowDecoder<'de>>(
            decoder: &mut D,
        ) -> Result<Self, bincode::error::DecodeError> {
            <Self as Decode<<D as bincode::de::Decoder>::Context>>::decode(decoder)
        }
    }

    // Manual impls for TrustCategory (as discriminant)
    impl bincode::Encode for TrustCategory {
        fn encode<E: bincode::enc::Encoder>(
            &self,
            encoder: &mut E,
        ) -> Result<(), bincode::error::EncodeError> {
            (self.clone() as u8).encode(encoder)
        }
    }
    impl<Context> bincode::Decode<Context> for TrustCategory {
        fn decode<D: bincode::de::Decoder>(
            decoder: &mut D,
        ) -> Result<Self, bincode::error::DecodeError> {
            let v = u8::decode(decoder)?;
            // SAFETY: assumes all discriminants are valid
            Ok(match v {
                0 => TrustCategory::Communication, // Clear, respectful, responsive
                1 => TrustCategory::Technical,     // Competent, knowledgeable, helpful
                2 => TrustCategory::Collaboration, // Good team player, reliable
                3 => TrustCategory::Reliability,   // Follows through on commitments
                4 => TrustCategory::Privacy,       // Respects confidentiality and data
                5 => TrustCategory::Overall,       // General trustworthiness
                _ => {
                    return Err(bincode::error::DecodeError::Other(
                        "Invalid TrustCategory discriminant",
                    ));
                }
            })
        }
    }
    impl<'de, Context> bincode::BorrowDecode<'de, Context> for TrustCategory {
        fn borrow_decode<D: bincode::de::BorrowDecoder<'de>>(
            decoder: &mut D,
        ) -> Result<Self, bincode::error::DecodeError> {
            <Self as bincode::Decode<<D as bincode::de::Decoder>::Context>>::decode(decoder)
        }
    }

    // DashMap cannot derive Encode/Decode, so we serialize as Vec<(K, V)>
    impl Encode for EntityTrustRatings {
        fn encode<E: bincode::enc::Encoder>(
            &self,
            encoder: &mut E,
        ) -> Result<(), bincode::error::EncodeError> {
            let received: Vec<(String, DirectTrustScore)> = self
                .received_ratings
                .iter()
                .map(|r| (r.key().clone(), r.value().clone()))
                .collect();
            let given: Vec<(String, DirectTrustScore)> = self
                .given_ratings
                .iter()
                .map(|r| (r.key().clone(), r.value().clone()))
                .collect();
            received.encode(encoder)?;
            given.encode(encoder)?;
            self.average_received.encode(encoder)?;
            self.total_ratings_received.encode(encoder)?;
            self.last_updated.encode(encoder)?;
            Ok(())
        }
    }
    impl<Context> Decode<Context> for EntityTrustRatings {
        fn decode<D: bincode::de::Decoder>(
            decoder: &mut D,
        ) -> Result<Self, bincode::error::DecodeError> {
            let received: Vec<(String, DirectTrustScore)> = Decode::decode(decoder)?;
            let given: Vec<(String, DirectTrustScore)> = Decode::decode(decoder)?;
            let average_received = Decode::decode(decoder)?;
            let total_ratings_received = Decode::decode(decoder)?;
            let last_updated = Decode::decode(decoder)?;
            let received_ratings = DashMap::new();
            for (k, v) in received {
                received_ratings.insert(k, v);
            }
            let given_ratings = DashMap::new();
            for (k, v) in given {
                given_ratings.insert(k, v);
            }
            Ok(EntityTrustRatings {
                received_ratings,
                given_ratings,
                average_received,
                total_ratings_received,
                last_updated,
            })
        }
    }
    impl<'de, Context> BorrowDecode<'de, Context> for EntityTrustRatings {
        fn borrow_decode<D: bincode::de::BorrowDecoder<'de>>(
            decoder: &mut D,
        ) -> Result<Self, bincode::error::DecodeError> {
            <Self as Decode<<D as bincode::de::Decoder>::Context>>::decode(decoder)
        }
    }

    // DirectTrustScore: Option<RelationshipType> cannot derive
    impl Encode for DirectTrustScore {
        fn encode<E: bincode::enc::Encoder>(
            &self,
            encoder: &mut E,
        ) -> Result<(), bincode::error::EncodeError> {
            self.score.encode(encoder)?;
            self.category.encode(encoder)?;
            self.given_by.encode(encoder)?;
            self.given_at.encode(encoder)?;
            self.comment.encode(encoder)?;
            self.relationship_context.encode(encoder)?;
            Ok(())
        }
    }
    impl<Context> Decode<Context> for DirectTrustScore {
        fn decode<D: bincode::de::Decoder>(
            decoder: &mut D,
        ) -> Result<Self, bincode::error::DecodeError> {
            Ok(DirectTrustScore {
                score: Decode::decode(decoder)?,
                category: Decode::decode(decoder)?,
                given_by: Decode::decode(decoder)?,
                given_at: Decode::decode(decoder)?,
                comment: Decode::decode(decoder)?,
                relationship_context: Decode::decode(decoder)?,
            })
        }
    }
    impl<'de, Context> BorrowDecode<'de, Context> for DirectTrustScore {
        fn borrow_decode<D: bincode::de::BorrowDecoder<'de>>(
            decoder: &mut D,
        ) -> Result<Self, bincode::error::DecodeError> {
            <Self as Decode<<D as bincode::de::Decoder>::Context>>::decode(decoder)
        }
    }

    // NetworkTrustRating: TrustBalance, ParticipationMetrics, Vec<VerifiableActionSummary>
    impl Encode for NetworkTrustRating {
        fn encode<E: bincode::enc::Encoder>(
            &self,
            encoder: &mut E,
        ) -> Result<(), bincode::error::EncodeError> {
            self.network_score.encode(encoder)?;
            self.trust_balance.encode(encoder)?;
            self.participation_metrics.encode(encoder)?;
            self.recent_actions.encode(encoder)?;
            self.last_calculated.encode(encoder)?;
            Ok(())
        }
    }
    impl<Context> Decode<Context> for NetworkTrustRating {
        fn decode<D: bincode::de::Decoder>(
            decoder: &mut D,
        ) -> Result<Self, bincode::error::DecodeError> {
            Ok(NetworkTrustRating {
                network_score: Decode::decode(decoder)?,
                trust_balance: Decode::decode(decoder)?,
                participation_metrics: Decode::decode(decoder)?,
                recent_actions: Decode::decode(decoder)?,
                last_calculated: Decode::decode(decoder)?,
            })
        }
    }
    impl<'de, Context> BorrowDecode<'de, Context> for NetworkTrustRating {
        fn borrow_decode<D: bincode::de::BorrowDecoder<'de>>(
            decoder: &mut D,
        ) -> Result<Self, bincode::error::DecodeError> {
            // Fix: Use the decoder's associated Context type
            <Self as Decode<<D as bincode::de::Decoder>::Context>>::decode(decoder)
        }
    }

    // TrustBalance
    impl Encode for TrustBalance {
        fn encode<E: bincode::enc::Encoder>(
            &self,
            encoder: &mut E,
        ) -> Result<(), bincode::error::EncodeError> {
            self.participant_id.encode(encoder)?;
            self.total_points.encode(encoder)?;
            self.available_points.encode(encoder)?;
            self.staked_points.encode(encoder)?;
            self.earned_lifetime.encode(encoder)?;
            self.last_activity.encode(encoder)?;
            self.decay_rate.encode(encoder)?;
            Ok(())
        }
    }
    impl<Context> Decode<Context> for TrustBalance {
        fn decode<D: bincode::de::Decoder>(
            decoder: &mut D,
        ) -> Result<Self, bincode::error::DecodeError> {
            Ok(TrustBalance {
                participant_id: Decode::decode(decoder)?,
                total_points: Decode::decode(decoder)?,
                available_points: Decode::decode(decoder)?,
                staked_points: Decode::decode(decoder)?,
                earned_lifetime: Decode::decode(decoder)?,
                last_activity: Decode::decode(decoder)?,
                decay_rate: Decode::decode(decoder)?,
            })
        }
    }
    impl<'de, Context> BorrowDecode<'de, Context> for TrustBalance {
        fn borrow_decode<D: bincode::de::BorrowDecoder<'de>>(
            decoder: &mut D,
        ) -> Result<Self, bincode::error::DecodeError> {
            <Self as Decode<<D as bincode::de::Decoder>::Context>>::decode(decoder)
        }
    }

    // ParticipationMetrics
    impl Encode for ParticipationMetrics {
        fn encode<E: bincode::enc::Encoder>(
            &self,
            encoder: &mut E,
        ) -> Result<(), bincode::error::EncodeError> {
            self.messages_sent.encode(encoder)?;
            self.messages_received.encode(encoder)?;
            self.response_rate.encode(encoder)?;
            self.successful_collaborations.encode(encoder)?;
            self.knowledge_contributions.encode(encoder)?;
            self.helpful_responses_given.encode(encoder)?;
            self.trust_reports_submitted.encode(encoder)?;
            self.trust_reports_validated.encode(encoder)?;
            self.trust_stakes_won.encode(encoder)?;
            self.trust_stakes_lost.encode(encoder)?;
            self.reports_against.encode(encoder)?;
            self.reports_validated_against.encode(encoder)?;
            self.spam_reports.encode(encoder)?;
            // Fix: encode account_age as seconds (i64)
            self.account_age.num_seconds().encode(encoder)?;
            self.days_active.encode(encoder)?;
            self.last_active.encode(encoder)?;
            Ok(())
        }
    }
    impl<Context> Decode<Context> for ParticipationMetrics {
        fn decode<D: bincode::de::Decoder>(
            decoder: &mut D,
        ) -> Result<Self, bincode::error::DecodeError> {
            Ok(ParticipationMetrics {
                messages_sent: Decode::decode(decoder)?,
                messages_received: Decode::decode(decoder)?,
                response_rate: Decode::decode(decoder)?,
                successful_collaborations: Decode::decode(decoder)?,
                knowledge_contributions: Decode::decode(decoder)?,
                helpful_responses_given: Decode::decode(decoder)?,
                trust_reports_submitted: Decode::decode(decoder)?,
                trust_reports_validated: Decode::decode(decoder)?,
                trust_stakes_won: Decode::decode(decoder)?,
                trust_stakes_lost: Decode::decode(decoder)?,
                reports_against: Decode::decode(decoder)?,
                reports_validated_against: Decode::decode(decoder)?,
                spam_reports: Decode::decode(decoder)?,
                // Fix: decode account_age from seconds (i64)
                account_age: {
                    let secs = i64::decode(decoder)?;
                    TimeDelta::seconds(secs)
                },
                days_active: Decode::decode(decoder)?,
                last_active: Decode::decode(decoder)?,
            })
        }
    }
    impl<'de, Context> BorrowDecode<'de, Context> for ParticipationMetrics {
        fn borrow_decode<D: bincode::de::BorrowDecoder<'de>>(
            decoder: &mut D,
        ) -> Result<Self, bincode::error::DecodeError> {
            <Self as Decode<<D as bincode::de::Decoder>::Context>>::decode(decoder)
        }
    }

    // Manual impls for ActionType (as discriminant)
    impl bincode::Encode for ActionType {
        fn encode<E: bincode::enc::Encoder>(
            &self,
            encoder: &mut E,
        ) -> Result<(), bincode::error::EncodeError> {
            (self.clone() as u8).encode(encoder)
        }
    }
    impl<Context> bincode::Decode<Context> for ActionType {
        fn decode<D: bincode::de::Decoder>(
            decoder: &mut D,
        ) -> Result<Self, bincode::error::DecodeError> {
            let v = u8::decode(decoder)?;
            Ok(match v {
                0 => ActionType::HelpfulResponse,
                1 => ActionType::KnowledgeSharing,
                2 => ActionType::BugReport,
                3 => ActionType::Mentoring,
                4 => ActionType::Collaboration,
                5 => ActionType::TrustValidation,
                6 => ActionType::Spam,
                7 => ActionType::Harassment,
                8 => ActionType::Misinformation,
                9 => ActionType::BadFaith,
                10 => ActionType::Abuse,
                11 => ActionType::FalseReport,
                _ => {
                    return Err(bincode::error::DecodeError::Other(
                        "Invalid ActionType discriminant",
                    ));
                }
            })
        }
    }
    impl<'de, Context> bincode::BorrowDecode<'de, Context> for ActionType {
        fn borrow_decode<D: bincode::de::BorrowDecoder<'de>>(
            decoder: &mut D,
        ) -> Result<Self, bincode::error::DecodeError> {
            <Self as bincode::Decode<<D as bincode::de::Decoder>::Context>>::decode(decoder)
        }
    }
}
