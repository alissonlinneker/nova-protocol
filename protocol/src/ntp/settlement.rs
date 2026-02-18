//! # NTP Step 4 -- Validation and Settlement
//!
//! After a signed transaction is broadcast to the network, validators
//! pick it up from the mempool and run a sequence of checks:
//!
//! 1. **Structural validation** -- size limits, fee minimums, memo length.
//! 2. **Signature verification** -- Ed25519 signature over the tx body.
//! 3. **ZKP verification** -- if the transaction carries a balance proof,
//!    verify it against the sender's on-chain commitment.
//! 4. **State transition** -- debit sender, credit receiver, update nonces.
//!
//! If all checks pass, the transaction is included in the next block.
//! If any check fails, the transaction is rejected with a reason.
//!
//! ## Settlement Results
//!
//! The outcome is one of three states:
//!
//! - **Confirmed** -- included in a block at a specific height.
//! - **Rejected** -- failed validation with a reason.
//! - **TimedOut** -- not included within the finality timeout.

use serde::{Deserialize, Serialize};
use std::time::Instant;

use crate::config;
use crate::transaction::builder::Transaction;
use crate::transaction::verification::{verify_transaction, TransactionError};

// ---------------------------------------------------------------------------
// Settlement Result
// ---------------------------------------------------------------------------

/// The final outcome of transaction settlement.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum SettlementResult {
    /// Transaction was included in a finalized block.
    Confirmed {
        /// Block height where the transaction was included.
        block_height: u64,
        /// Transaction hash (hex-encoded).
        tx_hash: String,
        /// Block hash (hex-encoded).
        block_hash: String,
        /// Index of the transaction within the block.
        tx_index: u32,
        /// Block timestamp (milliseconds).
        block_timestamp: u64,
    },
    /// Transaction failed validation and was rejected.
    Rejected {
        /// Human-readable reason for the rejection.
        reason: String,
        /// The specific validation stage that failed.
        stage: ValidationStage,
    },
    /// Transaction was not settled within the finality timeout.
    TimedOut {
        /// Milliseconds elapsed before the timeout triggered.
        elapsed_ms: u64,
        /// Configured timeout in milliseconds.
        timeout_ms: u64,
    },
}

/// Which validation stage rejected the transaction.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub enum ValidationStage {
    /// Structural checks (size, fees, memo).
    Structural,
    /// Signature verification.
    Signature,
    /// Zero-knowledge proof verification.
    ZkpVerification,
    /// Balance / nonce / state transition.
    StateTransition,
}

// ---------------------------------------------------------------------------
// Validation Request
// ---------------------------------------------------------------------------

/// What a validator receives when it needs to process a transaction.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct ValidationRequest {
    /// The transaction to validate.
    pub transaction: Transaction,
    /// The session ID from the NTP flow (for audit correlation).
    pub session_id: Option<String>,
    /// Unix timestamp when the request entered the mempool.
    pub received_at: u64,
    /// Fee-per-byte for priority ordering.
    pub priority_score: u64,
}

impl ValidationRequest {
    /// Create a new validation request from a transaction.
    pub fn new(transaction: Transaction, session_id: Option<String>) -> Self {
        let priority_score = transaction.fee_per_byte();
        let received_at = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis() as u64;

        Self {
            transaction,
            session_id,
            received_at,
            priority_score,
        }
    }
}

// ---------------------------------------------------------------------------
// Validation Logic
// ---------------------------------------------------------------------------

/// Validate a transaction through the full validation pipeline.
///
/// Runs structural checks and signature presence verification.
/// In production, this would also verify ZKP proofs and check the
/// state tree for sufficient balances.
///
/// Returns `Ok(())` if the transaction passes all stateless checks.
/// Returns `Err(SettlementResult::Rejected { .. })` if any check fails.
pub fn validate_transaction(tx: &Transaction) -> Result<(), SettlementResult> {
    // Stage 1: Structural validation.
    match verify_transaction(tx) {
        Ok(()) => {}
        Err(e) => {
            return Err(SettlementResult::Rejected {
                reason: format!("{}", e),
                stage: match &e {
                    TransactionError::InvalidSignature { .. } => ValidationStage::Signature,
                    _ => ValidationStage::Structural,
                },
            });
        }
    }

    // Stage 2: Signature presence check.
    if tx.signature.is_none() {
        return Err(SettlementResult::Rejected {
            reason: "missing transaction signature".to_string(),
            stage: ValidationStage::Signature,
        });
    }

    Ok(())
}

