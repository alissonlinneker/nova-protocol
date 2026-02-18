//! # Credit Line Management
//!
//! Credit lines are a first-class primitive in NOVA. Unlike traditional DeFi
//! lending protocols that require over-collateralization, NOVA supports
//! under-collateralized credit based on reputation scoring (handled by the
//! [`crate::credit`] module) and bilateral agreements between providers
//! and borrowers.
//!
//! A [`CreditLine`] is a revocable, time-bounded commitment from a provider
//! to lend up to a fixed limit at an agreed interest rate. The borrower can
//! draw against the line incrementally and repay on their own schedule,
//! as long as they stay within the limit and the line hasn't expired.
//!
//! A [`CreditLineManager`] aggregates multiple credit lines for a single
//! borrower and provides selection logic (e.g., cheapest available line).
//!
//! ## State Machine
//!
//! ```text
//!    ┌──────────┐
//!    │  Active   │ ← normal operating state
//!    └────┬──┬───┘
//!         │  │
//!   freeze│  │ close / expire
//!         │  │
//!    ┌────▼──┘   ┌──────────┐
//!    │  Frozen │──►│  Closed  │ ← terminal state, no further draws
//!    └────┬───┘   └──────────┘
//!         │
//!    default│ (missed payment deadline)
//!         │
//!    ┌────▼──────┐
//!    │ Defaulted  │ ← terminal state, triggers collection process
//!    └────────────┘
//! ```
//!
//! ## Interest Model
//!
//! Interest rates are expressed in **basis points** (bps). 1 bp = 0.01%.
//! So 500 bps = 5.00%. This avoids floating-point entirely and gives us
//! 0.01% granularity, which is more than sufficient for credit products.

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;
use uuid::Uuid;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during credit line operations.
#[derive(Debug, Error)]
pub enum CreditError {
    /// Attempted to draw more than the available credit.
    #[error(
        "credit limit exceeded: available {available}, requested {requested} (line {line_id})"
    )]
    LimitExceeded {
        /// The credit line ID.
        line_id: Uuid,
        /// Currently available credit (limit - used).
        available: u64,
        /// The draw amount that was rejected.
        requested: u64,
    },

    /// Attempted to draw on an inactive credit line.
    #[error("credit line {line_id} is not active (status: {status:?})")]
    NotActive {
        /// The credit line ID.
        line_id: Uuid,
        /// Current status of the line.
        status: CreditLineStatus,
    },

    /// Attempted to draw on an expired credit line.
    #[error("credit line {line_id} expired at {expired_at}")]
    Expired {
        /// The credit line ID.
        line_id: Uuid,
        /// When the line expired.
        expired_at: DateTime<Utc>,
    },

    /// Attempted to repay more than the outstanding balance.
    #[error("repayment {repayment} exceeds outstanding balance {outstanding} (line {line_id})")]
    OverRepayment {
        /// The credit line ID.
        line_id: Uuid,
        /// Current outstanding (used) amount.
        outstanding: u64,
        /// The repayment amount that was rejected.
        repayment: u64,
    },

    /// No credit line found that satisfies the requested criteria.
    #[error("no eligible credit line found for amount {0}")]
    NoEligibleLine(u64),

    /// The credit line has already been closed.
    #[error("credit line {0} is already closed")]
    AlreadyClosed(Uuid),
}

// ---------------------------------------------------------------------------
// CreditLineStatus
// ---------------------------------------------------------------------------

/// Lifecycle status of a credit line.
///
/// State transitions are unidirectional: once a line moves to `Closed` or
/// `Defaulted`, it cannot be reactivated. `Frozen` lines can be unfrozen
/// back to `Active` by the provider.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum CreditLineStatus {
    /// Line is open for draws and repayments.
    Active,

    /// Line is temporarily suspended. No new draws allowed, but
    /// repayments are still accepted. Can transition back to `Active`.
    Frozen,

    /// Line is permanently closed. No further operations allowed.
    /// Outstanding balance (if any) must still be repaid.
    Closed,

    /// Borrower has missed a payment deadline or violated terms.
    /// Triggers the collection/dispute resolution process.
    Defaulted,
}

impl CreditLineStatus {
    /// Returns `true` if the status allows new draws.
    pub fn allows_draws(&self) -> bool {
        matches!(self, CreditLineStatus::Active)
    }

