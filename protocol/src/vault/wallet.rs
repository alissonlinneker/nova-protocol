//! # Multi-Asset Wallet
//!
//! A [`Wallet`] is the user-facing abstraction for holding and moving value
//! in the NOVA protocol. It wraps a [`BalanceSheet`] with ownership semantics,
//! nonce tracking (for replay protection), and metadata.
//!
//! ## Nonce Model
//!
//! Every outgoing operation increments the wallet's nonce. This serves two
//! purposes:
//!
//! 1. **Replay protection** -- a transaction signed with nonce `n` is only
//!    valid if the wallet's current nonce is exactly `n`. Validators reject
//!    stale or future nonces.
//!
//! 2. **Ordering** -- within a single wallet, transactions are strictly
//!    ordered by nonce. No ambiguity, no MEV-style reordering.
//!
//! ## Persistence
//!
//! The entire wallet struct derives `Serialize`/`Deserialize` and can be
//! stored in RocksDB as a single key-value pair (key = owner address,
//! value = bincode/JSON blob).

use chrono::{DateTime, Utc};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use thiserror::Error;

use super::balance::{BalanceError, BalanceSheet};
use super::token::TokenId;

// ---------------------------------------------------------------------------
// Errors
// ---------------------------------------------------------------------------

/// Errors that can occur during wallet operations.
#[derive(Debug, Error)]
pub enum WalletError {
    /// A balance operation failed (insufficient funds, overflow, etc.).
    #[error("balance error: {0}")]
    Balance(#[from] BalanceError),

    /// The requested transfer amount is zero, which is a no-op and likely
    /// indicates a bug in the caller.
    #[error("zero-amount operations are not permitted")]
    ZeroAmount,

    /// The wallet is frozen (e.g., compliance hold) and cannot process
    /// outgoing operations.
    #[error("wallet is frozen: {reason}")]
    Frozen {
        /// Human-readable explanation for the freeze.
        reason: String,
    },
}

// ---------------------------------------------------------------------------
// Wallet Metadata
// ---------------------------------------------------------------------------

/// Arbitrary key-value metadata attached to a wallet.
///
/// Used for application-layer data: display name, avatar URI, compliance
/// flags, etc. The protocol doesn't interpret these -- it just stores them.
pub type WalletMetadata = HashMap<String, String>;

// ---------------------------------------------------------------------------
// Wallet
// ---------------------------------------------------------------------------

/// A multi-asset wallet owned by a NOVA identity.
///
/// This is the primary interface for deposits, withdrawals, and transfers.
/// Each wallet is bound to a single owner address and maintains its own
/// nonce sequence for replay protection.
///
/// # Thread Safety
///
/// `Wallet` is `Send` but not `Sync`. Concurrent access should be
/// coordinated at the storage layer (e.g., via `parking_lot::RwLock`
/// or the `DashMap` in the wallet registry).
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct Wallet {
    /// The NOVA address that owns this wallet.
    /// Formatted as `nova:<hex-pubkey>`.
    owner: String,

    /// All token balances held by this wallet.
    balances: BalanceSheet,

    /// Monotonically increasing nonce for replay protection.
    ///
    /// Starts at 0. Incremented before each outgoing operation (withdraw,
    /// transfer_out). Deposits do not increment the nonce because they
    /// are initiated by external parties.
    nonce: u64,

    /// Timestamp when this wallet was created.
    created_at: DateTime<Utc>,

    /// Application-layer metadata (display name, avatar, etc.).
    metadata: WalletMetadata,

    /// If `true`, all outgoing operations are rejected.
    /// Can be set by compliance processes or by the owner themselves.
    frozen: bool,
}

impl Wallet {
    /// Creates a new empty wallet for the given owner address.
    ///
    /// The wallet starts with zero balances, nonce 0, and no metadata.
    ///
    /// # Arguments
    ///
    /// * `owner_address` -- The NOVA address string (e.g., `nova:a3b2c1...`).
    pub fn new(owner_address: &str) -> Self {
        Self {
            owner: owner_address.to_string(),
            balances: BalanceSheet::new(),
            nonce: 0,
            created_at: Utc::now(),
            metadata: HashMap::new(),
            frozen: false,
        }
    }