// ---------------------------------------------------------------------------
// Settlement State Machine
// ---------------------------------------------------------------------------

/// Settlement state for tracking a transaction through the pipeline.
#[derive(Clone, Debug)]
pub enum SettlementState {
    /// Transaction submitted, waiting for validation.
    Pending,
    /// Currently being validated by the network.
    Validating,
    /// Terminal: confirmed in a block.
    Confirmed(SettlementResult),
    /// Terminal: rejected by validators.
    Rejected(SettlementResult),
    /// Terminal: not settled within the timeout window.
    TimedOut,
}

/// Tracks the lifecycle of a single transaction through settlement.
///
/// Created when a transaction is broadcast, updated as the network
/// processes it, and finalized when a result is received or the
/// timeout expires.
pub struct SettlementStateMachine {
    /// Current state.
    state: SettlementState,
    /// Transaction hash (hex) being tracked.
    tx_hash: String,
    /// Session ID (for NTP correlation).
    session_id: String,
    /// When tracking started.
    started_at: Instant,
    /// Finality timeout in milliseconds.
    timeout_ms: u64,
}

impl SettlementStateMachine {
    /// Create a new state machine for tracking a transaction.
    pub fn new(tx_hash: String, session_id: String) -> Self {
        Self {
            state: SettlementState::Pending,
            tx_hash,
            session_id,
            started_at: Instant::now(),
            timeout_ms: config::FINALITY_TIMEOUT.as_millis() as u64,
        }
    }

    /// Create with a custom timeout (for testing).
    pub fn with_timeout(tx_hash: String, session_id: String, timeout_ms: u64) -> Self {
        Self {
            state: SettlementState::Pending,
            tx_hash,
            session_id,
            started_at: Instant::now(),
            timeout_ms,
        }
    }

    /// Return the current settlement state.
    pub fn state(&self) -> &SettlementState {
        &self.state
    }

    /// Return the tracked transaction hash.
    pub fn tx_hash(&self) -> &str {
        &self.tx_hash
    }

    /// Return the session ID.
    pub fn session_id(&self) -> &str {
        &self.session_id
    }

    /// Transition to the validating state.
    pub fn mark_validating(&mut self) {
        if matches!(self.state, SettlementState::Pending) {
            self.state = SettlementState::Validating;
        }
    }

    /// Transition to the confirmed state.
    pub fn mark_confirmed(&mut self, result: SettlementResult) {
        match self.state {
            SettlementState::Pending | SettlementState::Validating => {
                self.state = SettlementState::Confirmed(result);
            }
            _ => {} // Terminal states are immutable.
        }
    }

    /// Transition to the rejected state.
    pub fn mark_rejected(&mut self, result: SettlementResult) {
        match self.state {
            SettlementState::Pending | SettlementState::Validating => {
                self.state = SettlementState::Rejected(result);
            }
            _ => {}
        }
    }

    /// Check if the settlement has timed out.
    ///
    /// If the timeout has elapsed and we are still pending/validating,
    /// transitions to `TimedOut` and returns the timeout result.
    pub fn check_timeout(&mut self) -> Option<SettlementResult> {
        let elapsed = self.started_at.elapsed().as_millis() as u64;

        match self.state {
            SettlementState::Pending | SettlementState::Validating => {
                if elapsed >= self.timeout_ms {
                    self.state = SettlementState::TimedOut;
                    Some(SettlementResult::TimedOut {
                        elapsed_ms: elapsed,
                        timeout_ms: self.timeout_ms,
                    })
                } else {
                    None
                }
            }
            _ => None,
        }
    }

    /// Return the final settlement result, if terminal.
    pub fn result(&self) -> Option<&SettlementResult> {
        match &self.state {
            SettlementState::Confirmed(r) | SettlementState::Rejected(r) => Some(r),
            _ => None,
        }
    }

