//! # Credit Escrow Contract
//!
//! Implements a trustless escrow for credit operations between a lender and
//! a borrower. The lifecycle is:
//!
//! 1. **Create** — lender and borrower agree on terms (principal, interest,
//!    repayment deadline).
//! 2. **Fund** — lender deposits the principal into the escrow.
//! 3. **Release** — escrowed funds are released to the borrower (partial
//!    or full release supported).
//! 4. **Repay** — borrower repays principal + interest over time.
//! 5. **Complete** — all obligations met, escrow closes.
//!
//! At any point, either party can initiate a dispute (see [`super::dispute_resolution`]).
//! If the borrower misses the repayment deadline, the escrow transitions
//! to `Defaulted`.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during escrow operations.
#[derive(Debug, Error)]
pub enum EscrowError {
    /// The escrow is not in a state that allows this operation.
    #[error("invalid state transition: escrow is {current}, expected {expected}")]
    InvalidState {
        /// The escrow's current status.
        current: String,
        /// The status required for this operation.
        expected: String,
    },

    /// An arithmetic overflow would occur (e.g., funding beyond the principal).
    #[error("amount overflow: operation would exceed allowed limits")]
    AmountOverflow,

    /// The funding amount exceeds the remaining unfunded principal.
    #[error("overfunded: attempted to fund {attempted} but only {remaining} remains")]
    Overfunded {
        /// Amount the caller tried to deposit.
        attempted: u64,
        /// Amount still needed to fully fund the escrow.
        remaining: u64,
    },

    /// Tried to release more than the currently escrowed (funded minus already released) amount.
    #[error("insufficient escrowed funds: requested {requested}, available {available}")]
    InsufficientEscrowed {
        /// Amount the caller tried to release.
        requested: u64,
        /// Amount currently available in escrow.
        available: u64,
    },

    /// Tried to repay more than the outstanding obligation.
    #[error("overpayment: attempted to repay {attempted} but only {outstanding} outstanding")]
    Overpayment {
        /// Amount the caller tried to repay.
        attempted: u64,
        /// Remaining obligation.
        outstanding: u64,
    },

    /// A dispute has already been opened on this escrow.
    #[error("escrow already has an active dispute")]
    AlreadyDisputed,
}

// ---------------------------------------------------------------------------
// Types
// ---------------------------------------------------------------------------

/// The current status of a credit escrow.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
pub enum EscrowStatus {
    /// Created but not yet funded by the lender.
    Pending,
    /// Lender has deposited the full principal.
    Funded,
    /// Funds have been (at least partially) released to the borrower.
    /// Repayment is now expected.
    Active,
    /// All obligations fulfilled — principal + interest repaid in full.
    Completed,
    /// A dispute has been opened by either party.
    Disputed,
    /// The borrower missed the repayment deadline.
    Defaulted,
}

impl std::fmt::Display for EscrowStatus {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            EscrowStatus::Pending => write!(f, "Pending"),
            EscrowStatus::Funded => write!(f, "Funded"),
            EscrowStatus::Active => write!(f, "Active"),
            EscrowStatus::Completed => write!(f, "Completed"),
            EscrowStatus::Disputed => write!(f, "Disputed"),
            EscrowStatus::Defaulted => write!(f, "Defaulted"),
        }
    }
}

/// The agreed-upon terms of a credit escrow.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditTerms {
    /// The principal amount in photons.
    pub principal: u64,
    /// Annual interest rate expressed in basis points (1 bp = 0.01%).
    /// E.g., 500 = 5.00% APR.
    pub interest_rate_bps: u32,
    /// Total amount owed (principal + interest), pre-computed at creation.
    pub total_owed: u64,
    /// Deadline by which all repayments must be completed.
    pub repayment_deadline: DateTime<Utc>,
    /// Optional grace period in seconds after the deadline before
    /// the escrow is marked as defaulted.
    pub grace_period_secs: u64,
}

/// A credit escrow instance.
///
/// Tracks the full lifecycle of a lender-borrower credit arrangement,
/// from initial creation through funding, release, repayment, and
/// completion (or default/dispute).
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CreditEscrow {
    /// Unique identifier for this escrow.
    pub escrow_id: String,
    /// Hex-encoded public key of the lender (funds provider).
    pub lender: String,
    /// Hex-encoded public key of the borrower (funds recipient).
    pub borrower: String,
    /// The principal amount agreed upon.
    pub principal: u64,
    /// Total amount deposited by the lender so far.
    pub funded_amount: u64,
    /// Total amount released to the borrower so far.
    pub released_amount: u64,
    /// Total amount repaid by the borrower so far.
    pub repaid_amount: u64,
    /// The credit terms governing this escrow.
    pub terms: CreditTerms,
    /// Current lifecycle status.
    pub status: EscrowStatus,
    /// Timestamp when the escrow was created.
    pub created_at: DateTime<Utc>,
    /// Timestamp of the most recent state change.
    pub updated_at: DateTime<Utc>,
}