    /// Returns the owner's NOVA address.
    pub fn owner(&self) -> &str {
        &self.owner
    }

    /// Returns the current nonce value.
    pub fn nonce(&self) -> u64 {
        self.nonce
    }

    /// Returns when this wallet was created.
    pub fn created_at(&self) -> DateTime<Utc> {
        self.created_at
    }

    /// Returns a reference to the wallet's metadata.
    pub fn metadata(&self) -> &WalletMetadata {
        &self.metadata
    }

    /// Returns a mutable reference to the wallet's metadata.
    pub fn metadata_mut(&mut self) -> &mut WalletMetadata {
        &mut self.metadata
    }

    /// Returns `true` if the wallet is frozen.
    pub fn is_frozen(&self) -> bool {
        self.frozen
    }

    /// Returns a reference to the underlying balance sheet.
    pub fn balance_sheet(&self) -> &BalanceSheet {
        &self.balances
    }

    // -----------------------------------------------------------------------
    // Nonce Management
    // -----------------------------------------------------------------------

    /// Increments the nonce and returns the new value.
    ///
    /// Called internally before each outgoing operation. The returned nonce
    /// should be included in the transaction payload for replay protection.
    pub fn next_nonce(&mut self) -> u64 {
        self.nonce += 1;
        self.nonce
    }

    // -----------------------------------------------------------------------
    // Balance Operations
    // -----------------------------------------------------------------------

    /// Deposits funds into the wallet.
    ///
    /// This is an incoming operation -- it does NOT increment the nonce.
    /// External systems (fiat on-ramps, bridge relayers, other wallets)
    /// call this to credit funds.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::ZeroAmount`] if `amount` is 0.
    /// Returns [`WalletError::Balance`] on arithmetic overflow.
    pub fn deposit(&mut self, token_id: TokenId, amount: u64) -> Result<u64, WalletError> {
        if amount == 0 {
            return Err(WalletError::ZeroAmount);
        }

        // In a full implementation, we'd compute the Pedersen commitment here:
        //   let blinding = Fr::rand(&mut rng);
        //   let commitment = zkp::commitment::commit(&params, new_balance, blinding);
        //   let commitment_bytes = commitment.to_bytes();
        //
        // For now, we use a placeholder that hashes the new balance.
        let commitment_bytes = Self::placeholder_commitment(token_id, amount);

        let new_balance = self.balances.credit(token_id, amount, commitment_bytes)?;
        Ok(new_balance)
    }

    /// Withdraws funds from the wallet.
    ///
    /// This is an outgoing operation -- it increments the nonce.
    ///
    /// # Returns
    ///
    /// A tuple of `(remaining_balance, nonce)` on success.
    ///
    /// # Errors
    ///
    /// Returns [`WalletError::Frozen`] if the wallet is frozen.
    /// Returns [`WalletError::ZeroAmount`] if `amount` is 0.
    /// Returns [`WalletError::Balance`] if funds are insufficient.
    pub fn withdraw(&mut self, token_id: TokenId, amount: u64) -> Result<(u64, u64), WalletError> {
        self.check_outgoing()?;

        if amount == 0 {
            return Err(WalletError::ZeroAmount);
        }

        let commitment_bytes = Self::placeholder_commitment(token_id, amount);
        let remaining = self.balances.debit(token_id, amount, commitment_bytes)?;
        let nonce = self.next_nonce();

        Ok((remaining, nonce))
    }