    /// Returns `true` if the state machine is in a terminal state.
    pub fn is_terminal(&self) -> bool {
        matches!(
            self.state,
            SettlementState::Confirmed(_)
                | SettlementState::Rejected(_)
                | SettlementState::TimedOut
        )
    }

    /// Elapsed time in milliseconds since tracking started.
    pub fn elapsed_ms(&self) -> u64 {
        self.started_at.elapsed().as_millis() as u64
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::crypto::keys::NovaKeypair;
    use crate::identity::NovaId;
    use crate::transaction::builder::TransactionBuilder;
    use crate::transaction::signing::sign_transaction;
    use crate::transaction::types::{Amount, Currency, TransactionType};

    fn make_signed_tx(nonce: u64) -> (Transaction, NovaKeypair) {
        let kp = NovaKeypair::generate();
        let sender_addr = NovaId::from_public_key(&kp.public_key()).to_address();
        let receiver_kp = NovaKeypair::generate();
        let receiver_addr = NovaId::from_public_key(&receiver_kp.public_key()).to_address();

        let mut tx = TransactionBuilder::new(TransactionType::Transfer)
            .sender(&sender_addr)
            .receiver(&receiver_addr)
            .amount(Amount::new(1_000, Currency::NOVA))
            .fee(100)
            .nonce(nonce)
            .build();
        sign_transaction(&mut tx, &kp);
        (tx, kp)
    }

    #[test]
    fn valid_transaction_passes_validation() {
        let (tx, _) = make_signed_tx(1);
        let result = validate_transaction(&tx);
        assert!(result.is_ok());
    }

    #[test]
    fn missing_signature_rejected() {
        let kp = NovaKeypair::generate();
        let sender_addr = NovaId::from_public_key(&kp.public_key()).to_address();
        let receiver_kp = NovaKeypair::generate();
        let receiver_addr = NovaId::from_public_key(&receiver_kp.public_key()).to_address();

        let tx = TransactionBuilder::new(TransactionType::Transfer)
            .sender(&sender_addr)
            .receiver(&receiver_addr)
            .amount(Amount::new(1_000, Currency::NOVA))
            .fee(100)
            .nonce(1)
            .build();
        let result = validate_transaction(&tx);
        assert!(result.is_err());
    }

    #[test]
    fn state_machine_lifecycle() {
        let mut sm = SettlementStateMachine::new("abc123".to_string(), "session-1".to_string());

        assert!(matches!(sm.state(), SettlementState::Pending));
        assert!(!sm.is_terminal());

        sm.mark_validating();
        assert!(matches!(sm.state(), SettlementState::Validating));

        let confirmed = SettlementResult::Confirmed {
            block_height: 42,
            tx_hash: "abc123".to_string(),
            block_hash: "block456".to_string(),
            tx_index: 0,
            block_timestamp: 1000000,
        };
        sm.mark_confirmed(confirmed);

        assert!(sm.is_terminal());
        assert!(matches!(sm.state(), SettlementState::Confirmed(_)));
    }

    #[test]
    fn state_machine_timeout() {
        let mut sm =
            SettlementStateMachine::with_timeout("tx_hash".to_string(), "session-1".to_string(), 0);

        let timeout_result = sm.check_timeout();
        assert!(timeout_result.is_some());
        assert!(matches!(
            timeout_result.unwrap(),
            SettlementResult::TimedOut { .. }
        ));
        assert!(sm.is_terminal());
    }

    #[test]
    fn terminal_states_are_immutable() {
        let mut sm = SettlementStateMachine::new("tx".to_string(), "session".to_string());

        let confirmed = SettlementResult::Confirmed {
            block_height: 1,
            tx_hash: "tx".to_string(),
            block_hash: "block".to_string(),
            tx_index: 0,
            block_timestamp: 1,
        };
        sm.mark_confirmed(confirmed);

        let rejected = SettlementResult::Rejected {
            reason: "too late".to_string(),
            stage: ValidationStage::Structural,
        };
        sm.mark_rejected(rejected);

        // Still confirmed.
        assert!(matches!(sm.state(), SettlementState::Confirmed(_)));
    }
}
