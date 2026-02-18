//! Transaction verification: structural checks and cryptographic validation.
//!
//! Every transaction entering the mempool or proposed in a block must pass
//! [`verify_transaction`]. The checks are ordered from cheapest to most
//! expensive (string comparisons before signature verification) to fail
//! fast and waste minimal CPU on invalid transactions.

use chrono::Utc;
use thiserror::Error;

use super::builder::Transaction;
use super::types::TransactionType;
use crate::crypto::keys::{NovaPublicKey, NovaSignature};
use crate::identity::nova_id::NovaId;
use crate::zkp::prover::BalanceProof;

// ---------------------------------------------------------------------------
// Error types
// ---------------------------------------------------------------------------

/// Errors that can occur during transaction verification.
///
/// Each variant maps to a specific validation rule. The error message
/// includes enough context for debugging without leaking internal state.
#[derive(Debug, Error)]
pub enum TransactionError {
    /// The transaction ID does not match the double-SHA-256 of its signable bytes.
    #[error("transaction ID mismatch: expected {expected}, got {actual}")]
    IdMismatch { expected: String, actual: String },

    /// The transaction is not signed (signature field is `None`).
    #[error("transaction is unsigned")]
    MissingSignature,

    /// The signature is malformed (cannot be decoded from hex or wrong length).
    #[error("malformed signature: {reason}")]
    MalformedSignature { reason: String },

    /// The Ed25519 signature does not verify against the sender's public key.
    #[error("invalid signature: does not verify against sender {sender}")]
    InvalidSignature { sender: String },

    /// The sender address cannot be parsed as a valid NOVA address.
    #[error("invalid sender address: {address}")]
    InvalidSenderAddress { address: String },

    /// The nonce is zero, which is reserved. Valid nonces start at 1.
    #[error("invalid nonce: must be > 0, got {nonce}")]
    InvalidNonce { nonce: u64 },

    /// The transaction amount is zero.
    #[error("amount must be > 0")]
    ZeroAmount,

    /// The sender and receiver are the same address.
    #[error("sender and receiver must differ: both are {address}")]
    SelfTransfer { address: String },

    /// The transaction timestamp is too far in the future.
    #[error("timestamp {timestamp_ms} is {delta_secs}s in the future (max allowed: {max_secs}s)")]
    TimestampTooFarInFuture {
        timestamp_ms: u64,
        delta_secs: i64,
        max_secs: i64,
    },

    /// A `ConfidentialTransfer` is missing its required ZKP proof.
    #[error("confidential transfer requires a Groth16 proof")]
    MissingProof,

    /// A `ConfidentialTransfer` is missing its required Pedersen commitment.
    #[error("confidential transfer requires an amount commitment")]
    MissingCommitment,

    /// The attached ZKP proof could not be deserialized.
    #[error("invalid ZKP proof: {reason}")]
    InvalidProof { reason: String },
}

// ---------------------------------------------------------------------------
// Verification
// ---------------------------------------------------------------------------

/// Maximum allowed clock skew for transaction timestamps, in seconds.
/// Transactions with timestamps more than this many seconds in the future
/// are rejected. 5 minutes matches the mempool TTL.
const MAX_FUTURE_SECONDS: i64 = 300;

