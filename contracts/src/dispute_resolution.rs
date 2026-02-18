//! # Dispute Resolution Contract
//!
//! Handles disputes that arise from credit escrow operations. Either the
//! lender or borrower can open a dispute, submit evidence, and request
//! arbitration. Resolution is determined by one or more designated arbiters
//! who review the evidence and cast a binding vote.
//!
//! ## Evidence Model
//!
//! Evidence is stored as a content hash (BLAKE3) rather than the raw data.
//! The actual evidence payload lives off-chain (IPFS, S3, etc.) — the hash
//! on-chain serves as a tamper-proof anchor. This keeps block sizes sane
//! while still providing non-repudiation.
//!
//! ## Resolution Flow
//!
//! 1. Either party opens a dispute on an active escrow.
//! 2. Both parties submit evidence (hashes + descriptions).
//! 3. An arbiter reviews the evidence off-chain.
//! 4. The arbiter calls `resolve()` with a signed resolution.
//! 5. The dispute status transitions to `ResolvedForInitiator` or
//!    `ResolvedForRespondent`, and the escrow is updated accordingly.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during dispute operations.
#[derive(Debug, Error)]
pub enum DisputeError {
    /// The dispute is not in a state that allows this operation.
    #[error("invalid state: dispute is {current}, expected {expected}")]
    InvalidState {
        /// Current dispute status.
        current: String,
        /// Required status for the attempted operation.
        expected: String,
    },

    /// The caller is not a party to this dispute.
    #[error("unauthorized: {party} is not a participant in this dispute")]
    Unauthorized {
        /// The address that attempted the operation.
        party: String,
    },

    /// The arbiter's signature failed verification.
    #[error("invalid arbiter signature")]
    InvalidArbiterSignature,

    /// The resolution type is not recognized.
    #[error("invalid resolution: {0}")]
    InvalidResolution(String),

    /// The dispute has already been resolved.
    #[error("dispute already resolved")]
    AlreadyResolved,
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// The current status of a dispute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum DisputeStatus {
    /// Dispute has been opened but not yet reviewed.
    Open,
    /// An arbiter has begun reviewing the evidence.
    UnderReview,
    /// Resolved in favor of the party who initiated the dispute.
    ResolvedForInitiator,
    /// Resolved in favor of the respondent.
    ResolvedForRespondent,
    /// Dispute was cancelled by the initiator before resolution.
    Cancelled,
}

impl std::fmt::Display for DisputeStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            DisputeStatus::Open => write!(f, "Open"),
            DisputeStatus::UnderReview => write!(f, "UnderReview"),
            DisputeStatus::ResolvedForInitiator => write!(f, "ResolvedForInitiator"),
            DisputeStatus::ResolvedForRespondent => write!(f, "ResolvedForRespondent"),
            DisputeStatus::Cancelled => write!(f, "Cancelled"),
        }
    }
}

/// The outcome requested when resolving a dispute.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum Resolution {
    /// Rule in favor of the initiator (e.g., refund the lender).
    ForInitiator,
    /// Rule in favor of the respondent (e.g., release funds to borrower).
    ForRespondent,
}

/// A piece of evidence submitted by a dispute participant.
///
/// The actual evidence payload (documents, screenshots, logs) is stored
/// off-chain. Only the content hash is recorded on-chain for integrity.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Evidence {
    /// Hex-encoded public key of the party that submitted this evidence.
    pub submitted_by: String,
    /// Human-readable description of what this evidence demonstrates.
    pub description: String,
    /// BLAKE3 hash of the off-chain evidence payload, hex-encoded.
    pub data_hash: String,
    /// Timestamp when the evidence was submitted.
    pub timestamp: DateTime<Utc>,
}

/// A dispute associated with a credit escrow.
///
/// Tracks the full arbitration lifecycle from opening through evidence
/// submission to resolution.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Dispute {
    /// Unique identifier for this dispute.
    pub id: String,
    /// The escrow this dispute pertains to.
    pub escrow_id: String,
    /// Hex-encoded public key of the party who opened the dispute.
    pub initiator: String,
    /// Hex-encoded public key of the other party.
    pub respondent: String,
    /// The initiator's stated reason for opening the dispute.
    pub reason: String,
    /// Evidence submitted by both parties.
    pub evidence: Vec<Evidence>,
    /// Current dispute status.
    pub status: DisputeStatus,
    /// Timestamp when the dispute was created.
    pub created_at: DateTime<Utc>,
    /// Timestamp when the dispute was resolved (if applicable).
    pub resolved_at: Option<DateTime<Utc>>,
}