    /// Prepares an outgoing transfer by debiting the wallet and returning
    /// the transfer details needed to construct a transaction.
    ///
    /// This is functionally identical to [`withdraw`](Self::withdraw) but
    /// semantically distinct: a withdrawal exits the NOVA network (off-ramp),
    /// while a transfer_out moves value to another NOVA wallet.
    ///
    /// # Returns
    ///
    /// A [`TransferReceipt`] containing the debited amount, new nonce,
    /// and timestamp.
    ///
    /// # Errors
    ///
    /// Same as [`withdraw`](Self::withdraw).
    pub fn transfer_out(
        &mut self,
        token_id: TokenId,
        amount: u64,
    ) -> Result<TransferReceipt, WalletError> {
        self.check_outgoing()?;

        if amount == 0 {
            return Err(WalletError::ZeroAmount);
        }

        let commitment_bytes = Self::placeholder_commitment(token_id, amount);
        let remaining = self.balances.debit(token_id, amount, commitment_bytes)?;
        let nonce = self.next_nonce();

        Ok(TransferReceipt {
            token_id,
            amount,
            remaining_balance: remaining,
            nonce,
            timestamp: Utc::now(),
        })
    }

    /// Returns the plaintext balance for a specific token.
    pub fn get_balance(&self, token_id: &TokenId) -> Option<u64> {
        self.balances.get_balance(token_id)
    }

    /// Returns all non-zero balances as `(TokenId, amount)` pairs.
    pub fn get_all_balances(&self) -> Vec<(TokenId, u64)> {
        self.balances.all_balances()
    }

    /// Returns the number of distinct tokens held.
    pub fn token_count(&self) -> usize {
        self.balances.token_count()
    }

    // -----------------------------------------------------------------------
    // Freeze / Unfreeze
    // -----------------------------------------------------------------------

    /// Freezes the wallet, preventing all outgoing operations.
    ///
    /// Deposits (incoming) are still allowed on a frozen wallet -- we don't
    /// want to break settlement flows for other parties.
    pub fn freeze(&mut self, reason: &str) {
        self.frozen = true;
        self.metadata
            .insert("freeze_reason".to_string(), reason.to_string());
        self.metadata
            .insert("frozen_at".to_string(), Utc::now().to_rfc3339());
    }

    /// Unfreezes the wallet, restoring normal operation.
    pub fn unfreeze(&mut self) {
        self.frozen = false;
        self.metadata.remove("freeze_reason");
        self.metadata.remove("frozen_at");
    }

    // -----------------------------------------------------------------------
    // Internal Helpers
    // -----------------------------------------------------------------------

    /// Validates that the wallet can perform outgoing operations.
    fn check_outgoing(&self) -> Result<(), WalletError> {
        if self.frozen {
            let reason = self
                .metadata
                .get("freeze_reason")
                .cloned()
                .unwrap_or_else(|| "no reason provided".to_string());
            return Err(WalletError::Frozen { reason });
        }
        Ok(())
    }

    /// Generates a placeholder commitment for balance updates.
    ///
    /// In production, this will be replaced with a real Pedersen commitment
    /// computed from the wallet's blinding factor key material. The placeholder
    /// uses BLAKE3 so that the bytes are at least unique per (token, amount).
    fn placeholder_commitment(token_id: TokenId, amount: u64) -> Vec<u8> {
        use crate::crypto::hash::blake3_hash;

        let mut preimage = Vec::with_capacity(40);
        preimage.extend_from_slice(token_id.as_bytes());
        preimage.extend_from_slice(&amount.to_le_bytes());
        blake3_hash(&preimage).to_vec()
    }
}

// ---------------------------------------------------------------------------
// TransferReceipt
// ---------------------------------------------------------------------------

/// Receipt returned by [`Wallet::transfer_out`] with the details needed
/// to construct the on-chain transfer transaction.
///
/// The caller uses these fields to build and sign a `TransactionType::Transfer`
/// payload before submitting it to the network.
#[derive(Clone, Debug, Serialize, Deserialize)]
pub struct TransferReceipt {
    /// The token that was debited.
    pub token_id: TokenId,

    /// The amount that was debited (in smallest units).
    pub amount: u64,

