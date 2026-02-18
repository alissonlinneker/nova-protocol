//! Transaction construction via the builder pattern.
//!
//! The [`TransactionBuilder`] enforces a disciplined construction flow:
//! set the required fields, call `.build()`, and get back an unsigned
//! [`Transaction`] with a deterministic ID derived from its contents.
//!
//! The builder does not sign -- that happens in [`super::signing`]. This
//! separation keeps construction testable without key material.

use chrono::Utc;
use serde::{Deserialize, Serialize};

use super::types::{Amount, Currency, TransactionType};
use crate::crypto::hash::double_sha256;

// ---------------------------------------------------------------------------
// Transaction
// ---------------------------------------------------------------------------

/// A NOVA protocol transaction.
///
/// This is the fundamental unit of state change on the network. Every
/// transfer, credit operation, and token action is encoded as a `Transaction`.
///
/// The `id` field is the double-SHA-256 hash of the canonical serialization
/// of all fields *except* `signature` and `zkp_proof`. This means the ID
/// is stable across signing -- you can compute it before the transaction is
/// signed and it will not change afterward.
///
/// # Canonical Byte Format
///
/// The signing and ID computation use [`Transaction::signable_bytes`], which
/// deterministically serializes: version, tx_type, sender, receiver, amount
/// value, amount currency, fee, nonce, timestamp, payload. Signature,
/// sender_public_key, and ZKP proof are excluded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct Transaction {
    /// Transaction ID: `hex(double_sha256(signable_bytes))`.
    pub id: String,

    /// Protocol version at the time of creation. Allows validators to
    /// apply the correct rule set during verification.
    pub version: u16,

    /// The operation this transaction represents.
    pub tx_type: TransactionType,

    /// Sender's NOVA address (Bech32-encoded, e.g. `nova1qw508d6...`).
    pub sender: String,

    /// Receiver's NOVA address (Bech32-encoded).
    pub receiver: String,

    /// Transfer amount in the smallest unit of the specified currency.
    pub amount: Amount,

    /// Fee paid to validators, in photons (NOVA smallest unit).
    pub fee: u64,

    /// Monotonically increasing per-sender sequence number.
    /// Prevents replay attacks and enforces transaction ordering.
    pub nonce: u64,

    /// Unix timestamp in milliseconds when the transaction was created.
    pub timestamp: u64,

    /// Optional application-specific payload (smart contract calls,
    /// binary memos, etc.). For human-readable memos, encode as UTF-8.
    pub payload: Option<Vec<u8>>,

    /// Hex-encoded sender public key. Embedded in the transaction so that
    /// validators can verify the signature without a separate key lookup.
    /// Set during signing via [`super::signing::sign_transaction`].
    pub sender_public_key: Option<String>,

    /// Ed25519 signature over [`Transaction::signable_bytes`], hex-encoded.
    /// `None` for unsigned transactions fresh from the builder.
    pub signature: Option<String>,

    /// Optional zero-knowledge proof bytes (e.g., a Groth16 balance proof).
    /// Attached for shielded transactions; `None` for transparent ones.
    pub zkp_proof: Option<Vec<u8>>,
}

