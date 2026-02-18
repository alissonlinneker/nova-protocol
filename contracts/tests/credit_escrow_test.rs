//! Integration tests for the credit escrow contract.
//!
//! These tests exercise the full escrow lifecycle across module boundaries,
//! simulating real-world scenarios: partial funding, incremental repayment,
//! default detection, and dispute initiation.

use chrono::{Duration, Utc};
use nova_contracts::credit_escrow::{CreditEscrow, CreditTerms, EscrowStatus};

/// Helper: creates standard credit terms with the given principal.
fn terms(principal: u64, days_until_deadline: i64) -> CreditTerms {
    CreditTerms {
        principal,
        interest_rate_bps: 500, // 5%
        total_owed: principal + (principal / 20),
        repayment_deadline: Utc::now() + Duration::days(days_until_deadline),
        grace_period_secs: 86400,
    }
}

// ---------------------------------------------------------------------------
// Lifecycle Tests
// ---------------------------------------------------------------------------

#[test]
fn full_lifecycle_happy_path() {
    let t = terms(10_000_000, 30);
    let total = t.total_owed;
    let mut escrow = CreditEscrow::create("lender".into(), "borrower".into(), t);

    // 1. Fund
    assert_eq!(escrow.status, EscrowStatus::Pending);
    escrow.fund(10_000_000).unwrap();
    assert_eq!(escrow.status, EscrowStatus::Funded);

    // 2. Release
    escrow.release_to_borrower(10_000_000).unwrap();
    assert_eq!(escrow.status, EscrowStatus::Active);

    // 3. Repay
    escrow.repay(total).unwrap();
    assert_eq!(escrow.status, EscrowStatus::Completed);
}

#[test]
fn partial_funding_then_full_funding() {
    let t = terms(1_000_000, 30);
    let mut escrow = CreditEscrow::create("l".into(), "b".into(), t);

    escrow.fund(300_000).unwrap();
    assert_eq!(escrow.status, EscrowStatus::Pending);
    assert_eq!(escrow.funded_amount, 300_000);

    escrow.fund(700_000).unwrap();
    assert_eq!(escrow.status, EscrowStatus::Funded);
    assert_eq!(escrow.funded_amount, 1_000_000);
}

#[test]
fn partial_release_and_multiple_repayments() {
    let t = terms(2_000_000, 30);
    let total = t.total_owed;
    let mut escrow = CreditEscrow::create("l".into(), "b".into(), t);

    escrow.fund(2_000_000).unwrap();

    // Release in two tranches.
    escrow.release_to_borrower(1_000_000).unwrap();
    assert_eq!(escrow.released_amount, 1_000_000);

    escrow.release_to_borrower(1_000_000).unwrap();
    assert_eq!(escrow.released_amount, 2_000_000);

    // Repay in three installments.
    let installment = total / 3;
    escrow.repay(installment).unwrap();
    escrow.repay(installment).unwrap();
    // Final installment covers the remainder.
    let remainder = total - (installment * 2);
    escrow.repay(remainder).unwrap();
    assert_eq!(escrow.status, EscrowStatus::Completed);
}

// ---------------------------------------------------------------------------
// Error Cases
// ---------------------------------------------------------------------------

#[test]
fn cannot_fund_when_already_funded() {
    let t = terms(1_000_000, 30);
    let mut escrow = CreditEscrow::create("l".into(), "b".into(), t);
    escrow.fund(1_000_000).unwrap();

    // Escrow is now Funded — additional funding should fail.
    let result = escrow.fund(1);
    assert!(result.is_err());
}

#[test]
fn cannot_release_when_pending() {
    let t = terms(1_000_000, 30);
    let mut escrow = CreditEscrow::create("l".into(), "b".into(), t);

    // No funding yet — release should fail.
    let result = escrow.release_to_borrower(500_000);
    assert!(result.is_err());
}

#[test]
fn cannot_repay_when_funded_but_not_released() {
    let t = terms(1_000_000, 30);
    let mut escrow = CreditEscrow::create("l".into(), "b".into(), t);
    escrow.fund(1_000_000).unwrap();

    // Funded but not Active — repay should fail.
    let result = escrow.repay(100);
    assert!(result.is_err());
}