    /// The sender's remaining balance after the debit.
    pub remaining_balance: u64,

    /// The nonce assigned to this operation (for replay protection).
    pub nonce: u64,

    /// When the debit was executed (UTC).
    pub timestamp: DateTime<Utc>,
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::token::{brl_token_id, native_token_id};

    const TEST_OWNER: &str =
        "nova:aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa";

    #[test]
    fn new_wallet_is_empty() {
        let w = Wallet::new(TEST_OWNER);
        assert_eq!(w.owner(), TEST_OWNER);
        assert_eq!(w.nonce(), 0);
        assert_eq!(w.token_count(), 0);
        assert!(w.get_all_balances().is_empty());
        assert!(!w.is_frozen());
    }

    #[test]
    fn deposit_credits_balance() {
        let mut w = Wallet::new(TEST_OWNER);
        let token = native_token_id();

        let balance = w.deposit(token, 5000).unwrap();
        assert_eq!(balance, 5000);
        assert_eq!(w.get_balance(&token), Some(5000));
        // Deposits don't increment nonce.
        assert_eq!(w.nonce(), 0);
    }

    #[test]
    fn deposit_accumulates() {
        let mut w = Wallet::new(TEST_OWNER);
        let token = native_token_id();

        w.deposit(token, 1000).unwrap();
        w.deposit(token, 2000).unwrap();
        assert_eq!(w.get_balance(&token), Some(3000));
    }

    #[test]
    fn deposit_zero_rejected() {
        let mut w = Wallet::new(TEST_OWNER);
        let result = w.deposit(native_token_id(), 0);
        assert!(matches!(result, Err(WalletError::ZeroAmount)));
    }

    #[test]
    fn withdraw_debits_and_increments_nonce() {
        let mut w = Wallet::new(TEST_OWNER);
        let token = native_token_id();

        w.deposit(token, 5000).unwrap();
        let (remaining, nonce) = w.withdraw(token, 2000).unwrap();

        assert_eq!(remaining, 3000);
        assert_eq!(nonce, 1);
        assert_eq!(w.nonce(), 1);
        assert_eq!(w.get_balance(&token), Some(3000));
    }

    #[test]
    fn withdraw_insufficient_funds() {
        let mut w = Wallet::new(TEST_OWNER);
        let token = native_token_id();

        w.deposit(token, 100).unwrap();
        let result = w.withdraw(token, 200);
        assert!(matches!(
            result,
            Err(WalletError::Balance(
                BalanceError::InsufficientBalance { .. }
            ))
        ));
        // Nonce should NOT have been incremented on failure.
        assert_eq!(w.nonce(), 0);
    }

    #[test]
    fn withdraw_zero_rejected() {
        let mut w = Wallet::new(TEST_OWNER);
        let token = native_token_id();
        w.deposit(token, 100).unwrap();

        let result = w.withdraw(token, 0);
        assert!(matches!(result, Err(WalletError::ZeroAmount)));
    }

    #[test]
    fn transfer_out_returns_receipt() {
        let mut w = Wallet::new(TEST_OWNER);
        let token = native_token_id();

        w.deposit(token, 10000).unwrap();
        let receipt = w.transfer_out(token, 3000).unwrap();

        assert_eq!(receipt.token_id, token);
        assert_eq!(receipt.amount, 3000);
        assert_eq!(receipt.remaining_balance, 7000);
        assert_eq!(receipt.nonce, 1);
        assert_eq!(w.get_balance(&token), Some(7000));
    }

    #[test]
    fn transfer_out_insufficient_funds() {
        let mut w = Wallet::new(TEST_OWNER);
        let token = native_token_id();

        w.deposit(token, 100).unwrap();
        let result = w.transfer_out(token, 500);
        assert!(result.is_err());
    }