    /// Returns `true` if the status allows repayments.
    pub fn allows_repayments(&self) -> bool {
        matches!(
            self,
            CreditLineStatus::Active | CreditLineStatus::Frozen | CreditLineStatus::Defaulted
        )
    }

    /// Returns `true` if this is a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(self, CreditLineStatus::Closed | CreditLineStatus::Defaulted)
    }
}

// ---------------------------------------------------------------------------
// CreditLine
// ---------------------------------------------------------------------------

/// A single credit line between a provider and a borrower.
///
/// The credit line defines the maximum amount the borrower can draw,
/// the interest rate, the term, and the current utilization. All amounts
/// are in the smallest unit of the associated token (specified externally
/// by the agreement — the credit line itself is token-agnostic).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreditLine {
    /// Unique identifier for this credit line.
    pub id: Uuid,

    /// NOVA address of the credit provider (lender).
    pub provider: String,

    /// NOVA address of the borrower.
    pub borrower: String,

    /// Maximum amount that can be drawn (in smallest units).
    pub limit: u64,

    /// Amount currently drawn and outstanding (in smallest units).
    ///
    /// Invariant: `used <= limit` (enforced by [`draw`](Self::draw)).
    pub used: u64,

    /// Annual interest rate in basis points.
    ///
    /// Example: 500 = 5.00% APR. Interest accrual is computed
    /// off-chain by the settlement engine; this field is the
    /// contractual rate used for those calculations.
    pub interest_rate_bps: u32,

    /// Credit term in days from `created_at`.
    ///
    /// After `created_at + term_days`, the line expires and no
    /// further draws are allowed. Outstanding balances remain due.
    pub term_days: u32,

    /// When this credit line was created.
    pub created_at: DateTime<Utc>,

    /// When this credit line expires (`created_at + term_days`).
    pub expires_at: DateTime<Utc>,

    /// Current lifecycle status.
    pub status: CreditLineStatus,
}

impl CreditLine {
    /// Creates a new active credit line.
    ///
    /// # Arguments
    ///
    /// * `provider` — Lender's NOVA address.
    /// * `borrower` — Borrower's NOVA address.
    /// * `limit` — Maximum drawable amount.
    /// * `interest_rate_bps` — Annual rate in basis points (e.g., 500 = 5%).
    /// * `term_days` — Duration of the credit line in days.
    pub fn new(
        provider: &str,
        borrower: &str,
        limit: u64,
        interest_rate_bps: u32,
        term_days: u32,
    ) -> Self {
        let now = Utc::now();
        let expires_at = now + chrono::Duration::days(term_days as i64);

        Self {
            id: Uuid::new_v4(),
            provider: provider.to_string(),
            borrower: borrower.to_string(),
            limit,
            used: 0,
            interest_rate_bps,
            term_days,
            created_at: now,
            expires_at,
            status: CreditLineStatus::Active,
        }
    }

    /// Returns the amount of credit currently available for drawing.
    ///
    /// This is simply `limit - used`. Returns 0 if the line is fully
    /// utilized or if the status doesn't allow draws.
    pub fn available(&self) -> u64 {
        if !self.status.allows_draws() {
            return 0;
        }
        self.limit.saturating_sub(self.used)
    }

    /// Returns the utilization ratio as a percentage (0-100).
    ///
    /// A utilization of 80%+ may trigger risk alerts in the credit
    /// scoring module.
    pub fn utilization_pct(&self) -> f64 {
        if self.limit == 0 {
            return 0.0;
        }
        (self.used as f64 / self.limit as f64) * 100.0
    }

    /// Returns `true` if the credit line has expired based on the
    /// current wall-clock time.
    pub fn is_expired(&self) -> bool {
        Utc::now() > self.expires_at
    }

    /// Returns the interest rate as a human-readable percentage string.
    ///
    /// Example: 500 bps -> "5.00%"
    pub fn interest_rate_display(&self) -> String {
        let pct = self.interest_rate_bps as f64 / 100.0;
        format!("{:.2}%", pct)
    }

