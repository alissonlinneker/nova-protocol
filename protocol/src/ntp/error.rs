//! Error types for the NOVA Transfer Protocol.
//!
//! Every NTP operation that can fail returns an [`NtpError`]. This enum
//! is exhaustive over the failure modes of the five-step protocol flow.

use thiserror::Error;

/// Errors that can occur during the NTP payment flow.
#[derive(Debug, Error)]
pub enum NtpError {
    /// The handshake could not be completed (version mismatch, bad key, etc.).
    #[error("handshake failed: {0}")]
    HandshakeFailed(String),

    /// The session ID in a message does not match the active session.
    #[error("session mismatch: expected {expected}, got {got}")]
    SessionMismatch {
        /// The session ID we expected.
        expected: String,
        /// The session ID we received.
        got: String,
    },

    /// The protocol version offered by the peer is not supported.
    #[error("unsupported protocol version: {0}")]
    UnsupportedVersion(String),

    /// Currency requested by the receiver is not supported by the sender.
    #[error("unsupported currency: {0}")]
    UnsupportedCurrency(String),

    /// The zero-knowledge proof of funds failed verification.
    #[error("proof of funds verification failed: {0}")]
    ProofVerificationFailed(String),

    /// The sender's balance is insufficient for the requested payment.
    #[error("insufficient funds: required {required}, proof covers {proven}")]
    InsufficientFunds {
        /// Amount required for the payment.
        required: u64,
        /// Amount the proof actually covers.
        proven: u64,
    },

    /// Transaction construction or signing failed.
    #[error("transaction error: {0}")]
    TransactionError(String),

    /// The transaction was rejected by validators.
    #[error("settlement rejected: {0}")]
    SettlementRejected(String),

    /// The settlement timed out before reaching finality.
    #[error("settlement timed out after {elapsed_ms}ms (timeout: {timeout_ms}ms)")]
    SettlementTimeout {
        /// Milliseconds elapsed before giving up.
        elapsed_ms: u64,
        /// Configured timeout in milliseconds.
        timeout_ms: u64,
    },

    /// Receipt signature verification failed.
    #[error("invalid receipt signature: {0}")]
    InvalidReceiptSignature(String),

    /// A cryptographic operation failed (key derivation, encryption, etc.).
    #[error("crypto error: {0}")]
    CryptoError(String),

    /// Serialization or deserialization of a protocol message failed.
    #[error("serialization error: {0}")]
    SerializationError(String),

    /// The protocol state machine received an out-of-order message.
    #[error("unexpected state: in {current_state}, received {message_type}")]
    InvalidState {
        /// The state we are currently in.
        current_state: String,
        /// The message type that was received.
        message_type: String,
    },
}