    #[test]
    fn nonce_increments_correctly() {
        let mut w = Wallet::new(TEST_OWNER);
        let token = native_token_id();

        w.deposit(token, 100_000).unwrap();
        w.withdraw(token, 100).unwrap(); // nonce -> 1
        w.withdraw(token, 200).unwrap(); // nonce -> 2
        w.transfer_out(token, 300).unwrap(); // nonce -> 3

        assert_eq!(w.nonce(), 3);
    }

    #[test]
    fn next_nonce_manual() {
        let mut w = Wallet::new(TEST_OWNER);
        assert_eq!(w.next_nonce(), 1);
        assert_eq!(w.next_nonce(), 2);
        assert_eq!(w.next_nonce(), 3);
    }

    #[test]
    fn multi_token_wallet() {
        let mut w = Wallet::new(TEST_OWNER);
        let nova = native_token_id();
        let brl = brl_token_id();

        w.deposit(nova, 5000).unwrap();
        w.deposit(brl, 10000).unwrap();

        assert_eq!(w.get_balance(&nova), Some(5000));
        assert_eq!(w.get_balance(&brl), Some(10000));
        assert_eq!(w.token_count(), 2);

        let all = w.get_all_balances();
        assert_eq!(all.len(), 2);
    }

    #[test]
    fn frozen_wallet_rejects_withdrawals() {
        let mut w = Wallet::new(TEST_OWNER);
        let token = native_token_id();

        w.deposit(token, 5000).unwrap();
        w.freeze("compliance investigation");

        assert!(w.is_frozen());
        let result = w.withdraw(token, 100);
        assert!(matches!(result, Err(WalletError::Frozen { .. })));

        // Deposits still work on a frozen wallet.
        let balance = w.deposit(token, 500).unwrap();
        assert_eq!(balance, 5500);
    }

    #[test]
    fn frozen_wallet_rejects_transfers() {
        let mut w = Wallet::new(TEST_OWNER);
        let token = native_token_id();

        w.deposit(token, 5000).unwrap();
        w.freeze("sanctions screening");

        let result = w.transfer_out(token, 100);
        assert!(matches!(result, Err(WalletError::Frozen { .. })));
    }

    #[test]
    fn unfreeze_restores_operations() {
        let mut w = Wallet::new(TEST_OWNER);
        let token = native_token_id();

        w.deposit(token, 5000).unwrap();
        w.freeze("temporary hold");
        w.unfreeze();

        assert!(!w.is_frozen());
        let (remaining, _nonce) = w.withdraw(token, 100).unwrap();
        assert_eq!(remaining, 4900);
    }

    #[test]
    fn metadata_operations() {
        let mut w = Wallet::new(TEST_OWNER);

        w.metadata_mut()
            .insert("display_name".to_string(), "Alice".to_string());
        assert_eq!(w.metadata().get("display_name").unwrap(), "Alice");
    }

    #[test]
    fn wallet_serialization_roundtrip() {
        let mut w = Wallet::new(TEST_OWNER);
        let token = native_token_id();

        w.deposit(token, 42000).unwrap();
        w.withdraw(token, 2000).unwrap();
        w.metadata_mut()
            .insert("name".to_string(), "Test Wallet".to_string());

        let json = serde_json::to_string(&w).expect("serialize");
        let recovered: Wallet = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(recovered.owner(), TEST_OWNER);
        assert_eq!(recovered.nonce(), 1);
        assert_eq!(recovered.get_balance(&token), Some(40000));
        assert_eq!(recovered.metadata().get("name").unwrap(), "Test Wallet");
    }

    #[test]
    fn transfer_receipt_serialization() {
        let mut w = Wallet::new(TEST_OWNER);
        let token = native_token_id();

        w.deposit(token, 10000).unwrap();
        let receipt = w.transfer_out(token, 3000).unwrap();

        let json = serde_json::to_string(&receipt).expect("serialize");
        let recovered: TransferReceipt = serde_json::from_str(&json).expect("deserialize");

        assert_eq!(recovered.amount, 3000);
        assert_eq!(recovered.remaining_balance, 7000);
        assert_eq!(recovered.nonce, 1);
    }
}