    /// Draws (borrows) against this credit line.
    ///
    /// Increases `used` by `amount`. The caller is responsible for
    /// crediting the corresponding funds to the borrower's wallet.
    ///
    /// # Errors
    ///
    /// - [`CreditError::NotActive`] if the line status doesn't allow draws.
    /// - [`CreditError::Expired`] if the line has passed its expiry date.
    /// - [`CreditError::LimitExceeded`] if `amount > available()`.
    pub fn draw(&mut self, amount: u64) -> Result<u64, CreditError> {
        // Check status.
        if !self.status.allows_draws() {
            return Err(CreditError::NotActive {
                line_id: self.id,
                status: self.status,
            });
        }

        // Check expiry.
        if self.is_expired() {
            return Err(CreditError::Expired {
                line_id: self.id,
                expired_at: self.expires_at,
            });
        }

        // Check limit.
        let avail = self.available();
        if amount > avail {
            return Err(CreditError::LimitExceeded {
                line_id: self.id,
                available: avail,
                requested: amount,
            });
        }

        self.used += amount;
        Ok(self.available())
    }

    /// Repays (reduces) the outstanding balance on this credit line.
    ///
    /// Repayments are accepted in any status except `Closed` (where
    /// the balance has already been settled).
    ///
    /// # Errors
    ///
    /// - [`CreditError::AlreadyClosed`] if the line is closed.
    /// - [`CreditError::OverRepayment`] if `amount > used`.
    pub fn repay(&mut self, amount: u64) -> Result<u64, CreditError> {
        if self.status == CreditLineStatus::Closed {
            return Err(CreditError::AlreadyClosed(self.id));
        }

        if amount > self.used {
            return Err(CreditError::OverRepayment {
                line_id: self.id,
                outstanding: self.used,
                repayment: amount,
            });
        }

        self.used -= amount;
        Ok(self.used)
    }

    /// Freezes the credit line, preventing new draws but allowing repayments.
    ///
    /// Typically invoked by the provider when risk thresholds are breached
    /// or during a compliance review.
    pub fn freeze(&mut self) -> Result<(), CreditError> {
        if self.status.is_terminal() {
            return Err(CreditError::AlreadyClosed(self.id));
        }
        self.status = CreditLineStatus::Frozen;
        Ok(())
    }

    /// Unfreezes a frozen credit line back to active status.
    ///
    /// Only valid when current status is `Frozen`.
    pub fn unfreeze(&mut self) -> Result<(), CreditError> {
        if self.status != CreditLineStatus::Frozen {
            return Err(CreditError::NotActive {
                line_id: self.id,
                status: self.status,
            });
        }
        self.status = CreditLineStatus::Active;
        Ok(())
    }

    /// Closes the credit line permanently.
    ///
    /// Should only be called when the outstanding balance is zero.
    /// If there's still an outstanding balance, the settlement engine
    /// should handle final settlement before calling this.
    pub fn close(&mut self) -> Result<(), CreditError> {
        if self.status == CreditLineStatus::Closed {
            return Err(CreditError::AlreadyClosed(self.id));
        }
        self.status = CreditLineStatus::Closed;
        Ok(())
    }

    /// Marks the credit line as defaulted.
    ///
    /// This is a terminal state triggered by missed payment deadlines
    /// or terms violations. Once defaulted, the line cannot be reactivated
    /// — it enters the collection/dispute resolution pipeline.
    pub fn default_line(&mut self) -> Result<(), CreditError> {
        if self.status.is_terminal() {
            return Err(CreditError::AlreadyClosed(self.id));
        }
        self.status = CreditLineStatus::Defaulted;
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// CreditLineManager
// ---------------------------------------------------------------------------

/// Manages multiple credit lines for a single borrower wallet.
///
/// Provides aggregation queries (total available credit, total outstanding)
/// and selection logic for choosing the best line to draw against.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct CreditLineManager {
    /// All credit lines for this borrower, indexed by UUID.
    lines: Vec<CreditLine>,
}

impl CreditLineManager {
    /// Creates a new empty credit line manager.
    pub fn new() -> Self {
        Self { lines: Vec::new() }
    }

    /// Adds a credit line to the manager.
    pub fn add_line(&mut self, line: CreditLine) {
        self.lines.push(line);
    }

    /// Removes a closed credit line by ID.
    ///
    /// Returns the removed line, or `None` if not found.
    /// Only closed lines can be removed — active/frozen/defaulted lines
    /// must remain for accounting purposes.
    pub fn remove_closed_line(&mut self, line_id: &Uuid) -> Option<CreditLine> {
        if let Some(pos) = self
            .lines
            .iter()
            .position(|l| l.id == *line_id && l.status == CreditLineStatus::Closed)
        {
            Some(self.lines.remove(pos))
        } else {
            None
        }
    }

