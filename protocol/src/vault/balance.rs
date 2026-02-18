//! # Balance Management with Pedersen Commitments
//!
//! Every token balance in NOVA is tracked as a pair: the plaintext `u64`
//! amount (for local computation) and its Pedersen commitment (for on-chain
//! privacy). The commitment is updated on every credit/debit operation so
//! that the wallet can produce ZK proofs at any time without re-committing.
//!
//! A [`BalanceSheet`] is the complete set of token balances for a single
//! wallet. It maps [`TokenId`] to [`Balance`] and enforces the invariant
//! that you can never spend more than you have (unless you have a credit
//! line -- that's handled in [`super::credit`]).

use std::collections::HashMap;

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use thiserror::Error;

use super::token::TokenId;
use crate::transaction::types::Currency;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during balance operations.
#[derive(Debug, Error)]
pub enum BalanceError {
    /// Attempted to debit more than the available balance.
    #[error(
        "insufficient balance: available {available}, requested {requested} (token {token_id})"
    )]
    InsufficientBalance {
        /// The token that was being debited.
        token_id: TokenId,
        /// The current balance.
        available: u64,
        /// The amount that was requested.
        requested: u64,
    },

    /// Arithmetic overflow during a credit operation.
    ///
    /// If you're hitting this, someone is trying to credit more than
    /// 18.4 quintillion units. That's either a bug or an attack.
    #[error("balance overflow: current {current}, credit {credit} (token {token_id})")]
    Overflow {
        /// The token that was being credited.
        token_id: TokenId,
        /// The current balance before the failed credit.
        current: u64,
        /// The amount that caused the overflow.
        credit: u64,
    },

    /// Attempted an operation on a token with no existing balance entry.
    #[error("no balance entry for token {0}")]
    TokenNotFound(TokenId),
}

// ---------------------------------------------------------------------------
// Balance
// ---------------------------------------------------------------------------

/// A single token balance with its associated Pedersen commitment.
///
/// The `amount` field is the plaintext balance used for local arithmetic.
/// The `committed_amount` is the compressed Pedersen commitment point,
/// stored as raw bytes so that the balance struct doesn't depend on
/// arkworks types at the serialization boundary.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Balance {
    /// The token this balance is for.
    pub token_id: TokenId,

    /// Plaintext balance in smallest units.
    ///
    /// This value is authoritative for local operations. The on-chain
    /// representation uses only the commitment.
    pub amount: u64,

    /// Pedersen commitment to `amount`: `C = amount * G + r * H`.
    ///
    /// Stored as compressed BN254/G1 point bytes. Updated on every
    /// credit/debit. The blinding factor `r` is managed by the wallet's
    /// key material and is NOT stored here.
    pub committed_amount: Vec<u8>,

    /// Timestamp of the last balance-modifying operation.
    pub last_updated: DateTime<Utc>,
}

impl Balance {
    /// Creates a new zero balance for the given token.
    ///
    /// The commitment is initialized to an empty byte vector. The first
    /// credit operation will compute the real commitment.
    pub fn new(token_id: TokenId) -> Self {
        Self {
            token_id,
            amount: 0,
            committed_amount: Vec::new(),
            last_updated: Utc::now(),
        }
    }

    /// Creates a balance with an explicit initial amount and commitment.
    pub fn with_amount(token_id: TokenId, amount: u64, commitment_bytes: Vec<u8>) -> Self {
        Self {
            token_id,
            amount,
            committed_amount: commitment_bytes,
            last_updated: Utc::now(),
        }
    }

    /// Returns `true` if this balance is zero.
    pub fn is_zero(&self) -> bool {
        self.amount == 0
    }
}

// ---------------------------------------------------------------------------
// BalanceSheet
// ---------------------------------------------------------------------------

/// The complete set of token balances for a single wallet.
///
/// Internally a `HashMap<TokenId, Balance>`. Provides credit/debit
/// operations that enforce non-negative balances and overflow protection.
/// Thread safety is handled at the [`Wallet`](super::wallet::Wallet) level
/// -- a `BalanceSheet` is not `Sync` by itself.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct BalanceSheet {
    /// Token balances indexed by token ID.
    #[serde(with = "crate::vault::token::token_id_map")]
    balances: HashMap<TokenId, Balance>,
}

impl BalanceSheet {
    /// Creates an empty balance sheet.
    pub fn new() -> Self {
        Self {
            balances: HashMap::new(),
        }
    }