impl CreditEscrow {
    /// Creates a new escrow in `Pending` status.
    ///
    /// The escrow is not funded yet — the lender must call [`fund`](Self::fund)
    /// to deposit the principal before any funds can be released.
    ///
    /// # Arguments
    ///
    /// * `lender` - Hex-encoded public key of the lender.
    /// * `borrower` - Hex-encoded public key of the borrower.
    /// * `terms` - The agreed-upon credit terms.
    pub fn create(lender: String, borrower: String, terms: CreditTerms) -> Self {
        let now = Utc::now();
        let principal = terms.principal;
        Self {
            escrow_id: Uuid::new_v4().to_string(),
            lender,
            borrower,
            principal,
            funded_amount: 0,
            released_amount: 0,
            repaid_amount: 0,
            terms,
            status: EscrowStatus::Pending,
            created_at: now,
            updated_at: now,
        }
    }

    /// Lender deposits funds into the escrow.
    ///
    /// Can be called multiple times for partial funding. Once the full
    /// principal is deposited, the status transitions to `Funded`.
    ///
    /// # Errors
    ///
    /// Returns [`EscrowError::InvalidState`] if the escrow is not `Pending`.
    /// Returns [`EscrowError::Overfunded`] if the deposit would exceed the principal.
    pub fn fund(&mut self, amount: u64) -> Result<(), EscrowError> {
        if self.status != EscrowStatus::Pending {
            return Err(EscrowError::InvalidState {
                current: self.status.to_string(),
                expected: "Pending".into(),
            });
        }

        let remaining = self
            .principal
            .checked_sub(self.funded_amount)
            .ok_or(EscrowError::AmountOverflow)?;

        if amount > remaining {
            return Err(EscrowError::Overfunded {
                attempted: amount,
                remaining,
            });
        }

        self.funded_amount = self
            .funded_amount
            .checked_add(amount)
            .ok_or(EscrowError::AmountOverflow)?;

        if self.funded_amount == self.principal {
            self.status = EscrowStatus::Funded;
        }

        self.updated_at = Utc::now();
        Ok(())
    }

    /// Releases escrowed funds to the borrower.
    ///
    /// Transitions the escrow to `Active` on the first release. Supports
    /// partial releases — the caller specifies the amount to disburse.
    ///
    /// # Errors
    ///
    /// Returns [`EscrowError::InvalidState`] if the escrow is not `Funded` or `Active`.
    /// Returns [`EscrowError::InsufficientEscrowed`] if the requested amount
    /// exceeds what is currently held in escrow.
    pub fn release_to_borrower(&mut self, amount: u64) -> Result<(), EscrowError> {
        if self.status != EscrowStatus::Funded && self.status != EscrowStatus::Active {
            return Err(EscrowError::InvalidState {
                current: self.status.to_string(),
                expected: "Funded or Active".into(),
            });
        }

        let available = self
            .funded_amount
            .checked_sub(self.released_amount)
            .ok_or(EscrowError::AmountOverflow)?;

        if amount > available {
            return Err(EscrowError::InsufficientEscrowed {
                requested: amount,
                available,
            });
        }

        self.released_amount = self
            .released_amount
            .checked_add(amount)
            .ok_or(EscrowError::AmountOverflow)?;

        self.status = EscrowStatus::Active;
        self.updated_at = Utc::now();
        Ok(())
    }

    /// Borrower repays towards the obligation.
    ///
    /// Once the total repaid amount equals or exceeds `terms.total_owed`,
    /// the escrow transitions to `Completed`.
    ///
    /// # Errors
    ///
    /// Returns [`EscrowError::InvalidState`] if the escrow is not `Active`.
    /// Returns [`EscrowError::Overpayment`] if the repayment exceeds the
    /// remaining outstanding amount.
    pub fn repay(&mut self, amount: u64) -> Result<(), EscrowError> {
        if self.status != EscrowStatus::Active {
            return Err(EscrowError::InvalidState {
                current: self.status.to_string(),
                expected: "Active".into(),
            });
        }

        let outstanding = self
            .terms
            .total_owed
            .checked_sub(self.repaid_amount)
            .ok_or(EscrowError::AmountOverflow)?;

        if amount > outstanding {
            return Err(EscrowError::Overpayment {
                attempted: amount,
                outstanding,
            });
        }

        self.repaid_amount = self
            .repaid_amount
            .checked_add(amount)
            .ok_or(EscrowError::AmountOverflow)?;

        if self.repaid_amount >= self.terms.total_owed {
            self.status = EscrowStatus::Completed;
        }

        self.updated_at = Utc::now();
        Ok(())
    }