impl Transaction {
    /// Returns the canonical byte representation used for signing and ID
    /// computation.
    ///
    /// The format is a deterministic concatenation of fields with null-byte
    /// separators and fixed-width little-endian integers. JSON/serde is
    /// intentionally avoided because field ordering is not guaranteed across
    /// serialization formats.
    ///
    /// Excluded fields: `id`, `sender_public_key`, `signature`, `zkp_proof`.
    pub fn signable_bytes(&self) -> Vec<u8> {
        let mut buf = Vec::with_capacity(256);

        // Protocol version (2 bytes, LE).
        buf.extend_from_slice(&self.version.to_le_bytes());

        // Transaction type discriminant.
        buf.extend_from_slice(format!("{}", self.tx_type).as_bytes());
        buf.push(0x00);

        // Sender address.
        buf.extend_from_slice(self.sender.as_bytes());
        buf.push(0x00);

        // Receiver address.
        buf.extend_from_slice(self.receiver.as_bytes());
        buf.push(0x00);

        // Amount: value as little-endian u64, then currency string.
        buf.extend_from_slice(&self.amount.value.to_le_bytes());
        buf.extend_from_slice(format!("{}", self.amount.currency).as_bytes());
        buf.push(0x00);

        // Fee as little-endian u64.
        buf.extend_from_slice(&self.fee.to_le_bytes());

        // Nonce as little-endian u64.
        buf.extend_from_slice(&self.nonce.to_le_bytes());

        // Timestamp as little-endian u64.
        buf.extend_from_slice(&self.timestamp.to_le_bytes());

        // Payload (length-prefixed if present).
        if let Some(ref payload) = self.payload {
            buf.push(0x01); // payload-present flag
            buf.extend_from_slice(&(payload.len() as u32).to_le_bytes());
            buf.extend_from_slice(payload);
        } else {
            buf.push(0x00); // no-payload flag
        }

        buf
    }

    /// Computes the transaction ID from the current field values.
    ///
    /// `id = hex(double_sha256(signable_bytes))`. Deterministic and
    /// independent of signature/ZKP state.
    pub fn compute_id(&self) -> String {
        let hash = double_sha256(&self.signable_bytes());
        hex::encode(hash)
    }

    /// Returns the total serialized size of the transaction in bytes.
    ///
    /// Uses JSON serialization for a conservative upper bound. Used for
    /// fee calculation (fee-per-byte) and mempool size tracking.
    pub fn size_bytes(&self) -> usize {
        serde_json::to_vec(self).map(|v| v.len()).unwrap_or(0)
    }

    /// Returns the fee per byte, useful for mempool priority ordering.
    pub fn fee_per_byte(&self) -> u64 {
        let size = self.size_bytes() as u64;
        if size == 0 {
            return 0;
        }
        self.fee / size
    }

    /// Returns `true` if the transaction carries a signature.
    pub fn is_signed(&self) -> bool {
        self.signature.is_some()
    }

    /// Returns `true` if the transaction includes a zero-knowledge proof.
    pub fn is_shielded(&self) -> bool {
        self.zkp_proof.is_some()
    }

    /// Returns the transaction ID as a hex string (convenience alias).
    pub fn id_hex(&self) -> String {
        self.id.clone()
    }
}

// ---------------------------------------------------------------------------
// TransactionBuilder
// ---------------------------------------------------------------------------

/// Fluent builder for constructing unsigned [`Transaction`] instances.
///
/// # Usage
///
/// ```rust,no_run
/// use nova_protocol::transaction::{TransactionBuilder, TransactionType};
/// use nova_protocol::transaction::types::{Amount, Currency};
///
/// let tx = TransactionBuilder::new(TransactionType::Transfer)
///     .sender("nova1qw508d6...")
///     .receiver("nova1pk3y7a...")
///     .amount(Amount::new(50_000_000, Currency::NOVA))
///     .fee(1_000)
///     .nonce(1)
///     .build();
/// ```
///
/// The builder sets `version` to the current protocol version and `timestamp`
/// to the current UTC time by default. Both can be overridden.
pub struct TransactionBuilder {
    version: u16,
    tx_type: TransactionType,
    sender: String,
    receiver: String,
    amount: Amount,
    fee: u64,
    nonce: u64,
    timestamp: Option<u64>,
    payload: Option<Vec<u8>>,
}

impl TransactionBuilder {
    /// Creates a new builder for the given transaction type.
    ///
    /// Defaults:
    /// - `version`: 1 (current protocol version)
    /// - `fee`: 0 (caller should set an appropriate fee)
    /// - `nonce`: 0
    /// - `timestamp`: set automatically at build time
    pub fn new(tx_type: TransactionType) -> Self {
        Self {
            version: 1,
            tx_type,
            sender: String::new(),
            receiver: String::new(),
            amount: Amount::new(0, Currency::NOVA),
            fee: 0,
            nonce: 0,
            timestamp: None,
            payload: None,
        }
    }

