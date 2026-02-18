//! Integration tests for the dispute resolution contract.
//!
//! These tests exercise the full dispute lifecycle: creation, evidence
//! submission by both parties, arbiter resolution, and cancellation.
//! Also verifies the integration between the escrow and dispute contracts.

use chrono::{Duration, Utc};
use nova_contracts::credit_escrow::{CreditEscrow, CreditTerms, EscrowStatus};
use nova_contracts::dispute_resolution::{Dispute, DisputeStatus, Resolution};

/// Helper: creates a funded and active escrow ready for dispute testing.
fn active_escrow() -> CreditEscrow {
    let terms = CreditTerms {
        principal: 5_000_000,
        interest_rate_bps: 300,
        total_owed: 5_150_000,
        repayment_deadline: Utc::now() + Duration::days(60),
        grace_period_secs: 86400,
    };
    let mut escrow = CreditEscrow::create("lender_pk".into(), "borrower_pk".into(), terms);
    escrow.fund(5_000_000).unwrap();
    escrow.release_to_borrower(5_000_000).unwrap();
    escrow
}

// ---------------------------------------------------------------------------
// Lifecycle Tests
// ---------------------------------------------------------------------------

#[test]
fn dispute_starts_in_open_state() {
    let d = Dispute::create(
        "escrow-1".into(),
        "lender_pk".into(),
        "borrower_pk".into(),
        "Funds not received".into(),
    );
    assert_eq!(d.status, DisputeStatus::Open);
    assert!(d.evidence.is_empty());
    assert!(d.resolved_at.is_none());
}

#[test]
fn evidence_submission_transitions_to_under_review() {
    let mut d = Dispute::create(
        "escrow-1".into(),
        "lender_pk".into(),
        "borrower_pk".into(),
        "Short delivery".into(),
    );

    d.submit_evidence(
        "lender_pk",
        "Bank transfer receipt".into(),
        "a1b2c3d4e5f6".into(),
    )
    .unwrap();

    assert_eq!(d.status, DisputeStatus::UnderReview);
    assert_eq!(d.evidence.len(), 1);
    assert_eq!(d.evidence[0].submitted_by, "lender_pk");
}

#[test]
fn both_parties_submit_evidence() {
    let mut d = Dispute::create(
        "escrow-1".into(),
        "lender_pk".into(),
        "borrower_pk".into(),
        "Disagreement on terms".into(),
    );

    d.submit_evidence("lender_pk", "Contract screenshot".into(), "hash1".into())
        .unwrap();
    d.submit_evidence("borrower_pk", "Chat log export".into(), "hash2".into())
        .unwrap();
    d.submit_evidence("lender_pk", "Additional proof".into(), "hash3".into())
        .unwrap();

    assert_eq!(d.evidence.len(), 3);
}

#[test]
fn resolve_in_favor_of_initiator() {
    let mut d = Dispute::create(
        "escrow-1".into(),
        "lender_pk".into(),
        "borrower_pk".into(),
        "Non-delivery".into(),
    );

    d.submit_evidence("lender_pk", "proof".into(), "hash".into())
        .unwrap();
    d.resolve(Resolution::ForInitiator, "arbiter_signature_hex")
        .unwrap();

    assert_eq!(d.status, DisputeStatus::ResolvedForInitiator);
    assert!(d.resolved_at.is_some());
}

#[test]
fn resolve_in_favor_of_respondent() {
    let mut d = Dispute::create(
        "escrow-1".into(),
        "lender_pk".into(),
        "borrower_pk".into(),
        "Funds withheld".into(),
    );

    d.resolve(Resolution::ForRespondent, "arbiter_sig").unwrap();
    assert_eq!(d.status, DisputeStatus::ResolvedForRespondent);
}

// ---------------------------------------------------------------------------
// Error Cases
// ---------------------------------------------------------------------------

#[test]
fn unauthorized_party_cannot_submit_evidence() {
    let mut d = Dispute::create(
        "escrow-1".into(),
        "lender_pk".into(),
        "borrower_pk".into(),
        "Issue".into(),
    );

    let result = d.submit_evidence("bystander_pk", "doc".into(), "hash".into());
    assert!(result.is_err());
}