    /// Checks whether the escrow has passed its repayment deadline.
    ///
    /// Returns `true` if the deadline (plus grace period) has passed and
    /// the escrow is still `Active` with an outstanding balance. Also
    /// transitions the status to `Defaulted` as a side effect.
    pub fn check_default(&mut self) -> bool {
        if self.status != EscrowStatus::Active {
            return false;
        }

        let grace = chrono::Duration::seconds(self.terms.grace_period_secs as i64);
        let effective_deadline = self.terms.repayment_deadline + grace;
        let now = Utc::now();

        if now > effective_deadline && self.repaid_amount < self.terms.total_owed {
            self.status = EscrowStatus::Defaulted;
            self.updated_at = now;
            return true;
        }

        false
    }

    /// Initiates a dispute on this escrow.
    ///
    /// Freezes the escrow by transitioning it to `Disputed` status. The
    /// actual dispute proceedings are handled by [`super::dispute_resolution::Dispute`].
    ///
    /// # Arguments
    ///
    /// * `_reason` - Human-readable description of the dispute grounds.
    ///   Stored in the associated `Dispute` struct, not in the escrow itself.
    ///
    /// # Errors
    ///
    /// Returns [`EscrowError::InvalidState`] if the escrow is already
    /// `Completed`, `Defaulted`, or `Disputed`.
    pub fn dispute(&mut self, _reason: &str) -> Result<(), EscrowError> {
        match self.status {
            EscrowStatus::Completed | EscrowStatus::Defaulted => {
                return Err(EscrowError::InvalidState {
                    current: self.status.to_string(),
                    expected: "Pending, Funded, or Active".into(),
                });
            }
            EscrowStatus::Disputed => {
                return Err(EscrowError::AlreadyDisputed);
            }
            _ => {}
        }

        self.status = EscrowStatus::Disputed;
        self.updated_at = Utc::now();
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_terms(principal: u64) -> CreditTerms {
        CreditTerms {
            principal,
            interest_rate_bps: 500,
            total_owed: principal + (principal / 20), // 5% flat for simplicity
            repayment_deadline: Utc::now() + chrono::Duration::days(30),
            grace_period_secs: 86400, // 1 day
        }
    }

    #[test]
    fn create_escrow_starts_pending() {
        let terms = sample_terms(1_000_000);
        let escrow = CreditEscrow::create("lender_pk".into(), "borrower_pk".into(), terms);
        assert_eq!(escrow.status, EscrowStatus::Pending);
        assert_eq!(escrow.funded_amount, 0);
        assert_eq!(escrow.released_amount, 0);
        assert_eq!(escrow.repaid_amount, 0);
    }

    #[test]
    fn full_fund_transitions_to_funded() {
        let terms = sample_terms(1_000_000);
        let mut escrow = CreditEscrow::create("l".into(), "b".into(), terms);
        escrow.fund(1_000_000).unwrap();
        assert_eq!(escrow.status, EscrowStatus::Funded);
    }

    #[test]
    fn partial_fund_stays_pending() {
        let terms = sample_terms(1_000_000);
        let mut escrow = CreditEscrow::create("l".into(), "b".into(), terms);
        escrow.fund(500_000).unwrap();
        assert_eq!(escrow.status, EscrowStatus::Pending);
        assert_eq!(escrow.funded_amount, 500_000);
    }

    #[test]
    fn overfund_rejected() {
        let terms = sample_terms(1_000_000);
        let mut escrow = CreditEscrow::create("l".into(), "b".into(), terms);
        let result = escrow.fund(1_500_000);
        assert!(result.is_err());
    }

    #[test]
    fn release_transitions_to_active() {
        let terms = sample_terms(1_000_000);
        let mut escrow = CreditEscrow::create("l".into(), "b".into(), terms);
        escrow.fund(1_000_000).unwrap();
        escrow.release_to_borrower(500_000).unwrap();
        assert_eq!(escrow.status, EscrowStatus::Active);
        assert_eq!(escrow.released_amount, 500_000);
    }

    #[test]
    fn release_more_than_available_rejected() {
        let terms = sample_terms(1_000_000);
        let mut escrow = CreditEscrow::create("l".into(), "b".into(), terms);
        escrow.fund(1_000_000).unwrap();
        let result = escrow.release_to_borrower(1_500_000);
        assert!(result.is_err());
    }

    #[test]
    fn full_repayment_completes_escrow() {
        let terms = sample_terms(1_000_000);
        let total_owed = terms.total_owed;
        let mut escrow = CreditEscrow::create("l".into(), "b".into(), terms);
        escrow.fund(1_000_000).unwrap();
        escrow.release_to_borrower(1_000_000).unwrap();
        escrow.repay(total_owed).unwrap();
        assert_eq!(escrow.status, EscrowStatus::Completed);
    }

    #[test]
    fn overpayment_rejected() {
        let terms = sample_terms(1_000_000);
        let total_owed = terms.total_owed;
        let mut escrow = CreditEscrow::create("l".into(), "b".into(), terms);
        escrow.fund(1_000_000).unwrap();
        escrow.release_to_borrower(1_000_000).unwrap();
        let result = escrow.repay(total_owed + 1);
        assert!(result.is_err());
    }
}