    /// Credits (adds) funds to a token balance.
    ///
    /// If no balance entry exists for the given `token_id`, one is created
    /// automatically. The Pedersen commitment is updated using the provided
    /// `commitment_bytes` -- pass the output of `zkp::commitment::commit()`
    /// serialized to bytes.
    ///
    /// # Errors
    ///
    /// Returns [`BalanceError::Overflow`] if the credit would exceed `u64::MAX`.
    pub fn credit(
        &mut self,
        token_id: TokenId,
        amount: u64,
        commitment_bytes: Vec<u8>,
    ) -> Result<u64, BalanceError> {
        let balance = self
            .balances
            .entry(token_id)
            .or_insert_with(|| Balance::new(token_id));

        let new_amount = balance
            .amount
            .checked_add(amount)
            .ok_or(BalanceError::Overflow {
                token_id,
                current: balance.amount,
                credit: amount,
            })?;

        balance.amount = new_amount;
        balance.committed_amount = commitment_bytes;
        balance.last_updated = Utc::now();

        Ok(new_amount)
    }

    /// Debits (subtracts) funds from a token balance.
    ///
    /// The Pedersen commitment is updated to reflect the new balance.
    ///
    /// # Errors
    ///
    /// Returns [`BalanceError::InsufficientBalance`] if the debit exceeds
    /// the current balance. Returns [`BalanceError::TokenNotFound`] if
    /// no balance entry exists for the token.
    pub fn debit(
        &mut self,
        token_id: TokenId,
        amount: u64,
        new_commitment_bytes: Vec<u8>,
    ) -> Result<u64, BalanceError> {
        let balance = self
            .balances
            .get_mut(&token_id)
            .ok_or(BalanceError::TokenNotFound(token_id))?;

        if balance.amount < amount {
            return Err(BalanceError::InsufficientBalance {
                token_id,
                available: balance.amount,
                requested: amount,
            });
        }

        balance.amount -= amount;
        balance.committed_amount = new_commitment_bytes;
        balance.last_updated = Utc::now();

        Ok(balance.amount)
    }

    /// Returns the plaintext balance for a token, or `None` if the token
    /// has never been credited to this wallet.
    pub fn get_balance(&self, token_id: &TokenId) -> Option<u64> {
        self.balances.get(token_id).map(|b| b.amount)
    }

    /// Returns the full [`Balance`] record for a token, including the
    /// commitment bytes and last-updated timestamp.
    pub fn get_balance_record(&self, token_id: &TokenId) -> Option<&Balance> {
        self.balances.get(token_id)
    }

    /// Returns all non-zero balances as `(TokenId, amount)` pairs.
    pub fn all_balances(&self) -> Vec<(TokenId, u64)> {
        self.balances
            .iter()
            .filter(|(_, b)| !b.is_zero())
            .map(|(id, b)| (*id, b.amount))
            .collect()
    }

    /// Returns the number of distinct tokens held (including zero balances).
    pub fn token_count(&self) -> usize {
        self.balances.len()
    }

    /// Returns `true` if this balance sheet has no entries at all.
    pub fn is_empty(&self) -> bool {
        self.balances.is_empty()
    }

    /// Placeholder for cross-currency total valuation.
    ///
    /// In a production system, this would query an oracle or price feed
    /// to convert all balances into `target_currency` and sum them.
    /// For now, it returns `None` -- implementing this properly requires
    /// the oracle module (which doesn't exist yet).
    pub fn total_value_in(&self, _target_currency: Currency) -> Option<u64> {
        // TODO: integrate with oracle/price feed module
        None
    }

    /// Removes a balance entry entirely. Used during wallet cleanup
    /// when a token is fully withdrawn and the user wants a clean sheet.
    pub fn remove_token(&mut self, token_id: &TokenId) -> Option<Balance> {
        self.balances.remove(token_id)
    }
}

impl Default for BalanceSheet {
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
    use crate::vault::token::{brl_token_id, native_token_id};

    fn test_commitment() -> Vec<u8> {
        vec![0xCA, 0xFE, 0xBA, 0xBE]
    }

    #[test]
    fn credit_creates_new_entry() {
        let mut sheet = BalanceSheet::new();
        let token_id = native_token_id();

        let result = sheet.credit(token_id, 1000, test_commitment());
        assert!(result.is_ok());
        assert_eq!(result.unwrap(), 1000);
        assert_eq!(sheet.get_balance(&token_id), Some(1000));
    }