#[test]
fn cannot_submit_evidence_after_resolution() {
    let mut d = Dispute::create(
        "escrow-1".into(),
        "lender_pk".into(),
        "borrower_pk".into(),
        "Issue".into(),
    );

    d.resolve(Resolution::ForInitiator, "sig").unwrap();

    let result = d.submit_evidence("lender_pk", "late evidence".into(), "hash".into());
    assert!(result.is_err());
}

#[test]
fn cannot_resolve_with_empty_signature() {
    let mut d = Dispute::create(
        "escrow-1".into(),
        "lender_pk".into(),
        "borrower_pk".into(),
        "Issue".into(),
    );

    let result = d.resolve(Resolution::ForInitiator, "");
    assert!(result.is_err());
}

#[test]
fn cannot_resolve_twice() {
    let mut d = Dispute::create(
        "escrow-1".into(),
        "lender_pk".into(),
        "borrower_pk".into(),
        "Issue".into(),
    );

    d.resolve(Resolution::ForInitiator, "sig").unwrap();
    let result = d.resolve(Resolution::ForRespondent, "sig2");
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Cancellation
// ---------------------------------------------------------------------------

#[test]
fn initiator_can_cancel_open_dispute() {
    let mut d = Dispute::create(
        "escrow-1".into(),
        "lender_pk".into(),
        "borrower_pk".into(),
        "Mistake".into(),
    );

    d.cancel("lender_pk").unwrap();
    assert_eq!(d.status, DisputeStatus::Cancelled);
    assert!(d.resolved_at.is_some());
}

#[test]
fn initiator_can_cancel_under_review_dispute() {
    let mut d = Dispute::create(
        "escrow-1".into(),
        "lender_pk".into(),
        "borrower_pk".into(),
        "Changed mind".into(),
    );

    d.submit_evidence("lender_pk", "doc".into(), "hash".into())
        .unwrap();
    assert_eq!(d.status, DisputeStatus::UnderReview);

    d.cancel("lender_pk").unwrap();
    assert_eq!(d.status, DisputeStatus::Cancelled);
}

#[test]
fn respondent_cannot_cancel() {
    let mut d = Dispute::create(
        "escrow-1".into(),
        "lender_pk".into(),
        "borrower_pk".into(),
        "Issue".into(),
    );

    let result = d.cancel("borrower_pk");
    assert!(result.is_err());
}

#[test]
fn cannot_cancel_resolved_dispute() {
    let mut d = Dispute::create(
        "escrow-1".into(),
        "lender_pk".into(),
        "borrower_pk".into(),
        "Issue".into(),
    );

    d.resolve(Resolution::ForRespondent, "sig").unwrap();
    let result = d.cancel("lender_pk");
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Escrow + Dispute Integration
// ---------------------------------------------------------------------------

#[test]
fn escrow_dispute_freezes_operations() {
    let mut escrow = active_escrow();
    assert_eq!(escrow.status, EscrowStatus::Active);

    // Open a dispute on the escrow.
    escrow
        .dispute("Borrower misrepresented use of funds")
        .unwrap();
    assert_eq!(escrow.status, EscrowStatus::Disputed);

    // Create the corresponding dispute record.
    let dispute = Dispute::create(
        escrow.escrow_id.clone(),
        escrow.lender.clone(),
        escrow.borrower.clone(),
        "Borrower misrepresented use of funds".into(),
    );
    assert_eq!(dispute.escrow_id, escrow.escrow_id);
    assert_eq!(dispute.status, DisputeStatus::Open);

    // While disputed, escrow operations fail.
    assert!(escrow.release_to_borrower(100).is_err());
    assert!(escrow.repay(100).is_err());
}

#[test]
fn serialization_roundtrip_dispute() {
    let mut d = Dispute::create(
        "escrow-42".into(),
        "lender_pk".into(),
        "borrower_pk".into(),
        "Test dispute".into(),
    );
    d.submit_evidence("lender_pk", "doc".into(), "hash".into())
        .unwrap();

    let json = serde_json::to_string(&d).unwrap();
    let restored: Dispute = serde_json::from_str(&json).unwrap();

    assert_eq!(d.id, restored.id);
    assert_eq!(d.escrow_id, restored.escrow_id);
    assert_eq!(d.status, restored.status);
    assert_eq!(d.evidence.len(), restored.evidence.len());
}