/// Verifies a signed transaction for structural correctness and cryptographic
/// validity.
///
/// The checks, in order:
///
/// 1. **Nonce** — must be > 0.
/// 2. **Amount** — must be > 0.
/// 3. **Self-transfer** — sender must differ from receiver.
/// 4. **Timestamp** — must not be more than 5 minutes in the future.
/// 5. **Transaction ID** — must equal `double_sha256(signable_bytes)`.
/// 6. **Signature present** — the transaction must be signed.
/// 7. **Sender address valid** — must parse as a `nova:<hex>` address.
/// 8. **Signature valid** — Ed25519 verification against the sender's public key.
/// 9. **ConfidentialTransfer fields** — proof and commitment required.
/// 10. **ZKP structural validity** — if proof attached, must deserialize.
///
/// # Errors
///
/// Returns the first failing check as a [`TransactionError`]. Checks are
/// ordered from cheapest to most expensive to minimize wasted computation
/// on clearly invalid transactions.
pub fn verify_transaction(tx: &Transaction) -> Result<(), TransactionError> {
    // 1. Nonce must be positive (0 is reserved for genesis/system txs).
    if tx.nonce == 0 {
        return Err(TransactionError::InvalidNonce { nonce: tx.nonce });
    }

    // 2. Amount must be non-zero.
    if tx.amount.value == 0 {
        return Err(TransactionError::ZeroAmount);
    }

    // 3. No self-transfers.
    if tx.sender == tx.receiver {
        return Err(TransactionError::SelfTransfer {
            address: tx.sender.clone(),
        });
    }

    // 4. Timestamp must not be unreasonably far in the future.
    let now_ms = Utc::now().timestamp_millis() as u64;
    let max_future_ms = now_ms + (MAX_FUTURE_SECONDS as u64 * 1_000);
    if tx.timestamp > max_future_ms {
        let delta_secs = (tx.timestamp as i64 - now_ms as i64) / 1_000;
        return Err(TransactionError::TimestampTooFarInFuture {
            timestamp_ms: tx.timestamp,
            delta_secs,
            max_secs: MAX_FUTURE_SECONDS,
        });
    }

    // 5. Transaction ID integrity check.
    let expected_id = tx.compute_id();
    if tx.id != expected_id {
        return Err(TransactionError::IdMismatch {
            expected: expected_id,
            actual: tx.id.clone(),
        });
    }

    // 6. Signature must be present.
    let sig_hex = tx
        .signature
        .as_ref()
        .ok_or(TransactionError::MissingSignature)?;

    // 7. Decode signature from hex.
    let sig_bytes = hex::decode(sig_hex).map_err(|e| TransactionError::MalformedSignature {
        reason: format!("hex decode failed: {}", e),
    })?;

    if sig_bytes.len() != 64 {
        return Err(TransactionError::MalformedSignature {
            reason: format!("expected 64 bytes, got {}", sig_bytes.len()),
        });
    }

    let mut sig_arr = [0u8; 64];
    sig_arr.copy_from_slice(&sig_bytes);
    let signature = NovaSignature::from_bytes(sig_arr);

    // 8. Parse sender address to validate it's a well-formed NOVA address.
    let _sender_id =
        NovaId::from_address(&tx.sender).map_err(|_| TransactionError::InvalidSenderAddress {
            address: tx.sender.clone(),
        })?;

    // 9. Verify the signature against the sender's public key.
    //    The sender's public key is extracted from the `sender_pubkey` field
    //    on the transaction. We also verify that the public key hashes to the
    //    sender address to prevent key substitution attacks.
    let sender_pk_hex =
        tx.sender_public_key
            .as_ref()
            .ok_or_else(|| TransactionError::InvalidSenderAddress {
                address: tx.sender.clone(),
            })?;
    let sender_pk = NovaPublicKey::from_hex(sender_pk_hex).map_err(|_| {
        TransactionError::InvalidSenderAddress {
            address: tx.sender.clone(),
        }
    })?;

    // Verify the public key maps to the claimed sender address.
    let derived_id = NovaId::from_public_key(&sender_pk);
    if derived_id.to_address() != tx.sender {
        return Err(TransactionError::InvalidSenderAddress {
            address: tx.sender.clone(),
        });
    }

    let signable = tx.signable_bytes();
    if !sender_pk.verify(&signable, &signature) {
        return Err(TransactionError::InvalidSignature {
            sender: tx.sender.clone(),
        });
    }

    // 10. ConfidentialTransfer type REQUIRES both a proof and commitment.
    if tx.tx_type == TransactionType::ConfidentialTransfer {
        if tx.proof.is_none() {
            return Err(TransactionError::MissingProof);
        }
        if tx.amount_commitment.is_none() {
            return Err(TransactionError::MissingCommitment);
        }
    }

    // 11. ZKP proof verification — if a proof is attached, validate that
    //     it is at least well-formed (deserializable as a Groth16 proof).
    //     Full semantic verification (against a specific commitment and
    //     required amount) requires the BalanceVerifier, which lives at the
    //     node layer. Here we perform structural validation only.
    if let Some(ref proof_bytes) = tx.proof {
        BalanceProof::from_bytes(proof_bytes).map_err(|e| TransactionError::InvalidProof {
            reason: e.to_string(),
        })?;
    }

    Ok(())
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

    /// Helper: build and sign a valid transaction.
    fn valid_signed_tx() -> (Transaction, NovaKeypair) {
        let kp = NovaKeypair::generate();
        let sender_addr = NovaId::from_public_key(&kp.public_key()).to_address();
        let receiver_kp = NovaKeypair::generate();
        let receiver_addr = NovaId::from_public_key(&receiver_kp.public_key()).to_address();

        let mut tx = TransactionBuilder::new(TransactionType::Transfer)
            .sender(&sender_addr)
            .receiver(&receiver_addr)
            .amount(Amount::new(1_000, Currency::NOVA))
            .fee(100)
            .nonce(1)
            .build();

        sign_transaction(&mut tx, &kp);
        (tx, kp)
    }

    #[test]
    fn valid_transaction_passes() {
        let (tx, _) = valid_signed_tx();
        assert!(verify_transaction(&tx).is_ok());
    }

    #[test]
    fn rejects_zero_nonce() {
        let (mut tx, kp) = valid_signed_tx();
        tx.nonce = 0;
        tx.id = tx.compute_id();
        sign_transaction(&mut tx, &kp);

        match verify_transaction(&tx) {
            Err(TransactionError::InvalidNonce { nonce: 0 }) => {}
            other => panic!("expected InvalidNonce, got {:?}", other),
        }
    }

    #[test]
    fn rejects_zero_amount() {
        let kp = NovaKeypair::generate();
        let sender_addr = NovaId::from_public_key(&kp.public_key()).to_address();
        let receiver_kp = NovaKeypair::generate();
        let receiver_addr = NovaId::from_public_key(&receiver_kp.public_key()).to_address();

        let mut tx = TransactionBuilder::new(TransactionType::Transfer)
            .sender(&sender_addr)
            .receiver(&receiver_addr)
            .amount(Amount::new(0, Currency::NOVA))
            .nonce(1)
            .build();
        sign_transaction(&mut tx, &kp);

        match verify_transaction(&tx) {
            Err(TransactionError::ZeroAmount) => {}
            other => panic!("expected ZeroAmount, got {:?}", other),
        }
    }

    #[test]
    fn rejects_self_transfer() {
        let kp = NovaKeypair::generate();
        let addr = NovaId::from_public_key(&kp.public_key()).to_address();

        let mut tx = TransactionBuilder::new(TransactionType::Transfer)
            .sender(&addr)
            .receiver(&addr)
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .build();
        sign_transaction(&mut tx, &kp);

        match verify_transaction(&tx) {
            Err(TransactionError::SelfTransfer { .. }) => {}
            other => panic!("expected SelfTransfer, got {:?}", other),
        }
    }

    #[test]
    fn rejects_future_timestamp() {
        let kp = NovaKeypair::generate();
        let sender_addr = NovaId::from_public_key(&kp.public_key()).to_address();
        let receiver_kp = NovaKeypair::generate();
        let receiver_addr = NovaId::from_public_key(&receiver_kp.public_key()).to_address();

        let far_future = Utc::now().timestamp_millis() as u64 + 600_000; // +10 min

        let mut tx = TransactionBuilder::new(TransactionType::Transfer)
            .sender(&sender_addr)
            .receiver(&receiver_addr)
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .timestamp(far_future)
            .build();
        sign_transaction(&mut tx, &kp);

        match verify_transaction(&tx) {
            Err(TransactionError::TimestampTooFarInFuture { .. }) => {}
            other => panic!("expected TimestampTooFarInFuture, got {:?}", other),
        }
    }

    #[test]
    fn rejects_tampered_id() {
        let (mut tx, _) = valid_signed_tx();
        tx.id = "0000000000000000000000000000000000000000000000000000000000000000".to_string();

        match verify_transaction(&tx) {
            Err(TransactionError::IdMismatch { .. }) => {}
            other => panic!("expected IdMismatch, got {:?}", other),
        }
    }

    #[test]
    fn rejects_unsigned_transaction() {
        let kp = NovaKeypair::generate();
        let sender_addr = NovaId::from_public_key(&kp.public_key()).to_address();
        let receiver_kp = NovaKeypair::generate();
        let receiver_addr = NovaId::from_public_key(&receiver_kp.public_key()).to_address();

        let tx = TransactionBuilder::new(TransactionType::Transfer)
            .sender(&sender_addr)
            .receiver(&receiver_addr)
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .build();

        match verify_transaction(&tx) {
            Err(TransactionError::MissingSignature) => {}
            other => panic!("expected MissingSignature, got {:?}", other),
        }
    }

    #[test]
    fn rejects_wrong_keypair_signature() {
        let kp_sender = NovaKeypair::generate();
        let kp_wrong = NovaKeypair::generate();
        let sender_addr = NovaId::from_public_key(&kp_sender.public_key()).to_address();
        let receiver_kp = NovaKeypair::generate();
        let receiver_addr = NovaId::from_public_key(&receiver_kp.public_key()).to_address();

        let mut tx = TransactionBuilder::new(TransactionType::Transfer)
            .sender(&sender_addr)
            .receiver(&receiver_addr)
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .build();

        // Sign with the WRONG keypair (sets sender_public_key to kp_wrong's key).
        sign_transaction(&mut tx, &kp_wrong);

        // Override sender_public_key to the REAL sender's key so the address
        // derivation check passes, but the signature (produced by kp_wrong)
        // will fail Ed25519 verification against kp_sender's public key.
        tx.sender_public_key = Some(kp_sender.public_key().to_hex());

        match verify_transaction(&tx) {
            Err(TransactionError::InvalidSignature { .. }) => {}
            other => panic!("expected InvalidSignature, got {:?}", other),
        }
    }

    #[test]
    fn rejects_invalid_sender_address() {
        let (mut tx, kp) = valid_signed_tx();
        tx.sender = "btc:not_a_nova_address".to_string();
        tx.id = tx.compute_id();
        sign_transaction(&mut tx, &kp);

        match verify_transaction(&tx) {
            Err(TransactionError::InvalidSenderAddress { .. }) => {}
            other => panic!("expected InvalidSenderAddress, got {:?}", other),
        }
    }

    #[test]
    fn accepts_near_future_timestamp() {
        let kp = NovaKeypair::generate();
        let sender_addr = NovaId::from_public_key(&kp.public_key()).to_address();
        let receiver_kp = NovaKeypair::generate();
        let receiver_addr = NovaId::from_public_key(&receiver_kp.public_key()).to_address();

        // 2 minutes in the future should be fine (< 5 min limit).
        let near_future = Utc::now().timestamp_millis() as u64 + 120_000;

        let mut tx = TransactionBuilder::new(TransactionType::Transfer)
            .sender(&sender_addr)
            .receiver(&receiver_addr)
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .timestamp(near_future)
            .build();
        sign_transaction(&mut tx, &kp);

        assert!(verify_transaction(&tx).is_ok());
    }

    #[test]
    fn accepts_past_timestamp() {
        let kp = NovaKeypair::generate();
        let sender_addr = NovaId::from_public_key(&kp.public_key()).to_address();
        let receiver_kp = NovaKeypair::generate();
        let receiver_addr = NovaId::from_public_key(&receiver_kp.public_key()).to_address();

        // 1 hour in the past.
        let past = Utc::now().timestamp_millis() as u64 - 3_600_000;

        let mut tx = TransactionBuilder::new(TransactionType::Transfer)
            .sender(&sender_addr)
            .receiver(&receiver_addr)
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .timestamp(past)
            .build();
        sign_transaction(&mut tx, &kp);

        assert!(verify_transaction(&tx).is_ok());
    }
}