    #[test]
    fn credit_accumulates() {
        let mut sheet = BalanceSheet::new();
        let token_id = native_token_id();

        sheet.credit(token_id, 500, test_commitment()).unwrap();
        sheet.credit(token_id, 300, test_commitment()).unwrap();

        assert_eq!(sheet.get_balance(&token_id), Some(800));
    }

    #[test]
    fn credit_overflow_rejected() {
        let mut sheet = BalanceSheet::new();
        let token_id = native_token_id();

        sheet.credit(token_id, u64::MAX, test_commitment()).unwrap();
        let result = sheet.credit(token_id, 1, test_commitment());
        assert!(result.is_err());
        assert!(matches!(result.unwrap_err(), BalanceError::Overflow { .. }));
    }

    #[test]
    fn debit_reduces_balance() {
        let mut sheet = BalanceSheet::new();
        let token_id = native_token_id();

        sheet.credit(token_id, 1000, test_commitment()).unwrap();
        let remaining = sheet.debit(token_id, 400, test_commitment()).unwrap();

        assert_eq!(remaining, 600);
        assert_eq!(sheet.get_balance(&token_id), Some(600));
    }

    #[test]
    fn debit_to_zero() {
        let mut sheet = BalanceSheet::new();
        let token_id = native_token_id();

        sheet.credit(token_id, 500, test_commitment()).unwrap();
        let remaining = sheet.debit(token_id, 500, test_commitment()).unwrap();

        assert_eq!(remaining, 0);
    }

    #[test]
    fn debit_insufficient_balance_rejected() {
        let mut sheet = BalanceSheet::new();
        let token_id = native_token_id();

        sheet.credit(token_id, 100, test_commitment()).unwrap();
        let result = sheet.debit(token_id, 200, test_commitment());

        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BalanceError::InsufficientBalance {
                available: 100,
                requested: 200,
                ..
            }
        ));
    }

    #[test]
    fn debit_unknown_token_rejected() {
        let mut sheet = BalanceSheet::new();
        let token_id = native_token_id();

        let result = sheet.debit(token_id, 100, test_commitment());
        assert!(result.is_err());
        assert!(matches!(
            result.unwrap_err(),
            BalanceError::TokenNotFound(_)
        ));
    }

    #[test]
    fn get_balance_nonexistent_returns_none() {
        let sheet = BalanceSheet::new();
        let token_id = native_token_id();
        assert_eq!(sheet.get_balance(&token_id), None);
    }

    #[test]
    fn all_balances_excludes_zeros() {
        let mut sheet = BalanceSheet::new();
        let nova = native_token_id();
        let brl = brl_token_id();

        sheet.credit(nova, 1000, test_commitment()).unwrap();
        sheet.credit(brl, 500, test_commitment()).unwrap();
        sheet.debit(brl, 500, test_commitment()).unwrap();

        let non_zero = sheet.all_balances();
        assert_eq!(non_zero.len(), 1);
        assert_eq!(non_zero[0].0, nova);
        assert_eq!(non_zero[0].1, 1000);
    }

    #[test]
    fn multi_token_balances() {
        let mut sheet = BalanceSheet::new();
        let nova = native_token_id();
        let brl = brl_token_id();

        sheet.credit(nova, 5000, test_commitment()).unwrap();
        sheet.credit(brl, 2500, test_commitment()).unwrap();

        assert_eq!(sheet.get_balance(&nova), Some(5000));
        assert_eq!(sheet.get_balance(&brl), Some(2500));
        assert_eq!(sheet.token_count(), 2);
    }

    #[test]
    fn remove_token_clears_entry() {
        let mut sheet = BalanceSheet::new();
        let token_id = native_token_id();

        sheet.credit(token_id, 1000, test_commitment()).unwrap();
        let removed = sheet.remove_token(&token_id);

        assert!(removed.is_some());
        assert_eq!(removed.unwrap().amount, 1000);
        assert_eq!(sheet.get_balance(&token_id), None);
        assert!(sheet.is_empty());
    }

    #[test]
    fn total_value_placeholder_returns_none() {
        let sheet = BalanceSheet::new();
        assert_eq!(sheet.total_value_in(Currency::USD), None);
    }

    #[test]
    fn balance_sheet_serialization_roundtrip() {
        let mut sheet = BalanceSheet::new();
        let token_id = native_token_id();
        sheet.credit(token_id, 42, test_commitment()).unwrap();

        let json = serde_json::to_string(&sheet).expect("serialize");
        let recovered: BalanceSheet = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(recovered.get_balance(&token_id), Some(42));
    }
}