impl Dispute {
    /// Opens a new dispute against the given escrow.
    ///
    /// The dispute starts in `Open` status. Both parties can then submit
    /// evidence before an arbiter reviews and resolves.
    ///
    /// # Arguments
    ///
    /// * `escrow_id` - The ID of the escrow being disputed.
    /// * `initiator` - Hex-encoded public key of the party opening the dispute.
    /// * `respondent` - Hex-encoded public key of the other party.
    /// * `reason` - Free-text reason for the dispute.
    pub fn create(
        escrow_id: String,
        initiator: String,
        respondent: String,
        reason: String,
    ) -> Self {
        Self {
            id: Uuid::new_v4().to_string(),
            escrow_id,
            initiator,
            respondent,
            reason,
            evidence: Vec::new(),
            status: DisputeStatus::Open,
            created_at: Utc::now(),
            resolved_at: None,
        }
    }

    /// Submits a piece of evidence to the dispute.
    ///
    /// Only the initiator or respondent may submit evidence. Evidence can
    /// be submitted while the dispute is `Open` or `UnderReview`.
    ///
    /// # Arguments
    ///
    /// * `party` - Hex-encoded public key of the submitting party.
    /// * `description` - What this evidence demonstrates.
    /// * `data_hash` - BLAKE3 hash of the off-chain evidence payload.
    ///
    /// # Errors
    ///
    /// Returns [`DisputeError::Unauthorized`] if `party` is not a participant.
    /// Returns [`DisputeError::InvalidState`] if the dispute is already resolved.
    pub fn submit_evidence(
        &mut self,
        party: &str,
        description: String,
        data_hash: String,
    ) -> Result<(), DisputeError> {
        // Verify the caller is a dispute participant.
        if party != self.initiator && party != self.respondent {
            return Err(DisputeError::Unauthorized {
                party: party.to_string(),
            });
        }

        // Evidence can only be submitted in Open or UnderReview states.
        match self.status {
            DisputeStatus::Open | DisputeStatus::UnderReview => {}
            _ => {
                return Err(DisputeError::InvalidState {
                    current: self.status.to_string(),
                    expected: "Open or UnderReview".into(),
                });
            }
        }

        self.evidence.push(Evidence {
            submitted_by: party.to_string(),
            description,
            data_hash,
            timestamp: Utc::now(),
        });

        // First evidence submission transitions from Open to UnderReview.
        if self.status == DisputeStatus::Open {
            self.status = DisputeStatus::UnderReview;
        }

        Ok(())
    }

    /// Resolves the dispute with an arbiter's signed decision.
    ///
    /// The `arbiter_signature` is verified against the resolution payload.
    /// In production, this would use the protocol's signature verification
    /// via [`nova_protocol::crypto::signatures::verify`]. For now, we accept
    /// any non-empty signature as valid (the crypto verification layer is
    /// plugged in at the execution engine level).
    ///
    /// # Arguments
    ///
    /// * `resolution` - Whether the ruling favors the initiator or respondent.
    /// * `arbiter_signature` - Hex-encoded Ed25519 signature from the arbiter
    ///   over the dispute ID + resolution.
    ///
    /// # Errors
    ///
    /// Returns [`DisputeError::InvalidState`] if the dispute is not
    /// `Open` or `UnderReview`.
    /// Returns [`DisputeError::InvalidArbiterSignature`] if the signature is empty.
    pub fn resolve(
        &mut self,
        resolution: Resolution,
        arbiter_signature: &str,
    ) -> Result<(), DisputeError> {
        match self.status {
            DisputeStatus::Open | DisputeStatus::UnderReview => {}
            DisputeStatus::ResolvedForInitiator | DisputeStatus::ResolvedForRespondent => {
                return Err(DisputeError::AlreadyResolved);
            }
            _ => {
                return Err(DisputeError::InvalidState {
                    current: self.status.to_string(),
                    expected: "Open or UnderReview".into(),
                });
            }
        }

        // Verify the arbiter signature is present. Full Ed25519 verification
        // happens at the execution engine layer, which has access to the
        // arbiter's public key from the validator set.
        if arbiter_signature.is_empty() {
            return Err(DisputeError::InvalidArbiterSignature);
        }

        let now = Utc::now();
        self.status = match resolution {
            Resolution::ForInitiator => DisputeStatus::ResolvedForInitiator,
            Resolution::ForRespondent => DisputeStatus::ResolvedForRespondent,
        };
        self.resolved_at = Some(now);

        Ok(())
    }