    /// Returns a reference to a credit line by ID.
    pub fn get_line(&self, line_id: &Uuid) -> Option<&CreditLine> {
        self.lines.iter().find(|l| l.id == *line_id)
    }

    /// Returns a mutable reference to a credit line by ID.
    pub fn get_line_mut(&mut self, line_id: &Uuid) -> Option<&mut CreditLine> {
        self.lines.iter_mut().find(|l| l.id == *line_id)
    }

    /// Returns the total number of credit lines (all statuses).
    pub fn line_count(&self) -> usize {
        self.lines.len()
    }

    /// Returns the total available credit across all active, non-expired lines.
    pub fn total_available(&self) -> u64 {
        self.lines
            .iter()
            .filter(|l| l.status.allows_draws() && !l.is_expired())
            .map(|l| l.available())
            .sum()
    }

    /// Returns the total outstanding (used) amount across all lines.
    pub fn total_outstanding(&self) -> u64 {
        self.lines.iter().map(|l| l.used).sum()
    }

    /// Returns the number of active (non-terminal, non-expired) credit lines.
    pub fn active_line_count(&self) -> usize {
        self.lines
            .iter()
            .filter(|l| l.status == CreditLineStatus::Active && !l.is_expired())
            .count()
    }

    /// Finds the cheapest credit line with sufficient available credit
    /// to cover the requested `amount`.
    ///
    /// "Cheapest" means the lowest `interest_rate_bps`. Among lines with
    /// equal rates, the one with the most available credit is preferred
    /// (to minimize utilization ratio impact).
    ///
    /// Only considers lines that are:
    /// - Status: `Active`
    /// - Not expired
    /// - Have `available() >= amount`
    ///
    /// Returns `None` if no eligible line exists.
    pub fn best_available_line(&self, amount: u64) -> Option<&CreditLine> {
        self.lines
            .iter()
            .filter(|l| l.status.allows_draws() && !l.is_expired() && l.available() >= amount)
            .min_by(|a, b| {
                // Primary sort: lowest interest rate.
                a.interest_rate_bps
                    .cmp(&b.interest_rate_bps)
                    // Secondary sort: most available credit (descending).
                    .then_with(|| b.available().cmp(&a.available()))
            })
    }

    /// Finds the cheapest line and draws against it.
    ///
    /// Convenience method that combines [`best_available_line`](Self::best_available_line)
    /// and [`CreditLine::draw`].
    ///
    /// # Returns
    ///
    /// The UUID of the line that was drawn against, and the remaining
    /// available credit on that line.
    ///
    /// # Errors
    ///
    /// Returns [`CreditError::NoEligibleLine`] if no line can cover the amount.
    pub fn draw_best_available(&mut self, amount: u64) -> Result<(Uuid, u64), CreditError> {
        // Find the best line's ID first (to avoid borrow issues).
        let line_id = self
            .best_available_line(amount)
            .map(|l| l.id)
            .ok_or(CreditError::NoEligibleLine(amount))?;

        let line = self.get_line_mut(&line_id).unwrap();
        let remaining = line.draw(amount)?;

        Ok((line_id, remaining))
    }

    /// Returns all credit lines (immutable slice).
    pub fn all_lines(&self) -> &[CreditLine] {
        &self.lines
    }

    /// Returns all credit lines from a specific provider.
    pub fn lines_from_provider(&self, provider: &str) -> Vec<&CreditLine> {
        self.lines
            .iter()
            .filter(|l| l.provider == provider)
            .collect()
    }