    /// Sets the protocol version. Only needed for testing version upgrades.
    pub fn version(mut self, version: u16) -> Self {
        self.version = version;
        self
    }

    /// Sets the sender's NOVA address.
    pub fn sender(mut self, address: &str) -> Self {
        self.sender = address.to_string();
        self
    }

    /// Sets the receiver's NOVA address.
    pub fn receiver(mut self, address: &str) -> Self {
        self.receiver = address.to_string();
        self
    }

    /// Sets the transfer amount.
    pub fn amount(mut self, amount: Amount) -> Self {
        self.amount = amount;
        self
    }

    /// Sets the transaction fee in photons.
    pub fn fee(mut self, fee: u64) -> Self {
        self.fee = fee;
        self
    }

    /// Sets the sender's nonce (sequence number).
    pub fn nonce(mut self, nonce: u64) -> Self {
        self.nonce = nonce;
        self
    }

    /// Sets the timestamp explicitly (Unix milliseconds).
    ///
    /// If not called, `build()` will use the current UTC time.
    pub fn timestamp(mut self, timestamp: u64) -> Self {
        self.timestamp = Some(timestamp);
        self
    }

    /// Attaches an application-specific payload.
    pub fn payload(mut self, data: Vec<u8>) -> Self {
        self.payload = Some(data);
        self
    }

    /// Consumes the builder and produces an unsigned [`Transaction`].
    ///
    /// The transaction ID is computed automatically from the signable bytes.
    /// The `signature`, `sender_public_key`, and `zkp_proof` fields are `None`.
    pub fn build(self) -> Transaction {
        let timestamp = self
            .timestamp
            .unwrap_or_else(|| Utc::now().timestamp_millis() as u64);

        let mut tx = Transaction {
            id: String::new(),
            version: self.version,
            tx_type: self.tx_type,
            sender: self.sender,
            receiver: self.receiver,
            amount: self.amount,
            fee: self.fee,
            nonce: self.nonce,
            timestamp,
            payload: self.payload,
            sender_public_key: None,
            signature: None,
            zkp_proof: None,
        };

        tx.id = tx.compute_id();
        tx
    }
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[cfg(test)]
mod tests {
    use super::*;

    fn sample_tx() -> Transaction {
        TransactionBuilder::new(TransactionType::Transfer)
            .sender("nova1aaaa")
            .receiver("nova1bbbb")
            .amount(Amount::new(1_000_000, Currency::NOVA))
            .fee(100)
            .nonce(1)
            .timestamp(1_700_000_000_000)
            .build()
    }

    #[test]
    fn builder_produces_deterministic_id() {
        let tx1 = sample_tx();
        let tx2 = sample_tx();
        assert_eq!(tx1.id, tx2.id, "same inputs must produce the same ID");
        assert!(!tx1.id.is_empty());
    }

    #[test]
    fn id_is_hex_encoded_64_chars() {
        let tx = sample_tx();
        // double_sha256 produces 32 bytes = 64 hex chars.
        assert_eq!(tx.id.len(), 64);
        assert!(tx.id.chars().all(|c| c.is_ascii_hexdigit()));
    }

    #[test]
    fn compute_id_matches_stored_id() {
        let tx = sample_tx();
        assert_eq!(tx.id, tx.compute_id());
    }

    #[test]
    fn different_nonce_different_id() {
        let tx1 = TransactionBuilder::new(TransactionType::Transfer)
            .sender("nova1aaaa")
            .receiver("nova1bbbb")
            .amount(Amount::new(1000, Currency::NOVA))
            .nonce(1)
            .timestamp(1_700_000_000_000)
            .build();

        let tx2 = TransactionBuilder::new(TransactionType::Transfer)
            .sender("nova1aaaa")
            .receiver("nova1bbbb")
            .amount(Amount::new(1000, Currency::NOVA))
            .nonce(2)
            .timestamp(1_700_000_000_000)
            .build();

        assert_ne!(tx1.id, tx2.id);
    }

    #[test]
    fn unsigned_transaction_has_no_signature() {
        let tx = sample_tx();
        assert!(!tx.is_signed());
        assert!(!tx.is_shielded());
    }