#[test]
fn cannot_repay_after_completion() {
    let t = terms(1_000_000, 30);
    let total = t.total_owed;
    let mut escrow = CreditEscrow::create("l".into(), "b".into(), t);
    escrow.fund(1_000_000).unwrap();
    escrow.release_to_borrower(1_000_000).unwrap();
    escrow.repay(total).unwrap();

    assert_eq!(escrow.status, EscrowStatus::Completed);
    let result = escrow.repay(1);
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Default Detection
// ---------------------------------------------------------------------------

#[test]
fn check_default_before_deadline_returns_false() {
    let t = terms(1_000_000, 30);
    let mut escrow = CreditEscrow::create("l".into(), "b".into(), t);
    escrow.fund(1_000_000).unwrap();
    escrow.release_to_borrower(1_000_000).unwrap();

    // Deadline is 30 days from now — should not be defaulted.
    assert!(!escrow.check_default());
    assert_eq!(escrow.status, EscrowStatus::Active);
}

#[test]
fn check_default_after_deadline_transitions_to_defaulted() {
    // Create terms with a deadline in the past.
    let t = CreditTerms {
        principal: 1_000_000,
        interest_rate_bps: 500,
        total_owed: 1_050_000,
        repayment_deadline: Utc::now() - Duration::days(2),
        grace_period_secs: 3600, // 1 hour grace — still in the past.
    };
    let mut escrow = CreditEscrow::create("l".into(), "b".into(), t);
    escrow.fund(1_000_000).unwrap();
    escrow.release_to_borrower(1_000_000).unwrap();

    assert!(escrow.check_default());
    assert_eq!(escrow.status, EscrowStatus::Defaulted);
}

#[test]
fn check_default_not_active_returns_false() {
    let t = terms(1_000_000, 30);
    let mut escrow = CreditEscrow::create("l".into(), "b".into(), t);
    // Still Pending — not eligible for default.
    assert!(!escrow.check_default());
}

// ---------------------------------------------------------------------------
// Dispute Integration
// ---------------------------------------------------------------------------

#[test]
fn dispute_freezes_escrow() {
    let t = terms(1_000_000, 30);
    let mut escrow = CreditEscrow::create("l".into(), "b".into(), t);
    escrow.fund(1_000_000).unwrap();
    escrow.release_to_borrower(500_000).unwrap();

    escrow.dispute("Funds misappropriated").unwrap();
    assert_eq!(escrow.status, EscrowStatus::Disputed);

    // Cannot release or repay while disputed.
    assert!(escrow.release_to_borrower(100).is_err());
    assert!(escrow.repay(100).is_err());
}

#[test]
fn cannot_dispute_completed_escrow() {
    let t = terms(1_000_000, 30);
    let total = t.total_owed;
    let mut escrow = CreditEscrow::create("l".into(), "b".into(), t);
    escrow.fund(1_000_000).unwrap();
    escrow.release_to_borrower(1_000_000).unwrap();
    escrow.repay(total).unwrap();

    let result = escrow.dispute("Too late");
    assert!(result.is_err());
}

#[test]
fn cannot_double_dispute() {
    let t = terms(1_000_000, 30);
    let mut escrow = CreditEscrow::create("l".into(), "b".into(), t);
    escrow.fund(1_000_000).unwrap();
    escrow.release_to_borrower(1_000_000).unwrap();
    escrow.dispute("First dispute").unwrap();

    let result = escrow.dispute("Second dispute");
    assert!(result.is_err());
}

// ---------------------------------------------------------------------------
// Serialization
// ---------------------------------------------------------------------------

#[test]
fn escrow_serialization_roundtrip() {
    let t = terms(5_000_000, 90);
    let escrow = CreditEscrow::create("lender_pk".into(), "borrower_pk".into(), t);

    let json = serde_json::to_string(&escrow).unwrap();
    let restored: CreditEscrow = serde_json::from_str(&json).unwrap();

    assert_eq!(escrow.escrow_id, restored.escrow_id);
    assert_eq!(escrow.lender, restored.lender);
    assert_eq!(escrow.borrower, restored.borrower);
    assert_eq!(escrow.principal, restored.principal);
    assert_eq!(escrow.status, restored.status);
}