    /// Returns the weighted average interest rate across all active lines,
    /// weighted by limit size.
    ///
    /// Returns `None` if there are no active lines.
    pub fn weighted_avg_rate_bps(&self) -> Option<u32> {
        let active_lines: Vec<&CreditLine> = self
            .lines
            .iter()
            .filter(|l| l.status == CreditLineStatus::Active)
            .collect();

        if active_lines.is_empty() {
            return None;
        }

        let total_limit: u64 = active_lines.iter().map(|l| l.limit).sum();
        if total_limit == 0 {
            return None;
        }

        let weighted_sum: u64 = active_lines
            .iter()
            .map(|l| l.limit * l.interest_rate_bps as u64)
            .sum();

        Some((weighted_sum / total_limit) as u32)
    }
}

impl Default for CreditLineManager {
    fn default() -> Self {
        Self::new()
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    const PROVIDER: &str = "nova:bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    const BORROWER: &str = "nova:cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";

    fn make_line(limit: u64, rate_bps: u32, term_days: u32) -> CreditLine {
        CreditLine::new(PROVIDER, BORROWER, limit, rate_bps, term_days)
    }

    // -- CreditLine tests --

    #[test]
    fn new_line_is_active_with_full_availability() {
        let line = make_line(10_000, 500, 365);
        assert_eq!(line.status, CreditLineStatus::Active);
        assert_eq!(line.available(), 10_000);
        assert_eq!(line.used, 0);
        assert!(!line.is_expired());
    }

    #[test]
    fn draw_reduces_availability() {
        let mut line = make_line(10_000, 500, 365);
        let remaining = line.draw(3_000).unwrap();
        assert_eq!(remaining, 7_000);
        assert_eq!(line.used, 3_000);
        assert_eq!(line.available(), 7_000);
    }

    #[test]
    fn draw_full_limit() {
        let mut line = make_line(5_000, 500, 365);
        let remaining = line.draw(5_000).unwrap();
        assert_eq!(remaining, 0);
        assert_eq!(line.used, 5_000);
        assert_eq!(line.available(), 0);
    }

    #[test]
    fn draw_exceeds_limit_rejected() {
        let mut line = make_line(5_000, 500, 365);
        let result = line.draw(5_001);
        assert!(matches!(result, Err(CreditError::LimitExceeded { .. })));
        assert_eq!(line.used, 0, "failed draw must not change state");
    }

    #[test]
    fn draw_on_frozen_line_rejected() {
        let mut line = make_line(10_000, 500, 365);
        line.freeze().unwrap();
        let result = line.draw(1_000);
        assert!(matches!(result, Err(CreditError::NotActive { .. })));
    }

    #[test]
    fn repay_reduces_outstanding() {
        let mut line = make_line(10_000, 500, 365);
        line.draw(5_000).unwrap();

        let outstanding = line.repay(2_000).unwrap();
        assert_eq!(outstanding, 3_000);
        assert_eq!(line.available(), 7_000);
    }

    #[test]
    fn repay_full_outstanding() {
        let mut line = make_line(10_000, 500, 365);
        line.draw(5_000).unwrap();

        let outstanding = line.repay(5_000).unwrap();
        assert_eq!(outstanding, 0);
        assert_eq!(line.available(), 10_000);
    }

    #[test]
    fn over_repayment_rejected() {
        let mut line = make_line(10_000, 500, 365);
        line.draw(3_000).unwrap();

        let result = line.repay(3_001);
        assert!(matches!(result, Err(CreditError::OverRepayment { .. })));
    }

    #[test]
    fn repay_on_frozen_line_allowed() {
        let mut line = make_line(10_000, 500, 365);
        line.draw(5_000).unwrap();
        line.freeze().unwrap();

        // Repayments should still work when frozen.
        let outstanding = line.repay(2_000).unwrap();
        assert_eq!(outstanding, 3_000);
    }

    #[test]
    fn repay_on_closed_line_rejected() {
        let mut line = make_line(10_000, 500, 365);
        line.close().unwrap();

        let result = line.repay(100);
        assert!(matches!(result, Err(CreditError::AlreadyClosed(_))));
    }

    #[test]
    fn freeze_and_unfreeze() {
        let mut line = make_line(10_000, 500, 365);

        line.freeze().unwrap();
        assert_eq!(line.status, CreditLineStatus::Frozen);
        assert_eq!(line.available(), 0);

        line.unfreeze().unwrap();
        assert_eq!(line.status, CreditLineStatus::Active);
        assert_eq!(line.available(), 10_000);
    }

    #[test]
    fn close_is_terminal() {
        let mut line = make_line(10_000, 500, 365);
        line.close().unwrap();
        assert!(line.status.is_terminal());

        // Double close fails.
        assert!(line.close().is_err());
        // Can't freeze a closed line.
        assert!(line.freeze().is_err());
    }

    #[test]
    fn default_is_terminal() {
        let mut line = make_line(10_000, 500, 365);
        line.default_line().unwrap();
        assert_eq!(line.status, CreditLineStatus::Defaulted);
        assert!(line.status.is_terminal());
    }

    #[test]
    fn repay_on_defaulted_line_allowed() {
        let mut line = make_line(10_000, 500, 365);
        line.draw(5_000).unwrap();
        line.default_line().unwrap();

        // Even defaulted lines should accept repayments.
        let outstanding = line.repay(5_000).unwrap();
        assert_eq!(outstanding, 0);
    }

    #[test]
    fn utilization_percentage() {
        let mut line = make_line(10_000, 500, 365);
        assert_eq!(line.utilization_pct(), 0.0);

        line.draw(5_000).unwrap();
        assert!((line.utilization_pct() - 50.0).abs() < 0.01);

        line.draw(5_000).unwrap();
        assert!((line.utilization_pct() - 100.0).abs() < 0.01);
    }

    #[test]
    fn interest_rate_display() {
        let line = make_line(10_000, 500, 365);
        assert_eq!(line.interest_rate_display(), "5.00%");

        let line2 = make_line(10_000, 1250, 365);
        assert_eq!(line2.interest_rate_display(), "12.50%");

        let line3 = make_line(10_000, 50, 365);
        assert_eq!(line3.interest_rate_display(), "0.50%");
    }

    // -- CreditLineManager tests --

    #[test]
    fn manager_total_available() {
        let mut mgr = CreditLineManager::new();
        mgr.add_line(make_line(10_000, 500, 365));
        mgr.add_line(make_line(5_000, 300, 365));

        assert_eq!(mgr.total_available(), 15_000);
    }

    #[test]
    fn manager_total_outstanding() {
        let mut mgr = CreditLineManager::new();
        let mut l1 = make_line(10_000, 500, 365);
        l1.draw(3_000).unwrap();
        let mut l2 = make_line(5_000, 300, 365);
        l2.draw(1_000).unwrap();

        mgr.add_line(l1);
        mgr.add_line(l2);

        assert_eq!(mgr.total_outstanding(), 4_000);
    }

    #[test]
    fn best_available_line_picks_cheapest() {
        let mut mgr = CreditLineManager::new();
        mgr.add_line(make_line(10_000, 800, 365)); // 8.00%
        mgr.add_line(make_line(10_000, 300, 365)); // 3.00% <- cheapest
        mgr.add_line(make_line(10_000, 500, 365)); // 5.00%

        let best = mgr.best_available_line(5_000).unwrap();
        assert_eq!(best.interest_rate_bps, 300);
    }

    #[test]
    fn best_available_line_skips_insufficient() {
        let mut mgr = CreditLineManager::new();
        mgr.add_line(make_line(1_000, 200, 365)); // cheap but too small
        mgr.add_line(make_line(10_000, 500, 365)); // sufficient

        let best = mgr.best_available_line(5_000).unwrap();
        assert_eq!(best.interest_rate_bps, 500);
    }

    #[test]
    fn best_available_line_skips_frozen() {
        let mut mgr = CreditLineManager::new();
        let mut frozen_line = make_line(10_000, 200, 365);
        frozen_line.freeze().unwrap();
        mgr.add_line(frozen_line);
        mgr.add_line(make_line(10_000, 500, 365));

        let best = mgr.best_available_line(5_000).unwrap();
        assert_eq!(best.interest_rate_bps, 500);
    }

    #[test]
    fn best_available_line_none_when_all_insufficient() {
        let mut mgr = CreditLineManager::new();
        mgr.add_line(make_line(1_000, 200, 365));
        mgr.add_line(make_line(2_000, 300, 365));

        assert!(mgr.best_available_line(5_000).is_none());
    }

    #[test]
    fn best_available_line_tiebreaker_by_capacity() {
        let mut mgr = CreditLineManager::new();
        mgr.add_line(make_line(5_000, 500, 365)); // same rate, less capacity
        mgr.add_line(make_line(20_000, 500, 365)); // same rate, more capacity

        let best = mgr.best_available_line(1_000).unwrap();
        assert_eq!(best.limit, 20_000);
    }

    #[test]
    fn draw_best_available() {
        let mut mgr = CreditLineManager::new();
        mgr.add_line(make_line(10_000, 800, 365));
        mgr.add_line(make_line(10_000, 300, 365)); // cheapest

        let (line_id, remaining) = mgr.draw_best_available(5_000).unwrap();
        let drawn_line = mgr.get_line(&line_id).unwrap();
        assert_eq!(drawn_line.interest_rate_bps, 300);
        assert_eq!(drawn_line.used, 5_000);
        assert_eq!(remaining, 5_000);
    }

    #[test]
    fn draw_best_available_no_eligible() {
        let mut mgr = CreditLineManager::new();
        mgr.add_line(make_line(1_000, 300, 365));

        let result = mgr.draw_best_available(5_000);
        assert!(matches!(result, Err(CreditError::NoEligibleLine(5_000))));
    }

    #[test]
    fn manager_active_line_count() {
        let mut mgr = CreditLineManager::new();
        mgr.add_line(make_line(10_000, 500, 365));

        let mut frozen = make_line(5_000, 300, 365);
        frozen.freeze().unwrap();
        mgr.add_line(frozen);

        let mut closed = make_line(3_000, 200, 365);
        closed.close().unwrap();
        mgr.add_line(closed);

        assert_eq!(mgr.line_count(), 3);
        assert_eq!(mgr.active_line_count(), 1);
    }

    #[test]
    fn remove_closed_line() {
        let mut mgr = CreditLineManager::new();
        let mut line = make_line(10_000, 500, 365);
        line.close().unwrap();
        let id = line.id;
        mgr.add_line(line);

        let removed = mgr.remove_closed_line(&id);
        assert!(removed.is_some());
        assert_eq!(mgr.line_count(), 0);
    }

    #[test]
    fn cannot_remove_active_line() {
        let mut mgr = CreditLineManager::new();
        let line = make_line(10_000, 500, 365);
        let id = line.id;
        mgr.add_line(line);

        let removed = mgr.remove_closed_line(&id);
        assert!(removed.is_none());
        assert_eq!(mgr.line_count(), 1);
    }

    #[test]
    fn weighted_avg_rate() {
        let mut mgr = CreditLineManager::new();
        // Line 1: limit 10,000 at 500 bps (weighted: 5,000,000)
        mgr.add_line(make_line(10_000, 500, 365));
        // Line 2: limit 20,000 at 800 bps (weighted: 16,000,000)
        mgr.add_line(make_line(20_000, 800, 365));
        // Total limit: 30,000. Weighted sum: 21,000,000.
        // Weighted avg: 21,000,000 / 30,000 = 700 bps.

        assert_eq!(mgr.weighted_avg_rate_bps(), Some(700));
    }

    #[test]
    fn weighted_avg_rate_empty() {
        let mgr = CreditLineManager::new();
        assert_eq!(mgr.weighted_avg_rate_bps(), None);
    }

    #[test]
    fn lines_from_provider() {
        let mut mgr = CreditLineManager::new();
        let other_provider =
            "nova:dddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddddd";
        mgr.add_line(make_line(10_000, 500, 365));
        mgr.add_line(CreditLine::new(other_provider, BORROWER, 5_000, 300, 365));

        let from_default = mgr.lines_from_provider(PROVIDER);
        assert_eq!(from_default.len(), 1);

        let from_other = mgr.lines_from_provider(other_provider);
        assert_eq!(from_other.len(), 1);
    }

    #[test]
    fn credit_line_serialization_roundtrip() {
        let mut line = make_line(10_000, 500, 365);
        line.draw(3_000).unwrap();

        let json = serde_json::to_string(&line).expect("serialize");
        let recovered: CreditLine = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(recovered.limit, 10_000);
        assert_eq!(recovered.used, 3_000);
        assert_eq!(recovered.interest_rate_bps, 500);
        assert_eq!(recovered.status, CreditLineStatus::Active);
    }

    #[test]
    fn manager_serialization_roundtrip() {
        let mut mgr = CreditLineManager::new();
        mgr.add_line(make_line(10_000, 500, 365));
        mgr.add_line(make_line(5_000, 300, 365));

        let json = serde_json::to_string(&mgr).expect("serialize");
        let recovered: CreditLineManager = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(recovered.line_count(), 2);
        assert_eq!(recovered.total_available(), 15_000);
    }
}