    #[test]
    fn size_bytes_is_positive() {
        let tx = sample_tx();
        assert!(tx.size_bytes() > 0);
    }

    #[test]
    fn builder_uses_current_time_if_not_set() {
        let before = Utc::now().timestamp_millis() as u64;
        let tx = TransactionBuilder::new(TransactionType::Transfer)
            .sender("nova1aaaa")
            .receiver("nova1bbbb")
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .build();
        let after = Utc::now().timestamp_millis() as u64;

        assert!(tx.timestamp >= before);
        assert!(tx.timestamp <= after);
    }

    #[test]
    fn transaction_json_roundtrip() {
        let tx = sample_tx();
        let json = serde_json::to_string(&tx).unwrap();
        let recovered: Transaction = serde_json::from_str(&json).unwrap();
        assert_eq!(tx, recovered);
    }

    #[test]
    fn signable_bytes_exclude_signature() {
        let mut tx = sample_tx();
        let bytes_before = tx.signable_bytes();

        tx.signature = Some("deadbeef".to_string());
        let bytes_after = tx.signable_bytes();

        assert_eq!(
            bytes_before, bytes_after,
            "signature must not affect signable bytes"
        );
    }

    #[test]
    fn signable_bytes_exclude_zkp_proof() {
        let mut tx = sample_tx();
        let bytes_before = tx.signable_bytes();

        tx.zkp_proof = Some(vec![0xCA, 0xFE]);
        let bytes_after = tx.signable_bytes();

        assert_eq!(
            bytes_before, bytes_after,
            "zkp_proof must not affect signable bytes"
        );
    }

    #[test]
    fn signable_bytes_exclude_sender_public_key() {
        let mut tx = sample_tx();
        let bytes_before = tx.signable_bytes();

        tx.sender_public_key = Some("abcdef1234".to_string());
        let bytes_after = tx.signable_bytes();

        assert_eq!(
            bytes_before, bytes_after,
            "sender_public_key must not affect signable bytes"
        );
    }

    #[test]
    fn payload_included_in_signable_bytes() {
        let tx_no_payload = TransactionBuilder::new(TransactionType::Transfer)
            .sender("nova1aaaa")
            .receiver("nova1bbbb")
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .timestamp(1_700_000_000_000)
            .build();

        let tx_with_payload = TransactionBuilder::new(TransactionType::Transfer)
            .sender("nova1aaaa")
            .receiver("nova1bbbb")
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .timestamp(1_700_000_000_000)
            .payload(b"hello world".to_vec())
            .build();

        assert_ne!(
            tx_no_payload.signable_bytes(),
            tx_with_payload.signable_bytes(),
            "payload must affect signable bytes"
        );
    }

    #[test]
    fn version_included_in_signable_bytes() {
        let tx_v1 = TransactionBuilder::new(TransactionType::Transfer)
            .version(1)
            .sender("nova1aaaa")
            .receiver("nova1bbbb")
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .timestamp(1_700_000_000_000)
            .build();

        let tx_v2 = TransactionBuilder::new(TransactionType::Transfer)
            .version(2)
            .sender("nova1aaaa")
            .receiver("nova1bbbb")
            .amount(Amount::new(100, Currency::NOVA))
            .nonce(1)
            .timestamp(1_700_000_000_000)
            .build();

        assert_ne!(
            tx_v1.id, tx_v2.id,
            "different version must produce different ID"
        );
    }

    #[test]
    fn default_version_is_one() {
        let tx = sample_tx();
        assert_eq!(tx.version, 1);
    }

    #[test]
    fn fee_per_byte_calculation() {
        let tx = TransactionBuilder::new(TransactionType::Transfer)
            .sender("nova1aaaa")
            .receiver("nova1bbbb")
            .amount(Amount::new(100, Currency::NOVA))
            .fee(10_000)
            .nonce(1)
            .timestamp(1_700_000_000_000)
            .build();

        assert!(tx.fee_per_byte() > 0);
    }
}
