// SPDX-License-Identifier: MIT OR Apache-2.0
//! The optional hook through which contact search and the privacy policy read a trust score.
//!
//! Synapse core carries no reputation model: a sender is authenticated (pinned keys, certificate
//! chains, revocations), not scored. A [`TrustSource`] lets a deployment supply a score from
//! elsewhere. The default, [`NoTrustSource`], answers [`TrustAnswer::Unsupported`], and a caller
//! that was asked to enforce a threshold turns that into an error. It never passes the check.

use anyhow::Result;
use async_trait::async_trait;
use std::fmt;

/// What a [`TrustSource`] knows about a participant.
#[derive(Debug, Clone, Copy, PartialEq)]
pub enum TrustAnswer {
    /// A score, higher is more trusted. The source and the thresholds it is compared with must share a
    /// scale; the built-in `TrustManager` reports its native 0-100 score unchanged.
    Score(f64),
    /// This source has no opinion. Not "zero"; not "trusted".
    Unsupported,
}

/// Returned when a trust threshold was requested but the source answered
/// [`TrustAnswer::Unsupported`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct TrustUnsupported;

impl fmt::Display for TrustUnsupported {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(
            "a trust threshold was requested but the TrustSource gave no answer (none is configured, or it has no opinion); \
             refusing to treat that as a pass",
        )
    }
}

impl std::error::Error for TrustUnsupported {}

/// A provider of trust evidence about a participant.
#[async_trait]
pub trait TrustSource: Send + Sync {
    /// How much `requester_id` should trust `subject_id`.
    async fn trust_of(&self, subject_id: &str, requester_id: &str) -> Result<TrustAnswer>;
}

/// The default source: has no opinion on anyone.
#[derive(Debug, Default, Clone, Copy)]
pub struct NoTrustSource;

#[async_trait]
impl TrustSource for NoTrustSource {
    async fn trust_of(&self, _subject_id: &str, _requester_id: &str) -> Result<TrustAnswer> {
        Ok(TrustAnswer::Unsupported)
    }
}

/// Does `subject_id` meet `threshold` according to `source`?
///
/// - `Score(s)` passes when `s >= threshold`.
/// - `Unsupported` is an error ([`TrustUnsupported`]), never a pass.
/// - A source error propagates, never a pass.
pub async fn meets_threshold(
    source: &dyn TrustSource,
    subject_id: &str,
    requester_id: &str,
    threshold: f64,
) -> Result<bool> {
    match source.trust_of(subject_id, requester_id).await? {
        TrustAnswer::Score(score) => Ok(score >= threshold),
        TrustAnswer::Unsupported => Err(TrustUnsupported.into()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    struct Fixed(f64);

    #[async_trait]
    impl TrustSource for Fixed {
        async fn trust_of(&self, _s: &str, _r: &str) -> Result<TrustAnswer> {
            Ok(TrustAnswer::Score(self.0))
        }
    }

    struct Broken;

    #[async_trait]
    impl TrustSource for Broken {
        async fn trust_of(&self, _s: &str, _r: &str) -> Result<TrustAnswer> {
            Err(anyhow::anyhow!("backend down"))
        }
    }

    #[tokio::test]
    async fn default_source_is_unsupported_not_a_score() {
        let a = NoTrustSource.trust_of("a", "b").await.unwrap();
        assert_eq!(a, TrustAnswer::Unsupported);
    }

    #[tokio::test]
    async fn unsupported_is_an_error_never_a_pass() {
        let err = meets_threshold(&NoTrustSource, "a", "b", 0.5)
            .await
            .unwrap_err();
        assert!(err.downcast_ref::<TrustUnsupported>().is_some());
    }

    #[tokio::test]
    async fn score_at_or_above_threshold_passes_below_fails() {
        assert!(meets_threshold(&Fixed(0.5), "a", "b", 0.5).await.unwrap());
        assert!(meets_threshold(&Fixed(0.9), "a", "b", 0.5).await.unwrap());
        assert!(!meets_threshold(&Fixed(0.49), "a", "b", 0.5).await.unwrap());
    }

    #[tokio::test]
    async fn a_failing_source_is_an_error_never_a_pass() {
        let err = meets_threshold(&Broken, "a", "b", 0.1).await.unwrap_err();
        assert!(err.downcast_ref::<TrustUnsupported>().is_none());
        assert!(err.to_string().contains("backend down"));
    }
}