    /// Cancels the dispute. Only the initiator can cancel, and only before
    /// resolution.
    ///
    /// # Arguments
    ///
    /// * `caller` - Hex-encoded public key of the caller.
    ///
    /// # Errors
    ///
    /// Returns [`DisputeError::Unauthorized`] if the caller is not the initiator.
    /// Returns [`DisputeError::InvalidState`] if the dispute is already resolved.
    pub fn cancel(&mut self, caller: &str) -> Result<(), DisputeError> {
        if caller != self.initiator {
            return Err(DisputeError::Unauthorized {
                party: caller.to_string(),
            });
        }

        match self.status {
            DisputeStatus::Open | DisputeStatus::UnderReview => {}
            _ => {
                return Err(DisputeError::InvalidState {
                    current: self.status.to_string(),
                    expected: "Open or UnderReview".into(),
                });
            }
        }

        self.status = DisputeStatus::Cancelled;
        self.resolved_at = Some(Utc::now());
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn create_test_dispute() -> Dispute {
        Dispute::create(
            "escrow-123".into(),
            "initiator_pk".into(),
            "respondent_pk".into(),
            "Funds not delivered as agreed".into(),
        )
    }

    #[test]
    fn new_dispute_is_open() {
        let d = create_test_dispute();
        assert_eq!(d.status, DisputeStatus::Open);
        assert!(d.evidence.is_empty());
        assert!(d.resolved_at.is_none());
    }

    #[test]
    fn submit_evidence_transitions_to_under_review() {
        let mut d = create_test_dispute();
        d.submit_evidence(
            "initiator_pk",
            "Payment receipt".into(),
            "abc123hash".into(),
        )
        .unwrap();
        assert_eq!(d.status, DisputeStatus::UnderReview);
        assert_eq!(d.evidence.len(), 1);
    }

    #[test]
    fn both_parties_can_submit_evidence() {
        let mut d = create_test_dispute();
        d.submit_evidence("initiator_pk", "doc1".into(), "hash1".into())
            .unwrap();
        d.submit_evidence("respondent_pk", "doc2".into(), "hash2".into())
            .unwrap();
        assert_eq!(d.evidence.len(), 2);
    }

    #[test]
    fn unauthorized_party_cannot_submit() {
        let mut d = create_test_dispute();
        let result = d.submit_evidence("random_pk", "doc".into(), "hash".into());
        assert!(result.is_err());
    }

    #[test]
    fn resolve_for_initiator() {
        let mut d = create_test_dispute();
        d.submit_evidence("initiator_pk", "proof".into(), "hash".into())
            .unwrap();
        d.resolve(Resolution::ForInitiator, "arbiter_sig_hex")
            .unwrap();
        assert_eq!(d.status, DisputeStatus::ResolvedForInitiator);
        assert!(d.resolved_at.is_some());
    }

    #[test]
    fn resolve_for_respondent() {
        let mut d = create_test_dispute();
        d.resolve(Resolution::ForRespondent, "arbiter_sig_hex")
            .unwrap();
        assert_eq!(d.status, DisputeStatus::ResolvedForRespondent);
    }

    #[test]
    fn empty_signature_rejected() {
        let mut d = create_test_dispute();
        let result = d.resolve(Resolution::ForInitiator, "");
        assert!(result.is_err());
    }

    #[test]
    fn double_resolve_rejected() {
        let mut d = create_test_dispute();
        d.resolve(Resolution::ForInitiator, "sig").unwrap();
        let result = d.resolve(Resolution::ForRespondent, "sig");
        assert!(result.is_err());
    }

    #[test]
    fn cancel_by_initiator() {
        let mut d = create_test_dispute();
        d.cancel("initiator_pk").unwrap();
        assert_eq!(d.status, DisputeStatus::Cancelled);
    }

    #[test]
    fn cancel_by_respondent_rejected() {
        let mut d = create_test_dispute();
        let result = d.cancel("respondent_pk");
        assert!(result.is_err());
    }
}
